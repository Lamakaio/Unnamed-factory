// Planet water (sea, lakes, rivers): the flat map's toon water (`water.wesl`) on the sphere.
// Patterns use 2D coordinates from the mesh (vertex colour red/green: along / across a river,
// or a cube face's cell coordinates for lakes and the sea), heights and up are radial
// (`planet.x` is the sea level radius). No swell yet (the flat map's wave field is baked over
// its continent): the sea has wind ripples and shore foam.
//
// The flat map's description:
//
// - Colour in crisp bands from the depth of water under each pixel (from the depth prepass):
//   pale turquoise shallows, teal, deep blue; more opaque when deeper.
// - Sea: swell from the baked wave field (`map::sea`): crests follow the travel time of waves
//   to the shore, so they wrap around headlands and bunch up in the shallows; they arrive a
//   little obliquely with the wind, break into foam on windward shallows, and wash up the beach
//   (a pulsing foam line). Offshore and on lakes, short wind ripples drift downwind.
// - Rivers: a flow map (river mesh UVs: velocity, whitewater; after Vlachos 2010): streaks
//   pushed along the flow, stretched where fast, bending from tributaries into their parent;
//   whitewater on rapids, below falls and confluences; falling streaks on steep faces
//   (waterfalls).
// - Glints and cel lighting from the sun and its shadows, like the terrain.
//
// Depth-based shore foam and colour are a common stylized water technique, and the wave
// fronts follow the eikonal travel time of swell (see CREDITS.md).

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
// x: the sea level radius.
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> planet: vec4<f32>;
// Prevailing wind direction (in the pattern coordinates).
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var<uniform> wind: vec4<f32>;

// Depth bands (linear colours) and the depths between them (world units).
const LAGOON: vec3<f32> = vec3(0.16, 0.80, 0.72);
const SHALLOW: vec3<f32> = vec3(0.04, 0.56, 0.66);
const MID: vec3<f32> = vec3(0.04, 0.36, 0.52);
const DEEP: vec3<f32> = vec3(0.025, 0.15, 0.34);
const BANDS: vec3<f32> = vec3(0.25, 1.2, 4.0);
const FOAM: vec3<f32> = vec3(1.1, 1.13, 1.15);
const GLINT: vec3<f32> = vec3(0.85, 0.95, 1.0);
const SHADOW: vec3<f32> = vec3(0.46, 0.51, 0.68);

// Swell: wavelength in deep water (world units) and period (seconds).
const WAVE_LENGTH: f32 = 4.0;
const WAVE_PERIOD: f32 = 5.0;
// Shore waves fade out between these travel times (far offshore, only wind ripples remain).
const SWELL_REACH: vec2<f32> = vec2(18.0, 34.0);
// Swell breaks into foam in water shallower than this (on fully exposed shores).
const BREAK_DEPTH: f32 = 1.5;
// Shore foam: the water thickness is averaged over this world distance (about the terrain
// mesh's cells near water), with this many taps around the pixel.
const SHORE_SMOOTH: f32 = 0.18;
const SHORE_TAPS: i32 = 8;
// River flow map: seconds before the pattern resets (two phases, cross-faded).
const FLOW_PERIOD: f32 = 1.6;
// ...and only this close to the shore (travel time, deep-water world units).
const SURF_ZONE: f32 = 10.0;

fn hash(c: vec2<i32>) -> f32 {
    var h = (bitcast<u32>(c.x) * 0x27d4eb2du) ^ (bitcast<u32>(c.y) * 0x165667b1u);
    h = (h ^ (h >> 15u)) * 0x85ebca6bu;
    h = (h ^ (h >> 13u)) * 0xc2b2ae35u;
    h = h ^ (h >> 16u);
    return f32(h) / 4294967295.0;
}

// Value noise in -1..1, features about one unit apart.
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

fn fbm2(p: vec2<f32>) -> f32 {
    return value_noise(p) * 0.65 + value_noise(p * 2.03 + vec2(17.3, 9.2)) * 0.35;
}

// Thickness of the water at `position` (fragment coordinates), averaged over a disc of `radius`
// pixels: the shore foam then doesn't trace the triangles of the terrain mesh behind.
@if(DEPTH_PREPASS)
fn smoothed_thickness(position: vec4<f32>, surface_z: f32, radius: f32) -> f32 {
    let size = view.viewport.zw - 1.0;
    var total = 0.0;
    for (var k = 0; k <= SHORE_TAPS; k++) {
        var offset = vec2(0.0);
        if k > 0 {
            let angle = f32(k) * 6.2832 / f32(SHORE_TAPS) + 0.4;
            offset = vec2(cos(angle), sin(angle)) * radius;
        }
        let at = vec4(clamp(position.xy + offset, vec2(0.0), size), position.zw);
        total += max(surface_z - depth_ndc_to_view_z(prepass_depth(at, 0u)), 0.0);
    }
    return total / f32(SHORE_TAPS + 1);
}

