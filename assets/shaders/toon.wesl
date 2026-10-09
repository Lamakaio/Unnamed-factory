// Toon material for objects (vegetation, rocks): the standard material's colour (texture ×
// vertex colour × base colour, alpha-masked leaves included) with the same cel lighting as the
// terrain (map_material.wesl): a hard lit / cool-shadow step from the sun and its shadow maps,
// a highlight band and a rim light. Level-of-detail cross-fades use the visibility range
// dither.
//
// With `LOD_FADE` (merged stand-ins of many objects, see `map::vegetation`): each vertex
// carries its object's origin (UV0 = x, z; UV1.x = y), and the object fades out where its
// detailed model fades in, with the same dither as Bevy's visibility ranges (`lod` = the
// detailed model's end margin), so the patterns complement each other.
//
// Snow settles on upward faces above the snow line (`snow.x` to `snow.y`, world heights),
// matching the terrain's snow.

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
    pbr_functions::main_pass_post_lighting_processing,
    mesh_view_bindings::{view, lights},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
};

@if(VISIBILITY_RANGE_DITHER)
import bevy_pbr::render::pbr_functions::visibility_range_dither;

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> tint: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> snow: vec4<f32>;

const SNOW: vec3<f32> = vec3(0.92, 0.94, 0.98);

// Cheap noise for the snow edge.
fn hash2(q: vec2<f32>) -> f32 {
    return fract(sin(dot(q, vec2(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let bottom = mix(hash2(cell), hash2(cell + vec2(1.0, 0.0)), u.x);
    let top = mix(hash2(cell + vec2(0.0, 1.0)), hash2(cell + vec2(1.0, 1.0)), u.x);
    return mix(bottom, top, u.y) * 2.0 - 1.0;
}
@if(LOD_FADE)
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> lod: vec4<f32>;

// Bevy's visibility range dither pattern (bevy_pbr pbr_functions.wesl).
const DITHER_THRESHOLD_MAP: array<u32, 4> = array(
    0x0a020800,
    0x060e040c,
    0x09010b03,
    0x050d070f
);

// Like Bevy's `visibility_range_dither`, for an object fading in between `lod.x` and `lod.y`
// (camera distance to its origin).
@if(LOD_FADE)
fn lod_fade_in(frag_coord: vec4<f32>, origin: vec3<f32>) {
    let distance = length(view.lod_view_world_position.xyz - origin);
    let dither = -16 + clamp(i32(round((distance - lod.x) / (lod.y - lod.x) * 16.0)), 0, 16);
    if dither == 0 {
        return;
    }
    if dither <= -16 {
        discard;
    }
    let coords = vec2<u32>(floor(frag_coord.xy)) % 4u;
    let threshold = i32((DITHER_THRESHOLD_MAP[coords.y] >> (coords.x * 8u)) & 0xffu);
    if 1 + dither + threshold <= 0 {
        discard;
    }
}

// Same constants as the terrain.
const LIT: vec3<f32> = vec3(1.0, 0.97, 0.9);
const SHADOW: vec3<f32> = vec3(0.33, 0.37, 0.54);
const HIGHLIGHT: f32 = 0.12;
const RIM: vec3<f32> = vec3(0.25, 0.22, 0.15);

fn crisp(edge: f32, value: f32) -> f32 {
    let w = max(fwidth(value), 1e-4);
    return smoothstep(edge - w, edge + w, value);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);
@if(LOD_FADE)
    lod_fade_in(in.position, vec3(in.uv.x, in.uv_b.x, in.uv.y));

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color * tint
    );

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    var albedo = pbr_input.material.base_color.rgb;
    let n = pbr_input.N;
    if snow.y > snow.x {
        let world = in.world_position.xyz;
        let wobble = noise(world.xz * 0.5) * 0.8;
        let cover = smoothstep(snow.x, snow.y, world.y + wobble) * smoothstep(0.3, 0.7, n.y);
        albedo = mix(albedo, SNOW, crisp(0.5, cover));
    }
    let sun = lights.directional_lights[0];
    let sun_level = dot(sun.color.rgb * view.exposure, vec3(0.2126, 0.7152, 0.0722))
        / 3.14159265;
    let facing = dot(n, sun.direction_to_light);
    var shadow = 1.0;
    if (sun.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
        let view_z = dot(vec4(
            view.view_from_world[0].z,
            view.view_from_world[1].z,
            view.view_from_world[2].z,
            view.view_from_world[3].z
        ), in.world_position);
        shadow = fetch_directional_shadow(0u, in.world_position, pbr_input.world_normal, view_z,
            in.position.xy);
    }
    let lit = crisp(0.03, facing) * crisp(0.5, shadow);
    let highlight = crisp(0.55, facing) * lit * HIGHLIGHT;
    let eye = normalize(view.world_position.xyz - in.world_position.xyz);
    let rim = crisp(0.75, 1.0 - saturate(dot(eye, n))) * lit;
    let light = mix(SHADOW, LIT, lit) + highlight + RIM * rim;
    // In-shader post processing (fog, alpha-premultiply, tonemapping if the camera is not HDR).
    out.color = main_pass_post_lighting_processing(
        pbr_input,
        vec4(albedo * light * sun_level, pbr_input.material.base_color.a)
    );
}

    return out;
}
