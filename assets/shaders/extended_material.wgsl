// File mostly taken from https://github.com/janhohenheim/bevy_wind_waker_shader
// dual licensed MIT / Apache 2.0


#import bevy_pbr::{
    pbr_fragment::pbr_input_from_vertex_output,
    pbr_functions::alpha_discard,
    mesh_view_bindings as view_bindings,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

@group(2) @binding(100) var mask: texture_2d<f32>;
@group(2) @binding(101) var mask_sampler: sampler;
@group(2) @binding(102) var<uniform> highlight_color: vec4<f32>;
@group(2) @binding(103) var<uniform> shadow_color: vec4<f32>;
@group(2) @binding(104) var<uniform> rim_color: vec4<f32>;
@group(2) @binding(105) var<uniform> grass_color: vec4<f32>;
@group(2) @binding(106) var<uniform> ocean_color: vec4<f32>;
@group(2) @binding(107) var<uniform> mountain_color: vec4<f32>;
@group(2) @binding(108) var<uniform> snow_color: vec4<f32>;
@group(2) @binding(109) var<uniform> sand_color: vec4<f32>;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_vertex_output(in, is_front, false);

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
    if height < 0.37 {
        texture = ocean_color;
    }
    else if height < 0.38 {
        texture = mix(ocean_color, sand_color, (height - 0.37)/ 0.01);
    }
    else if height < 0.39 {
        texture = sand_color;
    }
    else if height < 0.40 {
        texture = mix(sand_color, grass_color, (height - 0.39)/0.01);
    }
    else if height < 0.55 {
        texture = grass_color;
    }

    else if height < 0.6 {
        texture = mix(grass_color, mountain_color, (height - 0.55)/0.05);
    }
    else if height < 0.75 {
        texture = mountain_color;
    }
    else {
        texture = snow_color;
    }
    pbr_input.material.base_color = vec4<f32>(1.0, 1.0, 1.0, 1.0);

    out.color = apply_pbr_lighting(pbr_input);

    // Source for cel shading: https://www.youtube.com/watch?v=mnxs6CR6Zrk]
    // sample mask at the current fragment's intensity as u to get the cutoff
    let uv = vec2<f32>(out.color.r, 0.0);
    let quantization = textureSample(mask, mask_sampler, uv);
    out.color = mix(shadow_color, highlight_color, quantization);

    // apply rim highlights. Inspired by Breath of the Wild: https://www.youtube.com/watch?v=By7qcgaqGI4
    let eye = normalize(view_bindings::view.world_position.xyz - in.world_position.xyz);
    let rim = 1.0 - abs(dot(eye, in.world_normal));
    let rim_factor = rim * rim * rim * rim;
    out.color = mix(out.color, rim_color, rim_factor);

    // Reapply texture
    out.color = out.color * texture;
    pbr_input.material.base_color = texture;

    // apply in-shader post processing (fog, alpha-premultiply, and also tonemapping, debanding if the camera is non-hdr)
    // note this does not include fullscreen postprocessing effects like bloom.
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}