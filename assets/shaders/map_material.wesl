// Terrain material, toon style: flat painted ground colours and cel lighting.
//
// - Normals are per pixel: the base surface gradient comes from a texture covering the
//   continent, plus the gradient added by rivers and edits (mesh UVs), so the relief keeps its
//   detail at any mesh LOD.
// - Ground materials (grass, dirt, forest floors, alpine grass, marsh, sand, rock, snow, river
//   beds, sea floor) are flat colours with a few painted patches from world-space noise. Their
//   weights come from two textures baked per continent cell (`map::ground`), warped and
//   sharpened with noise so borders are crisp and irregular; rock on steep slopes and snow
//   sliding off them are decided per pixel. Mesh vertex color red is the river/lake wetness.
// - Grass is animated: short blade strokes along the wind close up, and gusts sweeping across
//   meadows as lighter bands (grass bending), at every distance.
// - Lighting: the sun is a hard-edged two-tone (lit / cool shadow, from the slope and the
//   shadow maps) with a small highlight band and a rim light. No specular.
//
// Everything is multiplied by `tint`. The cel lighting with a rim light follows
// https://github.com/janhohenheim/bevy_wind_waker_shader (MIT / Apache 2.0), itself after
// Wind Waker and Breath of the Wild (see CREDITS.md).

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
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var gradient_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var terrain_sampler: sampler;
// Texture coordinates from world xz: `uv = xz * zw + xy`. Zero until the texture is bound.
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> gradient_map: vec4<f32>;
// Ground weights: (sand, rock, snow, marsh) and (dryness, deciduous, conifer, alpine).
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var ground_a: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var ground_b: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var<uniform> ground_map: vec4<f32>;
// Ground weights: (desert, -, -, -).
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var ground_c: texture_2d<f32>;

// Wind direction (shared with toon_sway.wesl) and gust speed (world units per second).
const WIND: vec2<f32> = vec2(0.8, 0.6);
const GUST_SPEED: f32 = 3.0;

// Cel lighting: colour of lit and shadowed ground (relative to the sun's brightness), extra
// light on faces turned to the sun, rim light.
const LIT: vec3<f32> = vec3(1.0, 0.97, 0.9);
const SHADOW: vec3<f32> = vec3(0.33, 0.37, 0.54);
const HIGHLIGHT: f32 = 0.12;
const RIM: vec3<f32> = vec3(0.25, 0.22, 0.15);

// ---------------------------------------------------------------------------------------------
// Noise

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
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = hash(cell);
    let b = hash(cell + vec2(1, 0));
    let c = hash(cell + vec2(0, 1));
    let d = hash(cell + vec2(1, 1));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 2.0 - 1.0;
}

// Fractal noise in about -1..1. `footprint` is the pixel size in units of `p`: octaves finer
// than about half a pixel fade out.
fn fbm(p: vec2<f32>, octaves: i32, footprint: f32) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var frequency = 1.0;
    for (var octave = 0; octave < octaves; octave++) {
        let fade = 1.0 - smoothstep(0.3, 0.6, frequency * footprint);
        let offset = vec2(f32(octave) * 17.31, f32(octave) * 9.17);
        sum += value_noise(p * frequency + offset) * amplitude * fade;
        amplitude *= 0.5;
        frequency *= 2.03;
    }
    return sum * 1.2;
}

// ---------------------------------------------------------------------------------------------
// Ground

fn srgb(r: f32, g: f32, b: f32) -> vec3<f32> {
    return pow(vec3(r, g, b), vec3(2.2));
}

// A crisp step, antialiased over about a pixel (`width` = the value's change per pixel).
fn crisp(edge: f32, value: f32, width: f32) -> f32 {
    let w = max(width, 1e-4);
    return smoothstep(edge - w, edge + w, value);
}

// A cell weight (bilinear, smooth over a cell) made into an irregular, crisp border.
fn border(weight: f32, noise: f32) -> f32 {
    let value = weight + 0.3 * noise;
    return crisp(0.5, value, fwidth(value));
}

// Patches of a second colour: where the noise passes `threshold`, with crisp edges.
fn patches(noise: f32, threshold: f32) -> f32 {
    return crisp(threshold, noise, fwidth(noise));
}

// Rock: two tones in bands (strata on cliffs) and patches on flat rock.
fn rock_color(p: vec3<f32>, n: vec3<f32>, footprint: f32) -> vec3<f32> {
    let wobble = fbm(p.xz * 0.35 + vec2(p.y * 0.2, 0.0), 3, footprint * 0.35);
    let strata = sin(p.y * 2.2 + wobble * 2.5);
    let flat_patches = fbm(p.xz * 0.6, 3, footprint * 0.6);
    let pattern = mix(strata, flat_patches, smoothstep(0.6, 0.85, n.y));
    return mix(srgb(0.52, 0.49, 0.47), srgb(0.62, 0.59, 0.55), patches(pattern, 0.1));
}

