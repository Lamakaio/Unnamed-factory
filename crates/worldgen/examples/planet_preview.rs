//! Planet previews: a shaded relief map (equirectangular), two globe views and the plates.
//!
//! `cargo run -p worldgen --release --example planet_preview -- <out_dir> [--n <cells>] [--coarse <steps>] [--no-history] [seeds...]`
//!
//! Writes `<seed>_map.png`, `<seed>_globes.png` and `<seed>_plates.png` (continental plates
//! bright, oceanic dark, land brightest, ranges darkened), and prints the
//! planet's statistics and timings.

use std::{fs::File, io::BufWriter, path::Path};

use bevy_math::{Vec2, Vec3};
use worldgen::{
    grid::Grid,
    planet::{Planet, PlanetSettings, generate_planet},
};

/// Drainage area (cells) from which a cell is drawn as a river.
const RIVER_AREA: f32 = 150.;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .expect("usage: planet_preview <out_dir> [--n <cells>] [--coarse <steps>] [seeds...]");
    std::fs::create_dir_all(&out).unwrap();
    let mut n = PlanetSettings::default().n;
    let mut coarse_steps = PlanetSettings::default().coarse_steps;
    let mut history = PlanetSettings::default().history;
    let mut seeds = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--n" {
            n = args.next().unwrap().parse().unwrap();
        } else if arg == "--no-history" {
            history = None;
        } else if arg == "--coarse" {
            coarse_steps = args.next().unwrap().parse().unwrap();
        } else {
            seeds.push(arg.parse::<u64>().unwrap());
        }
    }
    if seeds.is_empty() {
        seeds.push(PlanetSettings::default().seed);
    }
    for seed in seeds {
        let planet = generate_planet(&PlanetSettings {
            seed,
            n,
            coarse_steps,
            history: history.clone(),
            ..Default::default()
        });
        report(seed, &planet);
        let shade = hillshade(&planet);
        let colour = |i: usize| relief_colour(&planet, &shade, i);
        write_map(
            &Path::new(&out).join(format!("{seed}_map.png")),
            &planet,
            colour,
        );
        write_globes(
            &Path::new(&out).join(format!("{seed}_globes.png")),
            &planet,
            colour,
        );
        let plates = |i: usize| {
            let k = planet.plate[i] as u32;
            let hue = [
                (k.wrapping_mul(97) % 255) as f32 / 255.,
                (k.wrapping_mul(53) % 255) as f32 / 255.,
                (k.wrapping_mul(181) % 255) as f32 / 255.,
            ];
            // Continental plates bright, oceanic ones dark; land brighter still.
            let tint = if planet.continental_plate[k as usize] {
                0.75
            } else {
                0.35
            } + if planet.sea[i] { 0. } else { 0.25 };
            let mountain = planet.range[i];
            hue.map(|c| (c * 0.6 + 0.3) * tint * (1. - 0.5 * mountain))
        };
        write_map(
            &Path::new(&out).join(format!("{seed}_plates.png")),
            &planet,
            plates,
        );
    }
}

fn report(seed: u64, planet: &Planet) {
    let cells = planet.grid.len();
    let land: Vec<f32> = (0..cells)
        .filter(|&i| !planet.sea[i])
        .map(|i| planet.heights[i])
        .collect();
    let mut sorted = land.clone();
    sorted.sort_by(f32::total_cmp);
    let percentile = |p: f32| sorted[((sorted.len() - 1) as f32 * p) as usize];
    println!(
        "seed {seed}: {} cells (n = {}), radius {:.0}, land {:.0}%, land heights p50 {:.1} p90 {:.1} p99 {:.1} max {:.1}",
        cells,
        planet.settings.n,
        planet.radius(),
        100. * land.len() as f32 / cells as f32,
        percentile(0.5),
        percentile(0.9),
        percentile(0.99),
        percentile(1.),
    );
    // A hash of the result, to check that optimisations leave it unchanged.
    let hash = planet
        .heights
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, v| {
            (h ^ v.to_bits() as u64).wrapping_mul(0x0100_0000_01b3)
        });
    println!("  heights hash {hash:016x}");
    for (name, duration) in &planet.timings {
        println!("  {name}: {:.2} s", duration.as_secs_f32());
    }
}

