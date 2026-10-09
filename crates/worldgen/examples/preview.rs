//! Render generated continents to PNG, to tune world generation without the game.
//!
//! `cargo run -p worldgen --release --example preview -- <out_dir> [seed...]`
//!
//! For each seed, writes `<seed>_biomes.png`, `<seed>_landforms.png`, `<seed>_relief.png` (all
//! hillshaded, with rivers and lakes) and `<seed>_detail.png` (a crop of the fine surface).

use std::{fs::File, io::BufWriter, path::Path};

use worldgen::{Continent, Landform, WorldgenSettings, generate};

/// Output pixels per cell.
const SCALE: usize = 2;

fn main() {
    let mut args = std::env::args().skip(1);
    let out_dir = args.next().unwrap_or_else(|| "worldgen-preview".into());
    let out_dir = Path::new(&out_dir);
    std::fs::create_dir_all(out_dir).unwrap();
    let seeds: Vec<u64> = args.map(|s| s.parse().expect("seeds are numbers")).collect();
    let seeds = if seeds.is_empty() { vec![1082] } else { seeds };

    for seed in seeds {
        let continent = generate(&WorldgenSettings {
            seed,
            ..Default::default()
        });
        let land = continent.heights.iter().filter(|&&h| h > 0.).count();
        println!(
            "seed {seed}: {} rivers, {} lakes, {:.0}% land, highest {:.1}, wind {:.0}°",
            continent.rivers.len(),
            continent.lakes.len(),
            100. * land as f32 / continent.heights.len() as f32,
            continent.heights.iter().copied().fold(f32::MIN, f32::max),
            continent.wind_angle.to_degrees(),
        );
        for (stage, time) in &continent.timings {
            println!("  {stage}: {time:.2?}");
        }
        let size = continent.size();
        print_landform_stats(&continent);
        let maps: [(&str, Box<dyn Fn(usize) -> [f32; 3]>); 3] = [
            ("biomes", Box::new(|i| continent.biome[i].color())),
            ("landforms", Box::new(|i| continent.landform[i].color())),
            (
                "relief",
                Box::new(|i| hypsometric(continent.heights[i] / continent.settings.max_height)),
            ),
        ];
        for (name, color) in maps {
            let path = out_dir.join(format!("{seed}_{name}.png"));
            save(&path, size * SCALE, &render(&continent, &*color));
        }
        let path = out_dir.join(format!("{seed}_detail.png"));
        let (side, image) = render_detail(&continent);
        save(&path, side, &image);
        println!("  -> {}", out_dir.display());
    }
}

/// Share of land, median and 95th percentile height and slope of each landform.
fn print_landform_stats(continent: &Continent) {
    let size = continent.size();
    let land = continent.heights.iter().filter(|&&h| h > 0.).count().max(1);
    for landform in [
        Landform::Plains,
        Landform::Hills,
        Landform::Plateau,
        Landform::Mountains,
    ] {
        let cells: Vec<usize> = (0..size * size)
            .filter(|&i| continent.landform[i] == landform && continent.heights[i] > 0.)
            .collect();
        if cells.is_empty() {
            continue;
        }
        let mut heights: Vec<f32> = cells.iter().map(|&i| continent.heights[i]).collect();
        let mut slopes: Vec<f32> = cells
            .iter()
            .map(|&i| {
                let (x, z) = ((i % size) as i64, (i / size) as i64);
                let at = |x: i64, z: i64| {
                    continent.heights
                        [z.clamp(0, size as i64 - 1) as usize * size + x.clamp(0, size as i64 - 1) as usize]
                };
                let dx = (at(x + 1, z) - at(x - 1, z)) / 2.;
                let dz = (at(x, z + 1) - at(x, z - 1)) / 2.;
                (dx * dx + dz * dz).sqrt() / continent.settings.cell_size
            })
            .collect();
        heights.sort_by(f32::total_cmp);
        slopes.sort_by(f32::total_cmp);
        let at = |values: &[f32], q: usize| values[(values.len() - 1) * q / 100];
        println!(
            "  {landform:?}: {:.0}% of land, height {:.1} / {:.1}, slope {:.2} / {:.2} (median / p95)",
            100. * cells.len() as f32 / land as f32,
            at(&heights, 50),
            at(&heights, 95),
            at(&slopes, 50),
            at(&slopes, 95),
        );
    }
}