// A crisp step, antialiased over about a pixel.
fn crisp(edge: f32, value: f32) -> f32 {
    let w = max(fwidth(value), 1e-4);
    return smoothstep(edge - w, edge + w, value);
}

// Short ripples drifting downwind, as light dashes across the wind (`strength` 0..1).
fn ripples(xz: vec2<f32>, wind_dir: vec2<f32>, time: f32, footprint: f32, strength: f32) -> f32 {
    let along = dot(xz, wind_dir);
    let across = dot(xz, vec2(-wind_dir.y, wind_dir.x));
    // Thin lines across the wind, wavy, in short segments.
    let bend = value_noise(vec2(along * 0.7, across * 0.9)) * 2.5;
    let lines = sin(along * 9.0 + bend - time * 2.0) * 0.5 + 0.5;
    let segments = value_noise(vec2(along * 1.5 - time * 0.3, across * 0.8) + vec2(31.0, 7.0));
    let dashes = crisp(0.93, lines) * crisp(0.45 - 0.2 * strength, segments);
    return dashes * (1.0 - smoothstep(0.02, 0.06, footprint)) * strength;
}

struct Sea {
    // Water depth (world units, negative if unknown).
    depth: f32,
    // Exposure to the wind (0..1) and travel time to the shore.
    exposure: f32,
    travel: f32,
    // Light crest lines.
    crest: f32,
    // Breaking foam.
    foam: f32,
    // Shore foam reach (water thickness up to which the beach is foamy).
    swash: f32,
    // Wind ripple strength.
    chop: f32,
}

