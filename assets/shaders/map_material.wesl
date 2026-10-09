// File mostly taken from https://github.com/janhohenheim/bevy_wind_waker_shader
// dual licensed MIT / Apache 2.0

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@if(VISIBILITY_RANGE_DITHER)
import bevy_pbr::render::pbr_functions::visibility_range_dither;

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> grass_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> ocean_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> mountain_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> snow_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var<uniform> sand_color: vec4<f32>;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    // If we're in the crossfade section of a visibility range, conditionally
    // discard the fragment according to the visibility pattern.
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // alpha discard
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color
    );

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    // in deferred mode we can't modify anything after that, as lighting is run in a separate fullscreen shader.
    out = deferred_output(in, pbr_input);
} @else {
    // remove texture
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

    texture = mix(texture, ocean_color, mix_hydro);

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
}

    return out;
}
