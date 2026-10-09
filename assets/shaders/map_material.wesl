// Terrain material: standard PBR lighting on a procedurally drawn ground.
//
// - Normals are per pixel: the base surface gradient comes from a texture covering the
//   continent, plus the gradient added by rivers and edits (mesh UVs), plus small bumps per
//   material. Lighting keeps the full surface detail at any mesh LOD.
// - Ground materials (grass, dirt, forest floors, alpine grass, marsh, sand, rock, snow, river
//   beds, sea floor) are drawn from world-space noise. Their weights come from two textures
//   baked per continent cell (`map::ground`), warped and sharpened with noise so borders are
//   irregular; rock on steep slopes and snow sliding off them are decided per pixel. Mesh
//   vertex color red is the river/lake wetness.
// - Noise octaves finer than about a pixel fade out, so distant ground doesn't shimmer.
//
// Everything is multiplied by `tint`.

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
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var terrain_sampler: sampler;
// Texture coordinates from world xz: `uv = xz * zw + xy`. Zero until the texture is bound.
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> gradient_map: vec4<f32>;
// Ground weights: (sand, rock, snow, marsh) and (dryness, deciduous, conifer, alpine).
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var ground_a: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var ground_b: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var<uniform> ground_map: vec4<f32>;

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
// Materials

fn srgb(r: f32, g: f32, b: f32) -> vec3<f32> {
    return pow(vec3(r, g, b), vec3(2.2));
}

// A cell weight (bilinear, smooth over a cell) made into an irregular, fairly crisp border.
fn edge(weight: f32, noise: f32) -> f32 {
    return smoothstep(0.4, 0.6, weight + 0.35 * noise);
}

// Rock, projected from the three axes (cliffs are as detailed as flat rock): mottled stone,
// strata on cliffs, dark cracks.
fn rock_color(p: vec3<f32>, n: vec3<f32>, footprint: f32) -> vec3<f32> {
    var w = pow(abs(n), vec3(4.0));
    w /= w.x + w.y + w.z;
    let f = footprint * 0.8;
    let mottle = fbm(p.zy * 0.8, 4, f) * w.x + fbm(p.xz * 0.8, 4, f) * w.y
        + fbm(p.xy * 0.8, 4, f) * w.z;
    let strata = sin(p.y * 2.7 + mottle * 3.0) * 0.5 + 0.5;
    var color = mix(srgb(0.36, 0.34, 0.32), srgb(0.56, 0.53, 0.49), saturate(0.5 + mottle * 0.7));
    color *= 0.9 + 0.15 * strata * (1.0 - w.y);
    let cracks = 1.0 - abs(fbm(p.xz * 1.7 + vec2(p.y * 0.9, 0.0), 3, footprint * 1.7));
    color *= 1.0 - 0.45 * smoothstep(0.88, 0.97, cracks);
    return color;
}

struct Ground {
    albedo: vec3<f32>,
    roughness: f32,
    // Amplitudes of the fine and coarse bumps (world units).
    bump_fine: f32,
    bump_coarse: f32,
}

