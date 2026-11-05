//! A world's deep-time history (`history.rs`): the events, a table, and a chart of the global
//! mean temperature, CO₂ and the ice line through time.
//!
//! `cargo run -p worldgen --release --example history_preview -- <out_dir> [--world <preset>[,name=value...]] [--to <Gyr>] [--seed <seed>]`

use std::{fs::File, io::BufWriter, path::Path};

use worldgen::{
    history::{Event, evolve},
    world::World,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .expect("usage: history_preview <out_dir> [--world <preset>] [--to <Gyr>] [--seed <seed>]");
    std::fs::create_dir_all(&out).unwrap();
    let (mut world, mut to, mut seed) = (World::virgin(), 4.57f32, 1082u64);
    while let Some(arg) = args.next() {
        let value = args.next().expect("a value");
        match arg.as_str() {
            "--world" => world = World::parse(&value).unwrap_or_else(|e| panic!("{e}")),
            "--to" => to = value.parse().unwrap(),
            "--seed" => seed = value.parse().unwrap(),
            _ => panic!("unknown argument {arg}"),
        }
    }
    let start = std::time::Instant::now();
    let history = evolve(&world, to, seed);
    println!(
        "from {:.2} to {to:.2} Gyr in {:.2} s",
        world.age,
        start.elapsed().as_secs_f32()
    );
    // (Only the big ones: there are thousands of impacts in the early bombardment.)
    let (mut impacts, mut eruptions) = (0, 0);
    for event in &history.events {
        match *event {
            Event::Impact { radius, .. } => {
                impacts += 1;
                if radius >= 400. {
                    println!("  {:.3} Gyr: {event:?}", event.age());
                }
            }
            Event::Eruption { strength, .. } => {
                eruptions += 1;
                if strength >= 5.5 {
                    println!("  {:.3} Gyr: {event:?}", event.age());
                }
            }
            _ => println!("  {:.3} Gyr: {event:?}", event.age()),
        }
    }
    println!("  {impacts} impacts over 50 km, {eruptions} large igneous provinces");
    println!("   age    CO2 (ppm)   T (°C)  ice line  outgassing  weathering");
    let every = (history.snapshots.len() / 24).max(1);
    for s in history.snapshots.iter().step_by(every) {
        println!(
            "  {:5.2}  {:10.0}  {:7.1}  {:7.0}°  {:10.2}  {:10.2}",
            s.age, s.co2, s.temperature, s.ice_line, s.outgassing, s.weathering
        );
    }
    chart(&Path::new(&out).join("history.png"), &history);
}

/// Temperature (red, −50..50 °C), log CO₂ (black, 10 ppm..1 bar), ice line (blue, 0..90°)
/// against age; snowballs shaded.
fn chart(path: &Path, history: &worldgen::history::History) {
    let (width, height) = (1200usize, 400usize);
    let mut image = vec![[255u8; 3]; width * height];
    let (first, last) = (
        history.snapshots[0].age,
        history.snapshots.last().unwrap().age,
    );
    let x_of = |age: f32| ((age - first) / (last - first).max(1e-6) * (width - 1) as f32) as usize;
    // Snowballs shaded.
    let mut frozen_since = None;
    for event in &history.events {
        match *event {
            Event::Snowball { age } => frozen_since = Some(age),
            Event::Thaw { age } => {
                if let Some(from) = frozen_since.take() {
                    for x in x_of(from)..=x_of(age).min(width - 1) {
                        for y in 0..height {
                            image[y * width + x] = [215, 230, 250];
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut plot = |value: &dyn Fn(&worldgen::history::Snapshot) -> f32, color: [u8; 3]| {
        let mut previous: Option<(usize, usize)> = None;
        for s in &history.snapshots {
            let x = x_of(s.age).min(width - 1);
            let y = ((1. - value(s).clamp(0., 1.)) * (height - 1) as f32) as usize;
            if let Some((px, py)) = previous {
                let steps = (x - px).max(py.abs_diff(y)).max(1);
                for k in 0..=steps {
                    let f = k as f32 / steps as f32;
                    let xx = px + ((x - px) as f32 * f) as usize;
                    let yy = (py as f32 + (y as f32 - py as f32) * f) as usize;
                    image[yy.min(height - 1) * width + xx] = color;
                }
            }
            previous = Some((x, y));
        }
    };
    plot(&|s| (s.temperature + 50.) / 100., [200, 30, 30]);
    plot(&|s| (s.co2.log10() - 1.) / 5., [20, 20, 20]);
    plot(&|s| s.ice_line / 90., [40, 90, 220]);
    let file = BufWriter::new(File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(image.as_flattened())
        .unwrap();
}
