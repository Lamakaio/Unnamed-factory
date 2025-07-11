// File mostly taken from https://github.com/janhohenheim/bevy_wind_waker_shader
// dual licensed MIT / Apache 2.0


#import bevy_pbr::{
    pbr_fragment::pbr_input_from_vertex_output,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    mesh_view_bindings as view_bindings,
    decal::clustered::apply_decal_base_color,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

#ifdef FORWARD_DECAL
#import bevy_pbr::decal::forward::get_forward_decal_info
#endif

@group(2) @binding(100) var<uniform> grass_color: vec4<f32>;
@group(2) @binding(101) var<uniform> ocean_color: vec4<f32>;
@group(2) @binding(102) var<uniform> mountain_color: vec4<f32>;
@group(2) @binding(103) var<uniform> snow_color: vec4<f32>;
@group(2) @binding(104) var<uniform> sand_color: vec4<f32>;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    // If we're in the crossfade section of a visibility range, conditionally
    // discard the fragment according to the visibility pattern.
#ifdef VISIBILITY_RANGE_DITHER
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
#endif

#ifdef FORWARD_DECAL
    let forward_decal_info = get_forward_decal_info(in);
    in.world_position = forward_decal_info.world_position;
    in.uv = forward_decal_info.uv;
#endif


    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // alpha discard
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);


    
#ifdef PREPASS_PIPELINE
    // in deferred mode we can't modify anything after that, as lighting is run in a separate fullscreen shader.
    let out = deferred_output(in, pbr_input);
#else

    // remove texture
    var out: FragmentOutput;
    var texture: vec4<f32>;
    let height = in.uv.x;

    let hydro = (abs(in.uv.y) - 0.94) / 100.;
    let mix_hydro = exp(-(1./hydro));

    if height < 0.34 {
        texture = ocean_color;
    }
    else if height < 0.345 {
        texture = mix(ocean_color, sand_color, (height - 0.34)/ 0.005);
    }
    else if height < 0.34 {
        texture = sand_color;
    }
    else if height < 0.35 {
        texture = mix(sand_color, grass_color, (height - 0.345)/0.005);
    }
    else if height < 0.42 {
        texture = grass_color;
    }
    else if height < 0.47 {
        texture = mix(grass_color, mountain_color, (height - 0.42)/0.05);
    }
    else if height < 0.55 {
        texture = mountain_color;
    }
    else {
        texture = snow_color;
    }

    // texture = mix(texture, ocean_color, mix_hydro);

    texture = apply_decal_base_color(
        in.world_position.xyz,
        in.position.xy,
        texture
    );


    pbr_input.material.base_color = vec4<f32>(1.0, 1.0, 1.0, 1.0);

    out.color = apply_pbr_lighting(pbr_input);

    // Source for cel shading: https://www.youtube.com/watch?v=mnxs6CR6Zrk]
    // sample mask at the current fragment's intensity as u to get the cutoff
    //let uv = vec2<f32>(out.color.r, 0.0);
    //let quantization = textureSample(mask, mask_sampler, uv);
    //out.color = mix(shadow_color, highlight_color, quantization);

    // apply rim highlights. Inspired by Breath of the Wild: https://www.youtube.com/watch?v=By7qcgaqGI4
    //let eye = normalize(view_bindings::view.world_position.xyz - in.world_position.xyz);
    //let rim = 1.0 - abs(dot(eye, in.world_normal));
    //let rim_factor = rim * rim * rim * rim;
    //out.color = mix(out.color, rim_color, rim_factor);

    // Reapply texture
    out.color = out.color * texture;
    pbr_input.material.base_color = texture;

    // apply in-shader post processing (fog, alpha-premultiply, and also tonemapping, debanding if the camera is non-hdr)
    // note this does not include fullscreen postprocessing effects like bloom.
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

#ifdef FORWARD_DECAL
    out.color.a = min(forward_decal_info.alpha, out.color.a);
#endif

    return out;
}