/// Hillshaded map colored per cell, with lakes and rivers.
fn render(continent: &Continent, color: &dyn Fn(usize) -> [f32; 3]) -> Vec<[u8; 3]> {
    let size = continent.size();
    let side = size * SCALE;
    let height = |x: usize, z: usize| continent.heights[z.min(size - 1) * size + x.min(size - 1)];
    let mut image = vec![[0u8; 3]; side * side];
    for z in 0..size {
        for x in 0..size {
            let i = z * size + x;
            let h = height(x, z);
            // Hillshade, light from the north west.
            let dx = height(x + 1, z) - height(x.saturating_sub(1), z);
            let dz = height(x, z + 1) - height(x, z.saturating_sub(1));
            let shade = if h <= 0. {
                1.
            } else {
                (1. + (-dx - dz) * 0.15).clamp(0.4, 1.4)
            };
            let base = match continent.lake_of[i] {
                Some(_) => [0.27, 0.47, 0.78],
                None if h <= 0. => {
                    let t = (-h / continent.settings.sea_depth).clamp(0., 1.);
                    [0.16 - 0.08 * t, 0.35 - 0.15 * t, 0.63 - 0.15 * t]
                }
                None => color(i),
            };
            let pixel = base.map(|c| (c * shade * 255.).clamp(0., 255.) as u8);
            for py in 0..SCALE {
                for px in 0..SCALE {
                    image[(z * SCALE + py) * side + x * SCALE + px] = pixel;
                }
            }
        }
    }

    // Rivers, drawn as discs along their polylines.
    let half = (size / 2) as f32;
    let cell = continent.settings.cell_size;
    for river in &continent.rivers {
        for point in &river.points {
            let center =
                (point.position.truncate().with_y(point.position.z) / cell + half) * SCALE as f32;
            let radius = (point.width / cell * SCALE as f32 / 2.).max(0.7);
            let (x0, x1) = ((center.x - radius) as i64, (center.x + radius) as i64);
            let (y0, y1) = ((center.y - radius) as i64, (center.y + radius) as i64);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if x < 0 || y < 0 || x >= side as i64 || y >= side as i64 {
                        continue;
                    }
                    let d = ((x as f32 - center.x).powi(2) + (y as f32 - center.y).powi(2)).sqrt();
                    if d <= radius {
                        image[y as usize * side + x as usize] = [40, 90, 200];
                    }
                }
            }
        }
    }
    image
}

/// Hillshade of a 512×512 crop of the fine surface, around the highest point.
fn render_detail(continent: &Continent) -> (usize, Vec<[u8; 3]>) {
    let surface = &continent.surface;
    let side = 512.min(surface.size);
    let highest = (0..surface.values.len())
        .max_by(|&a, &b| surface.values[a].total_cmp(&surface.values[b]))
        .unwrap_or(0);
    let (cx, cz) = (highest % surface.size, highest / surface.size);
    let x0 = cx.saturating_sub(side / 2).min(surface.size - side);
    let z0 = cz.saturating_sub(side / 2).min(surface.size - side);
    let at = |x: usize, z: usize| surface.at(x as i64, z as i64);
    let mut image = vec![[0u8; 3]; side * side];
    for z in 0..side {
        for x in 0..side {
            let (gx, gz) = (x0 + x, z0 + z);
            let dx = (at(gx + 1, gz) - at(gx.saturating_sub(1), gz)) / (2. * surface.cell);
            let dz = (at(gx, gz + 1) - at(gx, gz.saturating_sub(1))) / (2. * surface.cell);
            // Lambert shading, light from the north west.
            let normal = [-dx, 1., -dz];
            let light = [-0.35, 0.87, -0.35];
            let dot = (0..3).map(|k| normal[k] * light[k]).sum::<f32>()
                / (dx * dx + 1. + dz * dz).sqrt()
                / (0.35f32 * 0.35 * 2. + 0.87 * 0.87).sqrt();
            image[z * side + x] = [(40. + 200. * dot.max(0.)).min(255.) as u8; 3];
        }
    }
    (side, image)
}

/// Land colors from lowlands to summits (`t` = height / max height).
fn hypsometric(t: f32) -> [f32; 3] {
    const STOPS: [(f32, [f32; 3]); 6] = [
        (0.0, [0.86, 0.80, 0.59]),
        (0.03, [0.43, 0.63, 0.31]),
        (0.25, [0.27, 0.51, 0.24]),
        (0.5, [0.55, 0.47, 0.31]),
        (0.75, [0.47, 0.43, 0.41]),
        (0.9, [0.94, 0.94, 0.96]),
    ];
    let t = t.clamp(0., 1.);
    for pair in STOPS.windows(2) {
        let ((a, ca), (b, cb)) = (pair[0], pair[1]);
        if t <= b {
            let f = (t - a) / (b - a);
            return [0, 1, 2].map(|k| ca[k] + (cb[k] - ca[k]) * f);
        }
    }
    STOPS[STOPS.len() - 1].1
}

fn save(path: &Path, side: usize, image: &[[u8; 3]]) {
    let file = BufWriter::new(File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, side as u32, side as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(image.as_flattened()).unwrap();
}