// Grass animation: blade strokes (close up) and wind gusts. Returns the brightness factor.
fn wind(xz: vec2<f32>, footprint: f32, time: f32) -> f32 {
    let along = dot(xz, WIND);
    let across = dot(xz, vec2(-WIND.y, WIND.x));
    // Short strokes stretched along the wind, swaying a little.
    let sway = sin(time * 1.8 + along * 0.8 + across * 0.35) * 0.25;
    let stroke = value_noise(vec2(along * 3.0 + sway, across * 16.0));
    let near = 1.0 - smoothstep(0.015, 0.05, footprint);
    let strokes = (patches(stroke, 0.55) * 0.1 - patches(-stroke, 0.55) * 0.08) * near;
    // Gusts: wide bands moving downwind; bent grass shows its lighter side.
    let gust = fbm(vec2(along * 0.1 - time * GUST_SPEED * 0.1, across * 0.04), 2, footprint * 0.1);
    let gusting = smoothstep(0.2, 0.45, gust);
    return 1.0 + strokes + 0.1 * gusting;
}

fn ground(
    p: vec3<f32>,
    n: vec3<f32>,
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    wet: f32,
    footprint: f32,
    time: f32,
) -> vec3<f32> {
    let xz = p.xz;
    let steep = 1.0 - n.y;
    let medium = fbm(xz * 0.5, 3, footprint * 0.5);
    let large = fbm(xz * 0.07 + vec2(7.1, 3.3), 3, footprint * 0.07);
    let blotches = fbm(xz * 0.18 + vec2(2.0, 8.0), 3, footprint * 0.18);
    let wind_shade = wind(xz, footprint, time);

    // Grass, lush to dry (in a few steps), with lighter patches and a few dirt patches, swept
    // by the wind.
    let dry = saturate(b.r + 0.2 * large);
    let dry_step = floor(dry * 3.0 + 0.5) / 3.0;
    var grass = mix(srgb(0.40, 0.66, 0.24), srgb(0.80, 0.72, 0.38), dry_step);
    grass = mix(grass, grass * vec3(1.1, 1.1, 0.9), patches(blotches, 0.25));
    var color = grass * wind_shade;
    let dirt = patches(fbm(xz * 0.5 + vec2(3.3, 1.1), 3, footprint * 0.5), 0.72 - 0.1 * dry);
    color = mix(color, srgb(0.66, 0.52, 0.34), dirt);

    // Alpine grass (short, also wind-swept).
    let alpine = mix(srgb(0.60, 0.68, 0.42), srgb(0.66, 0.66, 0.50), patches(blotches, 0.1));
    color = mix(color, alpine * wind_shade, border(b.a, medium));

    // Forest floors.
    let deciduous = mix(srgb(0.27, 0.48, 0.18), srgb(0.33, 0.44, 0.17), patches(blotches, 0.0));
    color = mix(color, deciduous, border(b.g, medium));
    let conifer = mix(srgb(0.21, 0.38, 0.22), srgb(0.30, 0.34, 0.20), patches(blotches, 0.1));
    color = mix(color, conifer, border(b.b, medium));

    // Marsh: reed beds (dark strokes) around still pools with a muddy rim.
    let pool_noise = fbm(xz * 0.45 + vec2(5.0, 9.0), 3, footprint * 0.45);
    let pools = patches(pool_noise, 0.15);
    let mud = patches(pool_noise, 0.05) - pools;
    let reed_stroke = value_noise(vec2(dot(xz, WIND) * 5.0, dot(xz, vec2(-WIND.y, WIND.x)) * 22.0));
    let reeds = mix(srgb(0.42, 0.52, 0.26), srgb(0.30, 0.40, 0.20), patches(reed_stroke, 0.3))
        * wind_shade;
    let pool_water = mix(srgb(0.16, 0.30, 0.34), srgb(0.30, 0.46, 0.48),
        patches(fbm(xz * 1.5 + vec2(time * 0.05, 0.0), 2, footprint * 1.5), 0.45) * 0.6);
    var marsh = mix(reeds, srgb(0.36, 0.30, 0.20), mud);
    marsh = mix(marsh, pool_water, pools);
    color = mix(color, marsh, border(a.a, medium));

    // Sand.
    let sand = mix(srgb(0.94, 0.85, 0.62), srgb(0.89, 0.79, 0.56), patches(blotches, 0.2));
    color = mix(color, sand, border(a.r, medium));

    // Desert: warm sand in dune ripples, cracked clay flats.
    let along = dot(xz, WIND);
    let ripples = sin(along * 1.4 + fbm(xz * 0.15, 3, footprint * 0.15) * 4.0);
    var desert = mix(srgb(0.90, 0.72, 0.47), srgb(0.80, 0.60, 0.38), patches(ripples, 0.55));
    let flats = patches(fbm(xz * 0.1 + vec2(13.0, 4.0), 3, footprint * 0.1), 0.3);
    let cracks = patches(1.0 - abs(value_noise(xz * 2.5)), 0.93)
        * (1.0 - smoothstep(0.03, 0.08, footprint));
    let clay = mix(srgb(0.82, 0.63, 0.47), srgb(0.62, 0.45, 0.33), cracks);
    desert = mix(desert, clay, flats);
    let desertness = border(c.r, medium);
    color = mix(color, desert, desertness);

    // Rock: rocky ground, and every steep slope (redder in deserts).
    let steep_value = steep + 0.06 * medium;
    let rock = max(border(a.g, medium), crisp(0.34, steep_value, fwidth(steep_value)));
    let rock_tone = mix(vec3(1.0), vec3(1.12, 0.9, 0.78), desertness);
    color = mix(color, rock_color(p, n, footprint) * rock_tone, rock);

    // Snow: settles on flat ground (steep rock stays bare), with a ragged edge and wind-blown
    // drifts.
    let snow_noise = fbm(xz * 0.25 + vec2(3.0, 17.0), 3, footprint * 0.25);
    let snow_value = a.b * 1.25 + 0.3 * snow_noise + 0.6 * (n.y - 0.85)
        - smoothstep(0.35, 0.6, steep);
    let drifts = patches(fbm(vec2(along * 0.6, dot(xz, vec2(-WIND.y, WIND.x)) * 0.2), 3,
        footprint * 0.6), 0.25);
    let snow_color = mix(srgb(0.97, 0.98, 1.0), srgb(0.84, 0.89, 0.97), drifts);
    color = mix(color, snow_color, crisp(0.5, snow_value, fwidth(snow_value)));

    // River and lake beds.
    color = mix(color, srgb(0.52, 0.46, 0.36), crisp(0.5, wet, fwidth(wet)));

    // Sea floor: sand near the shore, teal further out, darker with depth.
    if p.y < 0.0 {
        let depth = -p.y;
        let floor_color = mix(sand, srgb(0.30, 0.46, 0.48), smoothstep(0.3, 3.0, depth));
        color = mix(color, floor_color, smoothstep(0.0, 0.2, depth));
        color *= max(1.0 - depth / 20.0, 0.4);
    }
    return color;
}

