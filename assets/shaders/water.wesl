// Water (sea, lakes, rivers), toon style, matching the terrain shader.
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
// Sea wave field: (travel time to the shore, depth, exposure to the wind, distance to the
// shore), see `map::sea`.
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var sea_field: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var sea_sampler: sampler;
// Texture coordinates from world xz: `uv = xz * zw + xy`. Zero until the texture is bound.
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> sea_map: vec4<f32>;
// Prevailing wind direction (x, z).
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
    if sea_map.z <= 0.0 {
        return out;
    }
    // Beyond the continent, the texture's edge (open sea) is repeated: no early exit, which
    // would break the derivatives along the edge.
    let uv = xz * sea_map.zw + sea_map.xy;
    let field = textureSampleLevel(sea_field, sea_sampler, uv, 0.0);
    let travel = field.x;
    let depth = field.y;
    let exposure = field.z;
    let wind_dir = wind.xy;
    out.depth = depth;
    out.exposure = exposure;
    out.travel = travel;

    // Crests: isolines of the travel time, moving shoreward. They arrive a little obliquely
    // with the wind, and slowly out of step along the coast.
    let oblique = dot(xz, wind_dir) * 0.08 + value_noise(xz * 0.03) * 1.5;
    // Crests are where the phase is a whole number: their travel time decreases over time.
    let phase = travel / WAVE_LENGTH + time / WAVE_PERIOD + oblique;
    let u = fract(phase);
    let reach = 1.0 - smoothstep(SWELL_REACH.x, SWELL_REACH.y, travel);
    // A sharp front and a trailing back, wider in the shallows (steeper, slower waves).
    let shallow = 1.0 - smoothstep(0.2, 3.0, depth);
    let back = mix(0.18, 0.4, shallow);
    let front = saturate((1.0 - u) / max(fwidth(phase) * 1.5, 1e-3));
    let band = front * (1.0 - smoothstep(0.0, back, u));
    // Crests come in segments (more continuous on exposed shores and in the shallows).
    let segments = fbm2(xz * 0.22 + wind_dir * time * 0.15) * 0.5 + 0.5;
    let continuity = 0.15 + 0.35 * exposure + 0.25 * shallow;
    let crest = band * smoothstep(1.0 - continuity, 1.2 - continuity, segments);
    let swell = reach * (0.35 + 0.65 * exposure);
    out.crest = crisp(0.5, crest * swell);

    // Breakers: crests turn to foam in the shallows, trailing lacy foam behind.
    let breaking = (1.0 - smoothstep(BREAK_DEPTH * 0.4, BREAK_DEPTH, depth / max(exposure, 0.2)))
        * (1.0 - smoothstep(SURF_ZONE * 0.5, SURF_ZONE, travel));
    // Lace: foam with holes, more of them further behind the breaking front.
    let lace = value_noise(xz * 4.0 + vec2(time * 0.2, 0.0)) * 0.5 + 0.5
        + value_noise(xz * 9.0 - vec2(0.0, time * 0.3)) * 0.25;
    let behind = smoothstep(0.0, 0.5, u);
    let trail = front * (1.0 - behind) * step(behind * 0.9 + 0.15, lace);
    out.foam = crisp(0.45, max(crest * step(behind * 1.6, lace), trail) * breaking * exposure
        * reach);

    // Swash: the foam line runs up the beach as each wave arrives.
    let arrive = 0.5 + 0.5 * sin(6.2832 * (time / WAVE_PERIOD + oblique));
    out.swash = 0.03 + (0.04 + 0.12 * arrive * arrive) * exposure;
    out.chop = mix(0.6, 0.25, reach * shallow);
    return out;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xyz;
    let time = globals.time;
    let footprint = length(fwidth(p.xz));
    let to_eye = normalize(view.world_position.xyz - p);

    // Thickness of water between this surface and the ground behind it, along the view ray,
    // and the depth under the surface it amounts to.
    let surface_z = depth_ndc_to_view_z(in.position.z);
    var thickness = 100.0;
@if(DEPTH_PREPASS)
    thickness = max(surface_z - depth_ndc_to_view_z(prepass_depth(in.position, 0u)), 0.0);
    var depth = thickness * max(to_eye.y, 0.2);
    var sea: Sea;
    sea.depth = -1.0;
    if p.y < 0.05 {
        sea = sea_waves(p.xz, time, footprint);
        if sea.depth >= 0.0 {
            // The baked depth is smoother; the prepass is exact at the shore.
            depth = max(min(depth, 0.3), sea.depth);
        }
    }

    // Depth bands, their edges wobbling.
    let wobble = value_noise(p.xz * 0.6 + vec2(time * 0.05, 0.0));
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
    let side = vec2(-dir.y, dir.x);
    let across = dot(p.xz, side);
    // Geometric slope of the surface: falls are steep.
    let face = normalize(cross(dpdx(p), dpdy(p)));
    let fall = smoothstep(0.35, 0.7, 1.0 - abs(face.y));
    // Flow map (two phases, cross-faded): the pattern is sampled in world space, pushed
    // downstream by the velocity and reset every FLOW_PERIOD; stretched along the flow where
    // fast. Velocities are continuous where rivers meet, so the pattern bends from one into
    // the other.
    let offset = value_noise(p.xz * 0.4) * 0.5;
    var streak_sum = 0.0;
    var froth_sum = 0.0;
    var weights = 0.0;
    for (var k = 0; k < 2; k++) {
        let t = fract(time / FLOW_PERIOD + f32(k) * 0.5 + offset);
        let w = 1.0 - abs(2.0 * t - 1.0);
        let q = p.xz - velocity * (t * FLOW_PERIOD) + f32(k) * vec2(17.0, 31.0);
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
    let falling = value_noise(vec2(across * 8.0, p.y * 3.0 + time * 5.0)) * 0.5 + 0.5;
    let fall_streaks = crisp(0.6, falling);
    // The lip and the foot of a fall are foamy.
    let lip = crisp(0.85, fall) * crisp(0.5, value_noise(vec2(across * 6.0, time * 2.0)) * 0.5 + 0.5);
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
        crest = max(crest, ripples(p.xz, wind.xy, time, footprint, chop));
    }

    // Glints: where two drifting noise layers are both high.
    let a = value_noise(p.xz * 12.0 + vec2(time * 1.2, time * 0.4));
    let b = value_noise(p.xz * 15.0 - vec2(time * 0.7, time * 1.1) + vec2(13.0, 7.0));
    // Faded out where a pixel covers more than a few glints (they would flicker).
    let glint = crisp(0.85, a * b + 0.4 * max(a, b)) * (1.0 - smoothstep(0.008, 0.025, footprint));

    // Shore foam: a band of shallow water along the shore, its edge wobbling.
    let edge_wobble = value_noise(p.xz * 2.0 + vec2(time * 0.5, 0.0)) * 0.3;
    let shore = 1.0 - smoothstep(swash * 0.8, swash, thickness * (1.0 + edge_wobble));

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
