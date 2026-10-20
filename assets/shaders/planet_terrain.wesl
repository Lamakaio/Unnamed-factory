// Planet terrain material: the flat map's toon ground (`map_material.wesl`) on the sphere.
//
// Same ground painting and cel lighting; what differs is where its inputs come from:
// - horizontal coordinates: the cube face's cell coordinates (UV0), continuous on a face (the
//   painted patterns may break at face edges); heights are radial (`planet.x` is the sea level
//   radius) and slopes are measured against the radial up;
// - ground materials come from the climate per vertex: colour = (dryness, forest, marsh
//   (positive) or salt flat (negative), wet river or lake bed), UV1 = (cold: conifers and
//   alpine grass, plus snow above 1; coast: beaches only along the ocean); deserts are the
//   driest ground;
// - the view direction for the grass parallax is taken into face coordinates from the screen
//   space derivatives.
//
// The cel lighting with a rim light follows https://github.com/janhohenheim/bevy_wind_waker_shader
// (MIT / Apache 2.0), itself after Wind Waker and Breath of the Wild (see CREDITS.md).

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
// x: the sea level radius.
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> planet: vec4<f32>;
// The face's rock field, by cell (`worldgen::geology`): rock id (red), strata offset (green,
// erosion units), fresh lava (blue, round active volcanoes), coral reef (alpha). `planet.y` is the cells along a face edge, `planet.z` world units per
// erosion unit (strata height = height / planet.z + offset), `planet.w` 1 with the field.
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var rock_field: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var rock_sampler: sampler;

// Prevailing wind direction (in face coordinates).
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var<uniform> wind: vec4<f32>;
// Gust speed (world units per second).
const GUST_SPEED: f32 = 3.0;
// Grass blades: per world unit, height of the grass layer (world units), ray march steps, and
// the blade spacing on screen (pixels) over which they fade out far away: well below a pixel
// (temporal antialiasing turns them into a fine grassy grain).
const BLADE_SCALE: f32 = 50.0;
const BLADE_HEIGHT: f32 = 0.11;
const GRASS_STEPS: i32 = 24;
const BLADE_FADE: vec2<f32> = vec2(0.2, 0.5);
// Clumps of taller grass per world unit.
const CLUMP_SCALE: f32 = 1.6;

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
// Rock by type (`worldgen::geology::Rock`): two tones each, and their layering. Layered rocks
// (sandstone, shale, limestone) show their beds along the strata surfaces (`level`: the same
// surfaces erosion carved steps along): hard beds in one tone, soft ones in the other, with
// fine bands; massive rocks have their own textures (granite speckles, metamorphic foliation,
// basalt columns).
const ROCK_LIGHT: array<vec3<f32>, 6> = array(
    vec3(0.74, 0.69, 0.65), // granite
    vec3(0.60, 0.60, 0.57), // metamorphic
    vec3(0.38, 0.36, 0.36), // basalt
    vec3(0.80, 0.53, 0.34), // sandstone (hard beds)
    vec3(0.55, 0.50, 0.46), // shale (hard beds)
    vec3(0.85, 0.82, 0.73), // limestone (hard beds)
);
const ROCK_DARK: array<vec3<f32>, 6> = array(
    vec3(0.62, 0.56, 0.53),
    vec3(0.48, 0.49, 0.48),
    vec3(0.28, 0.27, 0.28),
    vec3(0.66, 0.43, 0.33),
    vec3(0.44, 0.40, 0.38),
    vec3(0.70, 0.68, 0.62),
);
// Bed thickness (erosion units) and share of the hard beds (as in `geology::PROPERTIES`).
const ROCK_BEDS: array<vec2<f32>, 6> = array(
    vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(6.0, 0.4), vec2(4.0, 0.18), vec2(9.0, 0.7),
);