// ---------------------------------------------------------------------------------------------

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

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
    let p = in.world_position.xyz;
    // Size of a pixel on the ground, in world units.
    let footprint = max(length(dpdx(p.xz)), length(dpdy(p.xz)));

    var n = pbr_input.N;
    var a = vec4(0.0);
    var b = vec4(0.3, 0.0, 0.0, 0.0);
    var c = vec4(0.0);
    if gradient_map.z > 0.0 {
        var gradient = textureSample(
            gradient_texture,
            terrain_sampler,
            p.xz * gradient_map.zw + gradient_map.xy
        ).xy;
@if(VERTEX_UVS_A)
        gradient += in.uv;
        n = normalize(vec3(-gradient.x, 1.0, -gradient.y));
    }
    if ground_map.z > 0.0 {
        // Warped, so material borders don't follow the cell grid.
        let warp = vec2(
            fbm(p.xz * 0.4, 2, footprint * 0.4),
            fbm(p.xz * 0.4 + vec2(19.3, 7.7), 2, footprint * 0.4)
        ) * 0.7;
        let uv = (p.xz + warp) * ground_map.zw + ground_map.xy;
        a = textureSample(ground_a, terrain_sampler, uv);
        b = textureSample(ground_b, terrain_sampler, uv);
        c = textureSample(ground_c, terrain_sampler, uv);
    }
    var wet = 0.0;
@if(VERTEX_COLORS)
    wet = in.color.r;

    let albedo = ground(p, n, a, b, c, wet, footprint, globals.time) * tint.rgb;

    // Cel lighting from the sun (the first directional light).
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
        shadow = fetch_directional_shadow(0u, in.world_position, n, view_z, in.position.xy);
    }
    let lit = crisp(0.03, facing, fwidth(facing)) * crisp(0.5, shadow, fwidth(shadow));
    let highlight = crisp(0.55, facing, fwidth(facing)) * lit * HIGHLIGHT;
    let eye = normalize(view.world_position.xyz - p);
    let rim_value = 1.0 - saturate(dot(eye, n));
    let rim = crisp(0.75, rim_value, fwidth(rim_value)) * lit;
    let light = mix(SHADOW, LIT, lit) + highlight + RIM * rim;
    let color = albedo * light * sun_level;

    pbr_input.material.base_color = vec4(albedo, 1.0);
    pbr_input.N = n;
    pbr_input.world_normal = n;
    // In-shader post processing (fog, alpha-premultiply, tonemapping if the camera is not HDR).
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(color, 1.0));
}

    return out;
}
