// Terrain material: standard PBR with the ground color from the mesh vertex colors (biomes),
// multiplied by `tint`.
//
// Normals are per pixel: the base surface gradient comes from a texture covering the
// continent, plus the gradient added by rivers and edits (mesh UVs). Lighting keeps the full
// surface detail at any mesh LOD. Steep ground turns to bare rock. Originally based on https://github.com/janhohenheim/bevy_wind_waker_shader
// (dual licensed MIT / Apache 2.0); a place for terrain stylization.

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

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> tint: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var gradient_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var gradient_sampler: sampler;
// Texture coordinates from world xz: `uv = xz * zw + xy`. Zero until the texture is bound.
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> gradient_map: vec4<f32>;

// Linear color of bare rock, and the slope range (1 - normal.y) where ground turns to it.
const ROCK: vec3<f32> = vec3(0.171, 0.147, 0.133);
const ROCK_STEEPNESS: vec2<f32> = vec2(0.3, 0.55);

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

    // Includes the vertex colors.
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color * tint
    );

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    // In deferred mode, lighting runs in a separate fullscreen pass.
    out = deferred_output(in, pbr_input);
} @else {
    if gradient_map.z > 0.0 {
        var gradient = textureSample(
            gradient_texture,
            gradient_sampler,
            in.world_position.xz * gradient_map.zw + gradient_map.xy
        ).xy;
@if(VERTEX_UVS_A)
        gradient += in.uv;
        let normal = normalize(vec3(-gradient.x, 1.0, -gradient.y));
        pbr_input.N = normal;
        pbr_input.world_normal = normal;
    }
    let rock = smoothstep(ROCK_STEEPNESS.x, ROCK_STEEPNESS.y, 1.0 - pbr_input.N.y);
    pbr_input.material.base_color = vec4(
        mix(pbr_input.material.base_color.rgb, ROCK * tint.rgb, rock),
        pbr_input.material.base_color.a
    );
    out.color = apply_pbr_lighting(pbr_input);
    // In-shader post processing (fog, alpha-premultiply, tonemapping if the camera is not HDR).
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}

    return out;
}