fn rock_color(xz: vec2<f32>, height: f32, up: f32, footprint: f32, rock: i32, level: f32) -> vec3<f32> {
    let light = srgb(ROCK_LIGHT[rock].x, ROCK_LIGHT[rock].y, ROCK_LIGHT[rock].z);
    let dark = srgb(ROCK_DARK[rock].x, ROCK_DARK[rock].y, ROCK_DARK[rock].z);
    let wobble = fbm(xz * 0.35 + vec2(height * 0.2, 0.0), 3, footprint * 0.35);
    let flat_patches = fbm(xz * 0.6, 3, footprint * 0.6);
    let beds = ROCK_BEDS[rock];
    var pattern = 0.0;
    if beds.x > 0.0 {
        // Beds: hard (light) and soft (dark), fine bands within, slightly wavy.
        let phase = fract(level / beds.x + wobble * 0.04);
        let hard = 1.0 - smoothstep(beds.y - 0.04, beds.y + 0.04, phase);
        let fine = sin(level / beds.x * 6.2832 * 3.0 + wobble * 2.0) * 0.35;
        pattern = mix(-0.6 + fine, 0.6 + fine, hard);
    } else if rock == 0 {
        // Granite: blotches and speckles.
        pattern = flat_patches + 0.5 * value_noise(xz * 6.0);
    } else if rock == 2 {
        // Basalt: vertical columns on cliffs.
        let columns = value_noise(vec2(dot(xz, vec2(0.7, 0.7)) * 3.0, height * 0.3));
        pattern = mix(columns, flat_patches, smoothstep(0.5, 0.8, up));
    } else {
        // Metamorphic: banded foliation.
        pattern = sin(height * 2.2 + wobble * 2.5);
    }
    // Flat rock tops: patches rather than beds.
    pattern = mix(pattern, flat_patches, smoothstep(0.7, 0.9, up) * 0.6);
    return mix(dark, light, patches(pattern, 0.1));
}

// Grass carpet: the blades are a height field (peaks of value noise) filling a thin layer
// whose top is the terrain surface, and the view ray is marched down through it (parallax
// mapping), so the grass has depth: blades hide each other, their bases are darker, and their
// tips bend downwind (more than their bases). After the grass reference, Shadertoy MdsGzS (see
// CREDITS.md). The blades have a fixed world size, so they follow perspective.
//
// Height of the blade field at `q` (blade units), 0 (ground) to 1 (blade tips): narrow peaks.
// `clump` (0..1, see `clumps`) makes the blades taller and denser.
fn blade_field(q: vec2<f32>, clump: f32) -> f32 {
    let v = value_noise(q) * 0.5 + 0.5;
    return pow(v, 4.0 - 1.5 * clump) * (0.85 + 0.4 * clump);
}

// Clumps of taller grass, a few blades across (world position).
fn clumps(xz: vec2<f32>, footprint: f32) -> f32 {
    return saturate(fbm(xz * CLUMP_SCALE, 2, footprint * CLUMP_SCALE) * 1.2 + 0.5);
}

// March the view ray `dir` through the grass layer from the surface point `p` (normal `n`).
// Returns (height of the blade seen, 0 = ground to 1 = tip; blade coordinates seen).
fn grass_shell(
    xz: vec2<f32>,
    dir2: vec2<f32>,
    descent: f32,
    bend: vec2<f32>,
    clump: f32,
    scale: f32,
    height: f32,
) -> vec3<f32> {
    let down = max(descent, 0.25);
    let span = height / down;
    var previous_h = 1.0;
    var previous_gap = 1.0 - blade_field(xz * scale + bend, clump);
    var q = xz * scale + bend;
    if previous_gap <= 0.0 {
        return vec3(1.0, q);
    }
    for (var i = 1; i <= GRASS_STEPS; i++) {
        let f = f32(i) / f32(GRASS_STEPS);
        let h = 1.0 - f;
        q = (xz + dir2 * (span * f)) * scale + bend * h * h;
        let gap = h - blade_field(q, clump);
        if gap <= 0.0 {
            let k = previous_gap / (previous_gap - gap);
            return vec3(mix(previous_h, h, k), q);
        }
        previous_gap = gap;
        previous_h = h;
    }
    return vec3(0.0, q);
}