fn ground(
    p: vec3<f32>,
    n: vec3<f32>,
    a: vec4<f32>,
    b: vec4<f32>,
    wet: f32,
    footprint: f32,
) -> Ground {
    let xz = p.xz;
    let steep = 1.0 - n.y;
    // Medium and large scale noise shared by the borders and colour drifts.
    let medium = fbm(xz * 0.5, 3, footprint * 0.5);
    let large = fbm(xz * 0.07 + vec2(7.1, 3.3), 3, footprint * 0.07);

    // Grass, lush to dry, drifting in colour over large areas, with lighter tufts.
    let dry = saturate(b.r + 0.25 * large);
    var grass = mix(srgb(0.25, 0.40, 0.12), srgb(0.62, 0.56, 0.30), dry);
    grass *= 0.85 + 0.2 * fbm(xz * 0.9, 3, footprint * 0.9);
    let tufts = fbm(xz * 6.0, 2, footprint * 6.0);
    grass = mix(grass, grass * vec3(1.15, 1.12, 0.85), saturate(tufts * 1.5));
    // Fine grain, only visible up close.
    grass *= 0.92 + 0.16 * fbm(xz * 16.0, 2, footprint * 16.0);
    var color = grass;
    var roughness = 0.9;
    var bump_fine = 0.02;
    var bump_coarse = 0.0;

    // Bare dirt patches, more in dry grass.
    let dirt_color = srgb(0.50, 0.41, 0.28) * (0.85 + 0.3 * fbm(xz * 2.3, 3, footprint * 2.3));
    let dirt = smoothstep(0.72, 0.85, fbm(xz * 0.6 + vec2(3.3, 1.1), 3, footprint * 0.6) * 0.5
        + 0.5 + 0.08 * dry);
    color = mix(color, dirt_color, dirt * 0.7);

    // Alpine grass: short and pale, stony.
    let alpine = srgb(0.45, 0.48, 0.30) * (0.85 + 0.3 * fbm(xz * 1.7, 3, footprint * 1.7));
    color = mix(color, alpine, edge(b.a, medium));

    // Forest floors: dark undergrowth with patches of leaf litter, or moss with patches of
    // needles.
    let undergrowth = srgb(0.17, 0.29, 0.09) * (0.85 + 0.25 * fbm(xz * 2.0, 3, footprint * 2.0));
    let litter = mix(undergrowth, srgb(0.34, 0.27, 0.15),
        smoothstep(0.35, 0.7, fbm(xz * 1.3 + vec2(11.0, 2.0), 3, footprint * 1.3)));
    color = mix(color, litter, edge(b.g, medium));
    let moss = srgb(0.16, 0.26, 0.10) * (0.85 + 0.25 * fbm(xz * 2.4, 3, footprint * 2.4));
    let needles = mix(moss, srgb(0.27, 0.22, 0.13),
        smoothstep(0.2, 0.6, fbm(xz * 0.9 + vec2(23.0, 5.0), 3, footprint * 0.9)));
    color = mix(color, needles, edge(b.b, medium));

    // Marsh: reeds, mud and shiny puddles.
    let marsh = edge(a.a, medium);
    let mud = srgb(0.25, 0.22, 0.16);
    let puddle = smoothstep(0.15, 0.3, fbm(xz * 0.6 + vec2(5.0, 9.0), 3, footprint * 0.6));
    let reeds = srgb(0.27, 0.34, 0.14) * (0.85 + 0.3 * fbm(xz * 3.0, 2, footprint * 3.0));
    color = mix(color, mix(reeds, mud * 0.6, puddle), marsh);
    roughness = mix(roughness, mix(0.85, 0.15, puddle), marsh);

    // Sand: fine grain and faint wind ripples.
    let sand_weight = edge(a.r, medium);
    let ripples = sin(dot(xz, vec2(0.8, 0.6)) * 6.0 + fbm(xz * 0.5, 2, footprint * 0.5) * 4.0)
        * (1.0 - smoothstep(0.05, 0.15, footprint));
    let sand = srgb(0.84, 0.75, 0.54)
        * (0.92 + 0.08 * fbm(xz * 5.0, 2, footprint * 5.0) + 0.03 * ripples);
    color = mix(color, sand, sand_weight);
    roughness = mix(roughness, 0.95, sand_weight);
    bump_fine = mix(bump_fine, 0.008, sand_weight);

    // Rock: rocky ground, and every steep slope.
    let rock = max(edge(a.g, medium), smoothstep(0.28, 0.45, steep + 0.08 * medium));
    color = mix(color, rock_color(p, n, footprint), rock);
    roughness = mix(roughness, 0.75, rock);
    bump_coarse = rock * 0.12;

    // Snow, which slides off steep slopes.
    let snow = edge(a.b, medium) * (1.0 - smoothstep(0.4, 0.65, steep));
    color = mix(color, srgb(0.92, 0.94, 0.97), snow);
    roughness = mix(roughness, 0.55, snow);
    bump_coarse *= 1.0 - snow;

    // River and lake beds: gravel and mud, dark and shiny.
    let bed_weight = smoothstep(0.2, 0.8, wet);
    let bed = mix(srgb(0.30, 0.27, 0.21), srgb(0.42, 0.40, 0.35),
        smoothstep(0.0, 0.4, fbm(xz * 3.0, 2, footprint * 3.0))) * 0.8;
    color = mix(color, bed, bed_weight);
    roughness = mix(roughness, 0.5, bed_weight);

    // Sea floor: sand near the shore, mud deeper, darker with depth (so shallows don't read
    // as dry sand through the water).
    if p.y < 0.0 {
        let depth = -p.y;
        let sea_floor = mix(sand, mud, smoothstep(0.5, 6.0, depth));
        color = mix(color, sea_floor, smoothstep(0.0, 0.3, depth));
        color *= (1.0 - 0.4 * min(depth / 0.3, 1.0)) * max(1.0 - depth / 25.0, 0.35);
    }

    return Ground(color, roughness, bump_fine, bump_coarse);
}

fn bump_height(xz: vec2<f32>, ground: Ground, footprint: f32) -> f32 {
    return fbm(xz * 4.0, 2, footprint * 4.0) * ground.bump_fine
        + fbm(xz * 1.2, 3, footprint * 1.2) * ground.bump_coarse;
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
    }
    var wet = 0.0;
@if(VERTEX_COLORS)
    wet = in.color.r;

    let surface = ground(p, n, a, b, wet, footprint);
    pbr_input.material.base_color = vec4(surface.albedo * tint.rgb, 1.0);
    pbr_input.material.perceptual_roughness = surface.roughness;

    // Material bumps, as a surface gradient.
    let e = 0.05;
    let h = bump_height(p.xz, surface, footprint);
    let slope = vec3(
        bump_height(p.xz + vec2(e, 0.0), surface, footprint) - h,
        0.0,
        bump_height(p.xz + vec2(0.0, e), surface, footprint) - h
    ) / e;
    let tangential = slope - n * dot(n, slope);
    pbr_input.world_normal = n;
    pbr_input.N = normalize(n - tangential);

    out.color = apply_pbr_lighting(pbr_input);
    // In-shader post processing (fog, alpha-premultiply, tonemapping if the camera is not HDR).
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}

    return out;
}