fn sea_waves(xz: vec2<f32>, time: f32, footprint: f32) -> Sea {
    var out: Sea;
    out.chop = 0.6;
    out.swash = 0.04;
    out.depth = -1.0;
    return out;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xyz;
    let radial = normalize(p);
    let height = length(p) - planet.x;
    var xz = vec2(0.0);
@if(VERTEX_COLORS)
    xz = in.color.xy;
    let time = globals.time;
    let footprint = length(fwidth(xz));
    let to_eye = normalize(view.world_position.xyz - p);

    // Thickness of water between this surface and the ground behind it, along the view ray,
    // and the depth under the surface it amounts to.
    let surface_z = depth_ndc_to_view_z(in.position.z);
    var thickness = 100.0;
    // For the shore foam: smoothed (see `smoothed_thickness`).
    var shore_thickness = 100.0;
@if(DEPTH_PREPASS) {
    thickness = max(surface_z - depth_ndc_to_view_z(prepass_depth(in.position, 0u)), 0.0);
    let pixel = max(footprint, 1e-4);
    shore_thickness = smoothed_thickness(in.position, surface_z, clamp(SHORE_SMOOTH / pixel, 1.0, 12.0));
}
    var depth = thickness * max(dot(to_eye, radial), 0.2);
    var sea: Sea;
    sea.depth = -1.0;

    // Depth bands, their edges wobbling.
    let wobble = value_noise(xz * 0.6 + vec2(time * 0.05, 0.0));
    var color = mix(LAGOON, SHALLOW, crisp(BANDS.x, depth * (1.0 + 0.25 * wobble)));
    color = mix(color, MID, crisp(BANDS.y, depth * (1.0 + 0.2 * wobble)));
    color = mix(color, DEEP, crisp(BANDS.z, depth * (1.0 + 0.15 * wobble)));
    var alpha = mix(mix(0.5, 0.8, smoothstep(0.0, 0.25, depth)), 0.97, smoothstep(0.25, 1.5, depth));

    var crest = 0.0;
    var foam = 0.0;
    var swash = 0.035;
    var chop = 0.35;
    var river = false;
    // Rivers fade out into the water they flow into.
    var fade = 1.0;

@if(VERTEX_UVS_B) {
    // Rivers: flow velocity (world x, z) and (whitewater, -).
    river = true;
    let velocity = in.uv;
    let whitewater = in.uv_b.x;
    fade = in.uv_b.y;
    let speed = max(length(velocity), 0.05);
    let dir = velocity / speed;
    // Geometric slope of the surface: falls are steep.
    let face = normalize(cross(dpdx(p), dpdy(p)));
    let fall = smoothstep(0.35, 0.7, 1.0 - abs(dot(face, radial)));
    // Flow map (two phases, cross-faded): the pattern is sampled in world space, pushed
    // downstream by the velocity and reset every FLOW_PERIOD; stretched along the flow where
    // fast. Velocities are continuous where rivers meet, so the pattern bends from one into
    // the other.
    let offset = value_noise(xz * 0.4) * 0.5;
    var streak_sum = 0.0;
    var froth_sum = 0.0;
    var weights = 0.0;
    for (var k = 0; k < 2; k++) {
        let t = fract(time / FLOW_PERIOD + f32(k) * 0.5 + offset);
        let w = 1.0 - abs(2.0 * t - 1.0);
        let q = xz - velocity * (t * FLOW_PERIOD) + f32(k) * vec2(17.0, 31.0);
        // Streaks: isotropic noise smeared along the flow (no rotated coordinates, which
        // would swing wildly with small direction changes far from the origin).
        let smear = dir * (0.2 + 0.35 * speed);
        var streak = -1.0;
        for (var j = 0; j < 4; j++) {
            let s = q - smear * f32(j);
            streak = max(streak, max(value_noise(s * 5.0), value_noise(s * 8.0 + 5.0) * 0.9));
        }
        streak_sum += w * streak;
        froth_sum += w * fbm2(q * 3.0);
        weights += w * w;
    }
    // Normalized so the blend keeps the contrast of a single layer (no pulsing).
    let norm = inverseSqrt(max(weights, 1e-3));
    let streaks = crisp(0.86, streak_sum * norm) * (1.0 - smoothstep(0.05, 0.15, footprint));
    crest = streaks * mix(0.5, 1.0, smoothstep(0.4, 1.5, speed));
    // Whitewater: foam patches, more where rougher.
    let froth = froth_sum * norm * 0.5 + 0.5;
    foam = crisp(1.0 - 0.55 * whitewater, froth);
    // Waterfalls: white falling streaks.
    // Streaks in world x and z both (no coordinates rotated with the flow: far from the origin
    // they swing with small direction changes, striping the fall), so they are vertical
    // whichever way the fall faces.
    let falling = max(value_noise(vec2(xz.x * 8.0, height * 3.0 + time * 5.0)),
        value_noise(vec2(xz.y * 8.0 + 31.0, height * 3.0 + time * 5.0))) * 0.5 + 0.5;
    let fall_streaks = crisp(0.7, falling);
    // The lip and the foot of a fall are foamy.
    let lip = crisp(0.85, fall)
        * crisp(0.5, value_noise(vec2(xz.x + xz.y, time * 0.4) * 6.0) * 0.5 + 0.5);
    foam = mix(foam, max(fall_streaks, lip), fall);
    crest = mix(crest, 1.0, fall);
    alpha = max(alpha, fall * 0.92);
    chop = 0.0;
    swash = 0.03 + 0.03 * whitewater;
}

    if !river {
        if sea.depth >= 0.0 {
            crest = sea.crest;
            foam = sea.foam;
            swash = sea.swash;
            chop = sea.chop;
        }
        crest = max(crest, ripples(xz, wind.xy, time, footprint, chop));
    }

    // Glints: where two drifting noise layers are both high.
    let a = value_noise(xz * 12.0 + vec2(time * 1.2, time * 0.4));
    let b = value_noise(xz * 15.0 - vec2(time * 0.7, time * 1.1) + vec2(13.0, 7.0));
    // Faded out where a pixel covers more than a few glints (they would flicker).
    let glint = crisp(0.85, a * b + 0.4 * max(a, b)) * (1.0 - smoothstep(0.008, 0.025, footprint));

    // Shore foam: a band of shallow water along the shore, its edge wobbling.
    // Muddled: the foam's reach varies along the shore (two drifting noise layers), and its
    // edge is broken up a little.
    let reach = value_noise(xz * 2.0 + vec2(time * 0.5, 0.0)) * 0.35
        + value_noise(xz * 6.5 - vec2(0.0, time * 0.7) + vec2(23.0, 5.0)) * 0.2;
    let shore = 1.0 - smoothstep(swash * 0.75, swash, shore_thickness * (1.0 + reach));

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
        shadow = fetch_directional_shadow(0u, in.world_position, radial, view_z,
            in.position.xy);
    }
    let lit = crisp(0.5, shadow);
    color = mix(color, mix(color, FOAM, 0.35) * 1.1, crest);
    color = mix(color, GLINT, glint * lit * 0.8);
    foam = max(foam, shore);
    color = mix(color, FOAM, foam);
    alpha = max(alpha, max(foam * 0.95, crest * 0.75));
    color *= mix(SHADOW, vec3(1.0), lit) * sun_level;

    alpha *= fade;
    pbr_input.material.base_color = vec4(color, alpha);
    var out: FragmentOutput;
    // In-shader post processing (fog, alpha-premultiply, tonemapping if the camera is not HDR).
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(color * tint.rgb, alpha));
    return out;
}
