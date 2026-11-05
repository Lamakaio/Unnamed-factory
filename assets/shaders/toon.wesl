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
    mesh_view_bindings::{view, lights, globals},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
};

@if(VISIBILITY_RANGE_DITHER)
import bevy_pbr::render::pbr_functions::visibility_range_dither;

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> tint: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> snow: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var<uniform> foliage: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var sky_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var sky_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var<uniform> sky_layer: vec4<f32>;

// The sky map (`worldgen::sky`: cloud density, how convective, humid haze, dust) by direction
// (longitude = atan2(z, x), latitude = asin(y)); `sky_layer.x` is the cloud layer's radius
// (0: no clouds).
// How often the clouds are redrawn (`planet::sky::REDRAW`).
const SKY_REDRAW: f32 = 0.2;

fn sky_at(d: vec3<f32>) -> vec4<f32> {
    // (Two snapshots side by side, `SKY_REDRAW` seconds apart, blended by the clock.)
    let u = (atan2(d.z, d.x) / 6.28318531 + 0.5) * 0.5;
    let v = acos(clamp(d.y, -1.0, 1.0)) / 3.14159265;
    let now = textureSampleLevel(sky_map, sky_sampler, vec2(u, v), 0.0);
    let next = textureSampleLevel(sky_map, sky_sampler, vec2(u + 0.5, v), 0.0);
    return mix(now, next, fract(globals.time / SKY_REDRAW));
}

// Sunlight the clouds leave at `p` (world, from the planet's centre) toward the sun `l`: 1 in
// the clear, `CLOUD_SHADOW` under cloud (where the ray to the sun crosses the cloud layer).
const CLOUD_SHADOW: f32 = 0.3;
fn cloud_light(p: vec3<f32>, l: vec3<f32>) -> f32 {
    let radius = sky_layer.x;
    let b = dot(p, l);
    let h = b * b - (dot(p, p) - radius * radius);
    if radius <= 0.0 || h < 0.0 {
        return 1.0;
    }
    let t = -b + sqrt(h);
    if t <= 0.0 {
        return 1.0;
    }
    let density = sky_at(normalize(p + l * t)).r;
    return mix(1.0, CLOUD_SHADOW, smoothstep(0.47, 0.53, density));
}

// Plants under another star (`foliage.x`: hue turn of the greens in radians, `foliage.y`:
// brightness; see `worldgen::world::Star::foliage`): green colours turned in YIQ hue, the
// rest (bark, rock, flowers, straw) left alone.
fn foliage_tint(c: vec3<f32>) -> vec3<f32> {
    let green = saturate((c.g - max(c.r, c.b)) * 6.0);
    if green <= 0.0 || (foliage.x == 0.0 && foliage.y == 1.0) {
        return c;
    }
    let y = dot(c, vec3(0.299, 0.587, 0.114));
    let i = dot(c, vec3(0.596, -0.274, -0.322));
    let q = dot(c, vec3(0.211, -0.523, 0.312));
    let ca = cos(foliage.x);
    let sa = sin(foliage.x);
    let i2 = i * ca - q * sa;
    let q2 = i * sa + q * ca;
    let turned = vec3(y + 0.956 * i2 + 0.621 * q2, y - 0.272 * i2 - 0.647 * q2,
        y - 1.106 * i2 + 1.703 * q2) * foliage.y;
    return mix(c, max(turned, vec3(0.0)), green);
}

// The sun's hue (its colour at unit luminance): the lit side takes it.
fn sun_hue(color: vec3<f32>) -> vec3<f32> {
    return color / max(dot(color, vec3(0.2126, 0.7152, 0.0722)), 1e-4);
}

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
    var albedo = foliage_tint(pbr_input.material.base_color.rgb);
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
    let lit = crisp(0.03, facing) * crisp(0.5, shadow)
        * cloud_light(in.world_position.xyz, sun.direction_to_light);
    let highlight = crisp(0.55, facing) * lit * HIGHLIGHT;
    let eye = normalize(view.world_position.xyz - in.world_position.xyz);
    let rim = crisp(0.75, 1.0 - saturate(dot(eye, n))) * lit;
    let light = mix(SHADOW, LIT * sun_hue(sun.color.rgb), lit) + highlight + RIM * rim;
    // In-shader post processing (fog, alpha-premultiply, tonemapping if the camera is not HDR).
    out.color = main_pass_post_lighting_processing(
        pbr_input,
        vec4(albedo * light * sun_level, pbr_input.material.base_color.a)
    );
}

    return out;
}