// Grass: the carpet, swaying in the wind, with gusts sweeping across meadows as lighter bands
// where the grass bends further. Returns (brightness, flower amount, gust, blade tip amount).
// `dir` is the view direction.
// `clearance` (0..1) flattens the grass near water and on building foundations, so it
// doesn't look like it runs under them.
fn grass_carpet(
    xz: vec2<f32>,
    dir2: vec2<f32>,
    descent: f32,
    footprint: f32,
    time: f32,
    clearance: f32,
) -> vec4<f32> {
    let along = dot(xz, wind.xy);
    let across = dot(xz, vec2(-wind.y, wind.x));
    let gust_noise = fbm(
        vec2(along * 0.12 - time * GUST_SPEED * 0.12, across * 0.05),
        2,
        footprint * 0.12
    );
    let gust = smoothstep(0.05, 0.3, gust_noise);
    let gust_light = 1.0 + 0.18 * gust;

    // Blades have a fixed size; far away, once much finer than a pixel, only the clumps remain
    // (as their average shade).
    let clump = clumps(xz, footprint);
    let far_shade = 0.82 + 0.24 * clump;
    let pixels = 1.0 / (BLADE_SCALE * max(footprint, 1e-5));
    let visible = smoothstep(BLADE_FADE.x, BLADE_FADE.y, pixels) * (1.0 - clearance);
    if visible <= 0.0 {
        return vec4(far_shade * gust_light, 0.0, gust, 0.4 + 0.4 * clump);
    }
    // Swaying: each blade layer bends downwind, more in gusts, with waves running through.
    let sway = 0.6 * sin(time * 3.1 + along * 3.0 + 0.8 * sin(across * 1.7))
        + 0.3 * sin(time * 5.3 + along * 7.0 + across * 2.0);
    let bend = wind.xy * (0.6 + 2.2 * gust + sway);
    let seen = grass_shell(xz, dir2, descent, bend, clump, BLADE_SCALE, BLADE_HEIGHT);
    var shade = blade_shade(seen);
    let h = seen.x;
    let blade = vec2<i32>(floor(seen.yz));
    shade = mix(far_shade, shade, visible) * gust_light;
    // Flowers: on some blade tips, in scattered clumps.
    let clumps = patches(fbm(xz * 0.35 + vec2(31.0, 5.0), 2, footprint * 0.35), 0.5);
    let flower = clumps * step(0.82, hash(blade + vec2(71, 13))) * smoothstep(0.7, 0.85, h)
        * visible;
    return vec4(shade, flower, gust, mix(0.4 + 0.4 * clump, h, visible));
}

// Brightness of the blade seen (`grass_shell`): bases a little shaded, tips lit, and some
// variation between blades.
fn blade_shade(seen: vec3<f32>) -> f32 {
    let blade = vec2<i32>(floor(seen.yz));
    return mix(0.7, 1.06, seen.x) * (0.93 + 0.14 * hash(blade));
}

