// The cloud layer: a shell round the planet showing the sky map's clouds (`worldgen::sky`:
// density by direction, how convective), toon-lit like the ground:
// - crisp edges where the density crosses 0.5, broken up by finer noise (more for convective
//   clouds: puffs; less for layered ones);
// - lit on the day side, a cool shadow tone where the cloud is thick toward the sun (the
//   cores of big systems), grey bases seen from below;
// - `layer.x`: the shell's radius; `tint`: the clouds' colour (yellowed in hazy skies).

import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, lights, globals},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> layer: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> tint: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var sky_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var sky_sampler: sampler;

const LIT: vec3<f32> = vec3(1.0, 0.99, 0.96);
const SHADOW: vec3<f32> = vec3(0.62, 0.66, 0.78);
const BASE: vec3<f32> = vec3(0.55, 0.58, 0.64);

// The sky map by direction (longitude = atan2(z, x), latitude = asin(y)).
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

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3(127.1, 311.7, 74.7))) * 43758.5453);
}

// Value noise in 0..1.
fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let x00 = mix(hash3(i), hash3(i + vec3(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(hash3(i + vec3(0.0, 1.0, 0.0)), hash3(i + vec3(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(hash3(i + vec3(0.0, 0.0, 1.0)), hash3(i + vec3(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(hash3(i + vec3(0.0, 1.0, 1.0)), hash3(i + vec3(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    let d = normalize(p);
    let sky = sky_at(d);
    // Finer detail (world units: puffs a few units across): the edges seethe as the clouds
    // drift through it.
    let q = p * 0.35;
    let detail = noise3(q) * 0.6 + noise3(q * 2.7 + vec3(5.2, 1.3, 7.7)) * 0.4;
    let density = sky.r + (detail - 0.5) * (0.18 + 0.3 * sky.g);
    let w = max(fwidth(density), 1e-3);
    let alpha = smoothstep(0.5 - w, 0.5 + w, density);
    if alpha <= 0.001 {
        discard;
    }
    let sun = lights.directional_lights[0];
    let l = sun.direction_to_light;
    let facing = dot(d, l);
    let day = smoothstep(-0.12, 0.08, facing);
    // Thick toward the sun: the cloud's own shadow.
    let toward = normalize(d + (l - d * facing) * 0.012);
    let thick = sky_at(toward).r + (detail - 0.5) * 0.15;
    let shade = smoothstep(0.78 - w, 0.78 + w, thick);
    var color = mix(LIT, SHADOW, shade);
    // Seen from below: the bases.
    if length(view.world_position.xyz) < layer.x {
        color = BASE * mix(1.0, 0.8, shade);
    }
    let sun_level = dot(sun.color.rgb * view.exposure, vec3(0.2126, 0.7152, 0.0722))
        / 3.14159265;
    let hue = sun.color.rgb / max(dot(sun.color.rgb, vec3(0.2126, 0.7152, 0.0722)), 1e-4);
    // (A little light on the night side: moonlight and the sky's.)
    let light = mix(0.04, 1.0, day) * sun_level;
    return vec4(color * tint.rgb * hue * light, alpha * 0.94);
}