/// Lambert shading of each cell, lit from the north-west in its own tangent frame.
fn hillshade(planet: &Planet) -> Vec<f32> {
    let grid = &planet.grid;
    let spacing = std::f32::consts::FRAC_PI_2 / planet.settings.n as f32;
    let light = Vec3::new(-1., 1., 1.4).normalize();
    (0..grid.len())
        .map(|i| {
            let up = grid.direction(i);
            let north = (Vec3::Y - up * up.y).try_normalize().unwrap_or(Vec3::X);
            let east = up.cross(north);
            // Least-squares gradient (height per cell) from the neighbours.
            let (mut ee, mut en, mut nn, mut eh, mut nh) = (0., 0., 0., 0., 0.);
            for (j, _) in grid.neighbours(i) {
                let t = (grid.direction(j) - up) / spacing;
                let (e, n) = (t.dot(east), t.dot(north));
                let dh = planet.heights[j].max(0.) - planet.heights[i].max(0.);
                ee += e * e;
                en += e * n;
                nn += n * n;
                eh += e * dh;
                nh += n * dh;
            }
            let det = ee * nn - en * en;
            let gradient = if det.abs() > 1e-9 {
                Vec2::new((nn * eh - en * nh) / det, (ee * nh - en * eh) / det)
            } else {
                Vec2::ZERO
            };
            let normal = Vec3::new(-gradient.x, -gradient.y, 1.).normalize();
            (normal.dot(light) / light.z).clamp(0., 1.3)
        })
        .collect()
}

fn relief_colour(planet: &Planet, shade: &[f32], i: usize) -> [f32; 3] {
    let h = planet.heights[i];
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
        let t = t.clamp(0., 1.);
        [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
    };
    if planet.sea[i] {
        let depth = (-h / planet.settings.sea_depth).clamp(0., 1.);
        return mix([0.32, 0.58, 0.72], [0.06, 0.16, 0.34], depth.sqrt());
    }
    let stops = [
        (0., [0.36, 0.56, 0.27]),
        (6., [0.52, 0.62, 0.32]),
        (14., [0.58, 0.50, 0.33]),
        (24., [0.55, 0.48, 0.42]),
        (32., [0.70, 0.68, 0.66]),
        (38., [0.95, 0.95, 0.97]),
    ];
    let mut colour = stops[stops.len() - 1].1;
    for w in stops.windows(2) {
        if h < w[1].0 {
            colour = mix(w[0].1, w[1].1, (h - w[0].0) / (w[1].0 - w[0].0));
            break;
        }
    }
    if planet.lake_depth[i] > 0.05 {
        return mix(
            [0.25, 0.5, 0.78],
            [0.12, 0.3, 0.6],
            planet.lake_depth[i] / 4.,
        );
    }
    let colour = colour.map(|c| c * shade[i]);
    let river = planet.drainage[i] / RIVER_AREA;
    if river >= 1. {
        let strength = (0.55 + 0.15 * river.ln()).min(1.);
        return mix(colour, [0.15, 0.35, 0.75], strength);
    }
    colour
}

/// Equirectangular map, 2:1, about two pixels per cell at the equator.
fn write_map(path: &Path, planet: &Planet, colour: impl Fn(usize) -> [f32; 3]) {
    let width = (planet.settings.n * 8).min(4096);
    let height = width / 2;
    let mut image = vec![[0u8; 3]; width * height];
    for y in 0..height {
        let latitude =
            std::f32::consts::FRAC_PI_2 - (y as f32 + 0.5) / height as f32 * std::f32::consts::PI;
        for x in 0..width {
            let longitude =
                (x as f32 + 0.5) / width as f32 * std::f32::consts::TAU - std::f32::consts::PI;
            let direction = Vec3::new(
                latitude.cos() * longitude.cos(),
                latitude.sin(),
                latitude.cos() * longitude.sin(),
            );
            image[y * width + x] = to_rgb(colour(planet.grid.cell_at(direction)));
        }
    }
    save(path, width, height, &image);
}

/// Two orthographic views of the globe, from opposite sides, tilted 20° toward the north.
fn write_globes(path: &Path, planet: &Planet, colour: impl Fn(usize) -> [f32; 3]) {
    let side = 768;
    let mut image = vec![[8u8, 8, 14]; 2 * side * side];
    for (view, longitude) in [0f32, 180f32].into_iter().enumerate() {
        let (longitude, tilt) = (longitude.to_radians(), 20f32.to_radians());
        let forward = Vec3::new(
            tilt.cos() * longitude.cos(),
            tilt.sin(),
            tilt.cos() * longitude.sin(),
        );
        let right = Vec3::Y.cross(forward).normalize();
        let up = forward.cross(right);
        for y in 0..side {
            for x in 0..side {
                let u = (x as f32 + 0.5) / side as f32 * 2.2 - 1.1;
                let v = 1.1 - (y as f32 + 0.5) / side as f32 * 2.2;
                let r2 = u * u + v * v;
                if r2 >= 1. {
                    continue;
                }
                let p = forward * (1. - r2).sqrt() - right * u + up * v;
                // A soft limb: darker toward the edge of the disc.
                let limb = 0.55 + 0.45 * (1. - r2).sqrt();
                let c = colour(planet.grid.cell_at(p)).map(|c| c * limb);
                image[y * 2 * side + view * side + x] = to_rgb(c);
            }
        }
    }
    save(path, 2 * side, side, &image);
}

fn to_rgb(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| (v.clamp(0., 1.) * 255.) as u8)
}

fn save(path: &Path, width: usize, height: usize, image: &[[u8; 3]]) {
    let file = BufWriter::new(File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(image.as_flattened()).unwrap();
}