fn ground(
    xz: vec2<f32>,
    height: f32,
    up: f32,
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    wet: f32,
    clearance: f32,
    dir2: vec2<f32>,
    descent: f32,
    footprint: f32,
    time: f32,
    rock_type: i32,
    level: f32,
    lava: f32,
    reef: f32,
) -> vec3<f32> {
    let steep = 1.0 - up;
    let medium = fbm(xz * 0.5, 3, footprint * 0.5);
    let large = fbm(xz * 0.07 + vec2(7.1, 3.3), 3, footprint * 0.07);
    let blotches = fbm(xz * 0.18 + vec2(2.0, 8.0), 3, footprint * 0.18);
    let carpet = grass_carpet(xz, dir2, descent, footprint, time, clearance);
    let wind_shade = carpet.x;

    // Grass, lush to dry (in a few steps), with lighter patches and a few dirt patches, swept
    // by the wind.
    let dry = saturate(b.r + 0.2 * large);
    let dry_step = floor(dry * 3.0 + 0.5) / 3.0;
    var grass = mix(srgb(0.40, 0.66, 0.24), srgb(0.80, 0.72, 0.38), dry_step);
    grass = mix(grass, grass * vec3(1.1, 1.1, 0.9), patches(blotches, 0.25));
    // Blade tips catch a warmer light; flowers in lush meadows.
    let tips = mix(grass, grass * vec3(1.12, 1.1, 0.8), carpet.w * carpet.w);
    var color = tips * wind_shade;
    let flower_color = mix(srgb(0.95, 0.45, 0.70), srgb(0.98, 0.92, 0.55),
        patches(fbm(xz * 0.05 + vec2(3.0, 3.0), 2, footprint * 0.05), 0.0));
    color = mix(color, flower_color, carpet.y * (1.0 - dry_step));
    let dirt = patches(fbm(xz * 0.5 + vec2(3.3, 1.1), 3, footprint * 0.5), 0.72 - 0.1 * dry);
    color = mix(color, srgb(0.66, 0.52, 0.34), dirt);

    // Alpine grass (short, also wind-swept).
    let alpine = mix(srgb(0.60, 0.68, 0.42), srgb(0.66, 0.66, 0.50), patches(blotches, 0.1));
    color = mix(color, alpine * wind_shade, border(b.a, medium));

    // Forest floors (a sparser, shaded carpet).
    let floor_shade = mix(1.0, wind_shade, 0.6);
    let deciduous = mix(srgb(0.27, 0.48, 0.18), srgb(0.33, 0.44, 0.17), patches(blotches, 0.0));
    color = mix(color, deciduous * floor_shade, border(b.g, medium));
    let conifer = mix(srgb(0.21, 0.38, 0.22), srgb(0.30, 0.34, 0.20), patches(blotches, 0.1));
    color = mix(color, conifer * floor_shade, border(b.b, medium));

    // Marsh: reed beds (dark strokes) around still pools with a muddy rim.
    let pool_noise = fbm(xz * 0.45 + vec2(5.0, 9.0), 3, footprint * 0.45);
    let pools = patches(pool_noise, 0.15);
    let mud = patches(pool_noise, 0.05) - pools;
    let reed_stroke = value_noise(vec2(dot(xz, wind.xy) * 5.0, dot(xz, vec2(-wind.y, wind.x)) * 22.0));
    let reeds = mix(srgb(0.42, 0.52, 0.26), srgb(0.30, 0.40, 0.20), patches(reed_stroke, 0.3))
        * wind_shade;
    let pool_water = mix(srgb(0.16, 0.30, 0.34), srgb(0.30, 0.46, 0.48),
        patches(fbm(xz * 1.5 + vec2(time * 0.05, 0.0), 2, footprint * 1.5), 0.45) * 0.6);
    var marsh = mix(reeds, srgb(0.36, 0.30, 0.20), mud);
    marsh = mix(marsh, pool_water, pools);
    color = mix(color, marsh, border(a.a, medium));

    // Sand: beaches just above the water line (and wherever the climate says so).
    var sand = mix(srgb(0.94, 0.85, 0.62), srgb(0.89, 0.79, 0.56), patches(blotches, 0.2));
    // Black sand on volcanic coasts.
    if rock_type == 2 {
        sand = mix(srgb(0.30, 0.29, 0.30), srgb(0.38, 0.36, 0.36), patches(blotches, 0.2));
    }
    let beach = (1.0 - smoothstep(0.15, 0.45, height + 0.15 * large)) * c.b;
    color = mix(color, sand, border(max(a.r, beach), medium));

    // Desert: warm sand in dune ripples, cracked clay flats.
    let along = dot(xz, wind.xy);
    let ripples = sin(along * 1.4 + fbm(xz * 0.15, 3, footprint * 0.15) * 4.0);
    var desert = mix(srgb(0.90, 0.72, 0.47), srgb(0.80, 0.60, 0.38), patches(ripples, 0.55));
    let flats = patches(fbm(xz * 0.1 + vec2(13.0, 4.0), 3, footprint * 0.1), 0.3);
    let cracks = patches(1.0 - abs(value_noise(xz * 2.5)), 0.93)
        * (1.0 - smoothstep(0.03, 0.08, footprint));
    let clay = mix(srgb(0.82, 0.63, 0.47), srgb(0.62, 0.45, 0.33), cracks);
    desert = mix(desert, clay, flats);
    let desertness = border(c.r, medium);
    color = mix(color, desert, desertness);

    // Salt flats (dry beds of lakes without an outlet): a white crust in polygons.
    let crust = patches(1.0 - abs(value_noise(xz * 1.6)), 0.9) * (1.0 - smoothstep(0.03, 0.08, footprint));
    let salt = mix(srgb(0.93, 0.91, 0.86), srgb(0.80, 0.76, 0.68), crust);
    color = mix(color, salt, border(c.g, medium));

    // Rock: rocky ground, and every steep slope (redder in deserts).
    let steep_value = steep + 0.06 * medium;
    let rock = max(border(a.g, medium), crisp(0.34, steep_value, fwidth(steep_value)));
    let rock_tone = mix(vec3(1.0), vec3(1.12, 0.9, 0.78), desertness);
    color = mix(color, rock_color(xz, height, up, footprint, rock_type, level) * rock_tone, rock);

    // Fresh lava: bare, black, in ropy swirls with a ragged edge (no grass, no forest).
    let lava_edge = lava + 0.25 * medium;
    let swirls = fbm(xz * 1.3 + vec2(fbm(xz * 0.4, 2, footprint * 0.4) * 2.0, 0.0), 3, footprint * 1.3);
    let lava_color = mix(srgb(0.11, 0.10, 0.11), srgb(0.22, 0.20, 0.20), patches(swirls, 0.25));
    color = mix(color, lava_color, crisp(0.5, lava_edge, fwidth(lava_edge)));

    // Snow: settles on flat ground (steep rock stays bare), with a ragged edge and wind-blown
    // drifts.
    let snow_noise = fbm(xz * 0.25 + vec2(3.0, 17.0), 3, footprint * 0.25);
    let snow_value = a.b * 1.25 + 0.3 * snow_noise + 0.6 * (up - 0.85)
        - smoothstep(0.35, 0.6, steep);
    let drifts = patches(fbm(vec2(along * 0.6, dot(xz, vec2(-wind.y, wind.x)) * 0.2), 3,
        footprint * 0.6), 0.25);
    let snow_color = mix(srgb(0.97, 0.98, 1.0), srgb(0.84, 0.89, 0.97), drifts);
    color = mix(color, snow_color, crisp(0.5, snow_value, fwidth(snow_value)));

    // River and lake beds.
    color = mix(color, srgb(0.52, 0.46, 0.36), crisp(0.5, wet, fwidth(wet)));

    // Sea floor (along the ocean: dry land below sea level is land): sand near the shore,
    // teal further out, darker with depth.
    if height < 0.0 && c.b > 0.5 {
        let depth = -height;
        let floor_color = mix(sand, srgb(0.30, 0.46, 0.48), smoothstep(0.3, 3.0, depth));
        color = mix(color, floor_color, smoothstep(0.0, 0.2, depth));
        color *= max(1.0 - depth / 20.0, 0.4);
    }

    // Coral reefs: pastel coral heads under the water, white coral sand on the islets.
    if reef > 0.05 {
        let heads = fbm(xz * 0.9 + vec2(4.0, 1.0), 3, footprint * 0.9);
        let kinds = fbm(xz * 0.3 + vec2(9.0, 2.0), 2, footprint * 0.3);
        var coral = mix(srgb(0.86, 0.60, 0.56), srgb(0.90, 0.76, 0.52), patches(kinds, 0.0));
        coral = mix(coral, srgb(0.52, 0.70, 0.62), patches(kinds, 0.45));
        coral = mix(srgb(0.86, 0.82, 0.70), coral, patches(heads, -0.1));
        let coral_sand = mix(srgb(0.97, 0.94, 0.87), srgb(0.92, 0.88, 0.80), patches(blotches, 0.2));
        let surface = select(coral * max(1.0 + height / 6.0, 0.6), coral_sand, height > 0.02);
        let edge = reef + 0.2 * medium;
        color = mix(color, surface, crisp(0.45, edge, fwidth(edge)));
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
    out = deferred_output(in, pbr_input);
} @else {
    let p = in.world_position.xyz;
    let radial = normalize(p);
    let height = length(p) - planet.x;
    var xz = vec2(0.0);
@if(VERTEX_UVS_A)
    xz = in.uv;
    // Size of a pixel on the ground, in face cells (about world units).
    let footprint = max(length(dpdx(xz)), length(dpdy(xz)));
    let n = pbr_input.N;
    let up = dot(n, radial);

    // Ground weights from the climate (see the header) into the flat map's channels:
    // a = (sand, rock, snow, marsh), b = (dryness, deciduous, conifer, alpine),
    // c = (desert, salt flat, -, -).
    var climate = vec4(0.3, 0.0, 0.0, 0.0);
    var cover = vec2(0.0);
@if(VERTEX_COLORS)
    climate = in.color;
@if(VERTEX_UVS_B)
    cover = in.uv_b;
    let forest = climate.g;
    let cold = min(cover.x, 1.0);
    let snow = max(cover.x - 1.0, 0.0);
    let marsh = max(climate.b, 0.0);
    let salt = max(-climate.b, 0.0);
    let a = vec4(0.0, 0.0, snow, marsh);
    let b = vec4(climate.r, forest * (1.0 - cold), forest * cold, (1.0 - forest) * cold * 0.8);
    // (c.b: how near the ocean, for the beaches.)
    let c = vec4(smoothstep(0.75, 1.0, climate.r), salt, cover.y, 0.0);
    let wet = climate.a;

    // The view direction in face coordinates (for the grass parallax): from the screen space
    // derivatives of the position and of the face coordinates.
    let dir = normalize(p - view.world_position.xyz);
    let jx = dpdx(xz);
    let jy = dpdy(xz);
    let px = dpdx(p);
    let py = dpdy(p);
    let det = jx.x * jy.y - jx.y * jy.x;
    var dir2 = vec2(0.0);
    if abs(det) > 1e-12 {
        // d p / d xz, column by column.
        let pu = (px * jy.y - py * jx.y) / det;
        let pv = (py * jx.x - px * jy.x) / det;
        dir2 = vec2(dot(dir, pu) / max(dot(pu, pu), 1e-8), dot(dir, pv) / max(dot(pv, pv), 1e-8));
    }
    let descent = -dot(dir, n);

    // The rock under the ground: the nearest cell's (from a warped position: irregular
    // borders), and the strata height along its beds.
    var rock_type = 1;
    var level = height;
    var lava = 0.0;
    var reef = 0.0;
    if planet.w > 0.0 {
        let n = i32(planet.y);
        let warp = vec2(value_noise(xz * 0.21), value_noise(xz * 0.21 + vec2(31.0, 7.0))) * 1.2;
        let cell = clamp(vec2<i32>(floor(xz + warp + 0.5)), vec2(0), vec2(n - 1));
        rock_type = clamp(i32(textureLoad(rock_field, cell, 0).r + 0.5), 0, 5);
        let offset = textureSampleLevel(rock_field, rock_sampler, (xz + 0.5) / planet.y, 0.0).g;
        level = height / planet.z + offset;
        let field = textureSampleLevel(rock_field, rock_sampler, (xz + 0.5) / planet.y, 0.0);
        lava = field.b;
        reef = field.a;
    }
    let albedo = ground(xz, height, up, a, b, c, wet, wet, dir2, descent, footprint, globals.time,
        rock_type, level, lava, reef)
        * tint.rgb;

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
    // In-shader post processing (fog, alpha-premultiply, tonemapping if the camera is not HDR).
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(color, 1.0));
}

    return out;
}
