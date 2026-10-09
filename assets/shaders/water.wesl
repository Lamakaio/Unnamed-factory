// Water (sea, lakes, rivers), toon style, matching the terrain shader:
//
// - Colour from the thickness of water behind each pixel (the depth prepass minus the water
//   surface depth): pale turquoise in the shallows to deep blue, more opaque when deeper.
// - A crisp, wobbling foam line where the water meets the ground.
// - Animated glints: two drifting noise layers, lit only where the sun reaches.
// - Cel lighting from the sun and its shadows, like the terrain.
//
// Depth-based shore foam and colour are a common stylized water technique (see CREDITS.md).

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
    mesh_view_bindings::{view, lights, globals},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    view_transformations::depth_ndc_to_view_z,
};

@if(DEPTH_PREPASS)
import bevy_pbr::prepass::utils::prepass_depth;

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> tint: vec4<f32>;

const SHALLOW: vec3<f32> = vec3(0.16, 0.55, 0.62);
const DEEP: vec3<f32> = vec3(0.03, 0.14, 0.30);
const FOAM: vec3<f32> = vec3(0.92, 0.96, 0.98);
const GLINT: vec3<f32> = vec3(0.85, 0.95, 1.0);
const SHADOW: vec3<f32> = vec3(0.55, 0.6, 0.75);
// Water thickness (world units) over which the colour goes from shallow to deep.
const DEPTH_RANGE: f32 = 2.5;

fn hash(c: vec2<i32>) -> f32 {
    var h = (bitcast<u32>(c.x) * 0x27d4eb2du) ^ (bitcast<u32>(c.y) * 0x165667b1u);
    h = (h ^ (h >> 15u)) * 0x85ebca6bu;
    h = (h ^ (h >> 13u)) * 0xc2b2ae35u;
    h = h ^ (h >> 16u);
    return f32(h) / 4294967295.0;
}

fn value_noise(p: vec2<f32>) -> f32 {
    let cell = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(cell);
    let b = hash(cell + vec2(1, 0));
    let c = hash(cell + vec2(0, 1));
    let d = hash(cell + vec2(1, 1));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 2.0 - 1.0;
}

fn crisp(edge: f32, value: f32) -> f32 {
    let w = max(fwidth(value), 1e-4);
    return smoothstep(edge - w, edge + w, value);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xyz;
    let time = globals.time;

    // Thickness of water between this surface and the ground behind it, along the view ray.
    let surface_z = depth_ndc_to_view_z(in.position.z);
    var thickness = 100.0;
@if(DEPTH_PREPASS)
    thickness = max(surface_z - depth_ndc_to_view_z(prepass_depth(in.position, 0u)), 0.0);

    let depth = smoothstep(0.0, DEPTH_RANGE, thickness);
    var color = mix(SHALLOW, DEEP, sqrt(depth));
    var alpha = mix(0.55, 0.92, depth);

    // Glints: where two drifting noise layers are both high.
    let a = value_noise(p.xz * 1.3 + vec2(time * 0.35, time * 0.12));
    let b = value_noise(p.xz * 1.7 - vec2(time * 0.21, time * 0.3) + vec2(13.0, 7.0));
    // Faded out where a pixel covers more than a few glints (they would flicker).
    let footprint = length(fwidth(p.xz));
    let glint = crisp(0.75, a * b + 0.4 * max(a, b)) * (1.0 - smoothstep(0.05, 0.2, footprint));

    // Shore foam: a band of shallow water, its edge wobbling over time.
    let wobble = value_noise(p.xz * 2.0 + vec2(time * 0.5, 0.0)) * 0.06;
    // Fixed edge width: the thickness jumps where the ground behind changes abruptly.
    let foam = 1.0 - smoothstep(0.03, 0.06, thickness - wobble * 0.5);

    // Cel lighting from the sun.
    let sun = lights.directional_lights[0];
    let sun_level = dot(sun.color.rgb * view.exposure, vec3(0.2126, 0.7152, 0.0722))
        / 3.14159265;
    var shadow = 1.0;
    if (sun.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
        let view_z = dot(vec4(
            view.view_from_world[0].z,
            view.view_from_world[1].z,
            view.view_from_world[2].z,
            view.view_from_world[3].z
        ), in.world_position);
        shadow = fetch_directional_shadow(0u, in.world_position, vec3(0.0, 1.0, 0.0), view_z,
            in.position.xy);
    }
    let lit = crisp(0.5, shadow);
    color = mix(color, GLINT, glint * lit * 0.8);
    color = mix(color, FOAM, foam);
    alpha = max(alpha, foam * 0.95);
    color *= mix(SHADOW, vec3(1.0), lit) * sun_level;

    pbr_input.material.base_color = vec4(color, alpha);
    var out: FragmentOutput;
    // In-shader post processing (fog, alpha-premultiply, tonemapping if the camera is not HDR).
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(color * tint.rgb, alpha));
    return out;
}
