//! Planet climate previews: Köppen, annual precipitation and January / July temperature maps
//! (equirectangular), and zonal means to check the patterns everyone knows.
//!
//! `cargo run -p worldgen --release --example climate_preview -- <out_dir> [--n <cells>] [seeds...]`

use std::{fs::File, io::BufWriter, path::Path, time::Instant};

use bevy_math::Vec3;
use worldgen::{
    grid::Grid,
    planet::{PlanetSettings, generate_planet},
    planet_climate::{ClimateSettings, Koppen, simulate},
};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("usage: climate_preview <out_dir> [--n <cells>] [seeds...]");
    std::fs::create_dir_all(&out).unwrap();
    let mut n = 256;
    let mut seeds = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--n" {
            n = args.next().unwrap().parse().unwrap();
        } else {
            seeds.push(arg.parse::<u64>().unwrap());
        }
    }
    if seeds.is_empty() {
        seeds.push(1082);
    }
    for seed in seeds {
        let planet = generate_planet(&PlanetSettings { seed, n, ..Default::default() });
        let start = Instant::now();
        let climate = simulate(&planet, &ClimateSettings::default());
        println!("seed {seed}: climate in {:.2} s", start.elapsed().as_secs_f32());

        // Zonal means over 10° bands.
        println!("  lat   land%  T(year) T(Jan) T(Jul)  P land  P sea (mm/year)");
        for band in 0..18 {
            let (lo, hi) = (-90. + band as f32 * 10., -80. + band as f32 * 10.);
            let (mut n_land, mut n_all) = (0., 0.);
            let (mut t_year, mut t_jan, mut t_jul) = (0., 0., 0.);
            let (mut p_land, mut p_sea, mut n_sea) = (0., 0., 0.);
            for c in 0..climate.months.len() {
                let d = climate.grid.direction(c);
                let lat = d.y.asin().to_degrees();
                if lat < lo || lat >= hi {
                    continue;
                }
                let m = &climate.months[c];
                let p: f32 = m.iter().map(|m| m.precipitation).sum();
                n_all += 1.;
                t_year += m.iter().map(|m| m.temperature).sum::<f32>() / 12.;
                t_jan += m[0].temperature;
                t_jul += m[6].temperature;
                if climate.land[c] > 0.5 {
                    n_land += 1.;
                    p_land += p;
                } else {
                    n_sea += 1.;
                    p_sea += p;
                }
            }
            println!(
                "  {:+3.0}  {:5.0}  {:6.1}  {:6.1} {:6.1}  {:6.0}  {:6.0}",
                (lo + hi) / 2.,
                100. * n_land / n_all,
                t_year / n_all,
                t_jan / n_all,
                t_jul / n_all,
                if n_land > 0. { p_land / n_land } else { f32::NAN },
                if n_sea > 0. { p_sea / n_sea } else { f32::NAN },
            );
        }
        // Köppen shares on land.
        let mut shares = std::collections::HashMap::new();
        let mut land = 0.;
        for i in 0..planet.grid.len() {
            if planet.sea[i] {
                continue;
            }
            land += 1.;
            *shares.entry(climate.koppen_at(planet.grid.direction(i), planet.heights[i])).or_insert(0.) += 1.;
        }
        let mut line = String::new();
        for k in Koppen::ALL {
            if let Some(count) = shares.get(&k) {
                line += &format!("{k:?} {:.0}%  ", 100. * count / land);
            }
        }
        println!("  land: {line}");

        let path = |name: &str| Path::new(&out).join(format!("{seed}_{name}.png"));
        write_map(&path("koppen"), |d| {
            let i = planet.grid.cell_at(d);
            if planet.sea[i] {
                [0.12, 0.16, 0.25]
            } else {
                climate.koppen_at(d, planet.heights[i]).map_color()
            }
        });
        write_map(&path("rain"), |d| {
            let i = planet.grid.cell_at(d);
            let (_, p) = climate.months_at(d, 0.);
            let p: f32 = p.iter().sum();
            let t = (p / 3000.).clamp(0., 1.).sqrt();
            let c = [0.95 - 0.85 * t, 0.9 - 0.5 * t, 0.7 + 0.25 * t];
            if planet.sea[i] { c.map(|v| v * 0.6) } else { c }
        });
        for (name, month) in [("temp_jan", 0), ("temp_jul", 6)] {
            write_map(&path(name), |d| {
                let i = planet.grid.cell_at(d);
                let (t, _) = climate.months_at(d, planet.heights[i].max(0.));
                let x = ((t[month] + 40.) / 75.).clamp(0., 1.);
                let c = [x, 1. - (2. * x - 1.).abs() * 0.8, 1. - x];
                if planet.sea[i] { c.map(|v| v * 0.6) } else { c }
            });
        }
    }
}

fn write_map(path: &Path, colour: impl Fn(Vec3) -> [f32; 3]) {
    let (width, height) = (1024usize, 512usize);
    let mut image = vec![[0u8; 3]; width * height];
    for y in 0..height {
        let lat = std::f32::consts::FRAC_PI_2 - (y as f32 + 0.5) / height as f32 * std::f32::consts::PI;
        for x in 0..width {
            let lon = (x as f32 + 0.5) / width as f32 * std::f32::consts::TAU - std::f32::consts::PI;
            let d = Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
            image[y * width + x] = colour(d).map(|v| (v.clamp(0., 1.) * 255.) as u8);
        }
    }
    let file = BufWriter::new(File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(image.as_flattened()).unwrap();
}
