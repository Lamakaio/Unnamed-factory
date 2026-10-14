//! Tectonic history previews: the crust after 0, ¼, ½, ¾ and all of the history (plates
//! tinted, continental crust bright, orogeny dark red: young bright, old faded).
//!
//! `cargo run -p worldgen --release --example tectonics_preview -- <out_dir> [seeds...]`

use std::{fs::File, io::BufWriter, path::Path, time::Instant};

use bevy_math::Vec3;
use worldgen::tectonics::{TectonicSettings, run};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("usage: tectonics_preview <out_dir> [seeds...]");
    std::fs::create_dir_all(&out).unwrap();
    let mut seeds: Vec<u64> = args.map(|a| a.parse().unwrap()).collect();
    if seeds.is_empty() {
        seeds.push(1082);
    }
    for seed in seeds {
        let total = TectonicSettings::default().steps;
        let (width, height) = (512usize, 256usize);
        let frames = [0, total / 4, total / 2, 3 * total / 4, total];
        let mut image = vec![[0u8; 3]; width * height * frames.len()];
        for (f, &steps) in frames.iter().enumerate() {
            let start = Instant::now();
            let t = run(&TectonicSettings { seed, steps, ..Default::default() });
            let land: f32 = t.crust.iter().filter(|&&c| c > 0.5).count() as f32 / t.crust.len() as f32;
            println!(
                "seed {seed}, {steps} steps ({:.0} Myr): {} plates, {:.0}% continental, {} rifts, in {:.2} s",
                steps as f32 * t.settings.step_myr,
                t.plates.len(),
                100. * land,
                t.events.len(),
                start.elapsed().as_secs_f32()
            );
            for y in 0..height {
                let lat = std::f32::consts::FRAC_PI_2 - (y as f32 + 0.5) / height as f32 * std::f32::consts::PI;
                for x in 0..width {
                    let lon = (x as f32 + 0.5) / width as f32 * std::f32::consts::TAU - std::f32::consts::PI;
                    let d = Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
                    let i = t.grid.cell_at(d);
                    let k = t.plate[i] as u32;
                    let hue = [
                        (k.wrapping_mul(97) % 255) as f32 / 255.,
                        (k.wrapping_mul(53) % 255) as f32 / 255.,
                        (k.wrapping_mul(181) % 255) as f32 / 255.,
                    ];
                    let continental = t.crust[i] > 0.5;
                    let base = if continental { 0.55 } else { 0.18 };
                    let mut c = hue.map(|h| base + 0.35 * h * if continental { 1. } else { 0.6 });
                    let young = (-t.orogeny_age[i] / 8.).exp();
                    let mountains = (t.orogeny[i] / 2.).min(1.) * (0.35 + 0.65 * young);
                    c = [c[0] + (0.75 - c[0]) * mountains, c[1] * (1. - mountains), c[2] * (1. - mountains)];
                    image[(f * height + y) * width + x] = c.map(|v| (v.clamp(0., 1.) * 255.) as u8);
                }
            }
        }
        let file = BufWriter::new(File::create(Path::new(&out).join(format!("{seed}_tectonics.png"))).unwrap());
        let mut encoder = png::Encoder::new(file, width as u32, (height * frames.len()) as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(image.as_flattened()).unwrap();
    }
}
