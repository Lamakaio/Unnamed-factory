//! Render generated continents to PNG, to tune world generation without the game.
//!
//! `cargo run -p worldgen --release --example preview -- <out_dir> [seed...]`

use std::{fs::File, io::BufWriter, path::PathBuf};

use worldgen::{Continent, WorldgenSettings, generate};

/// Output pixels per cell.
const SCALE: usize = 2;

fn main() {
    let mut args = std::env::args().skip(1);
    let out_dir = PathBuf::from(args.next().unwrap_or_else(|| "worldgen-preview".into()));
    std::fs::create_dir_all(&out_dir).unwrap();
    let seeds: Vec<u64> = args
        .map(|s| s.parse().expect("seeds are numbers"))
        .collect();
    let seeds = if seeds.is_empty() { vec![1082] } else { seeds };

    for seed in seeds {
        let continent = generate(&WorldgenSettings {
            seed,
            ..Default::default()
        });
        let land = continent.heights.iter().filter(|&&h| h > 0.).count();
        println!(
            "seed {seed}: {} rivers, {} lakes, {:.0}% land, highest {:.1}",
            continent.rivers.len(),
            continent.lakes.len(),
            100. * land as f32 / continent.heights.len() as f32,
            continent.heights.iter().copied().fold(f32::MIN, f32::max),
        );
        for (stage, time) in &continent.timings {
            println!("  {stage}: {time:.2?}");
        }
        let path = out_dir.join(format!("continent_{seed}.png"));
        save(&path, &render(&continent));
        println!("  -> {}", path.display());
    }
}

fn render(continent: &Continent) -> Vec<[u8; 3]> {
    let size = continent.size();
    let side = size * SCALE;
    let height = |x: usize, z: usize| continent.heights[z.min(size - 1) * size + x.min(size - 1)];
    let mut image = vec![[0u8; 3]; side * side];
    for z in 0..size {
        for x in 0..size {
            let h = height(x, z);
            // Hillshade, light from the north west.
            let dx = height(x + 1, z) - height(x.saturating_sub(1), z);
            let dz = height(x, z + 1) - height(x, z.saturating_sub(1));
            let shade = (1. + (-dx - dz) * 0.15).clamp(0.4, 1.4);
            let color = match continent.lake_of[z * size + x] {
                Some(_) => [70., 120., 200.],
                None if h <= 0. => {
                    let t = (-h / continent.settings.sea_depth).clamp(0., 1.);
                    [40. - 20. * t, 90. - 40. * t, 160. - 40. * t]
                }
                None => hypsometric(h / continent.settings.max_height),
            };
            let shade = if h <= 0. { 1. } else { shade };
            let pixel = color.map(|c| (c * shade).clamp(0., 255.) as u8);
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

/// Land colors from lowlands to summits (`t` = height / max height).
fn hypsometric(t: f32) -> [f32; 3] {
    const STOPS: [(f32, [f32; 3]); 6] = [
        (0.0, [220., 205., 150.]),
        (0.03, [110., 160., 80.]),
        (0.25, [70., 130., 60.]),
        (0.5, [140., 120., 80.]),
        (0.75, [120., 110., 105.]),
        (0.9, [240., 240., 245.]),
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

fn save(path: &PathBuf, image: &[[u8; 3]]) {
    let side = (image.len() as f32).sqrt() as u32;
    let file = BufWriter::new(File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, side, side);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(image.as_flattened()).unwrap();
}
