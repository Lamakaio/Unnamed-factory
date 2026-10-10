//! Statistics of river water-line slopes and depths (for water rendering: rapids, falls),
//! plus the start point, wind and a few shallow sea spots (camera targets for lab shots).
//!
//! `cargo run -p worldgen --release --example river_slopes -- [seed]`

use bevy_math::Vec3Swizzles;
use worldgen::{WorldgenSettings, generate};

fn main() {
    let seed = std::env::args().nth(1).map_or(1082, |s| s.parse().unwrap());
    let continent = generate(&WorldgenSettings {
        seed,
        ..Default::default()
    });
    println!("start {:?} wind {:.0} deg", continent.start_point(), continent.wind_angle.to_degrees());
    // Coast cells (sea next to land), every so often.
    let size = continent.size();
    let mut k = 0;
    for z in (8..size - 8).step_by(37) {
        for x in (8..size - 8).step_by(37) {
            let i = z * size + x;
            if continent.heights[i] <= 0. && continent.heights[i] > -1.5 {
                k += 1;
                if k < 25 { println!("  shallow sea at {:?}", continent.cell_to_world(x, z)); }
            }
        }
    }
    // Sharpest bends: radius of curvature relative to the channel width.
    let mut tight = 0;
    let mut worst = f32::MAX;
    for river in &continent.rivers {
        for w in river.points.windows(3) {
            let (a, b, c) = (w[0].position.xz(), w[1].position.xz(), w[2].position.xz());
            let (ab, bc) = (b - a, c - b);
            if ab.length() < 1e-4 || bc.length() < 1e-4 { continue; }
            let turn = ab.normalize().dot(bc.normalize()).clamp(-1., 1.).acos();
            let radius = (ab.length() + bc.length()) / 2. / turn.max(1e-4) / w[1].width;
            worst = worst.min(radius);
            if radius < 1.0 {
                tight += 1;
                if radius < 0.5 {
                    let k = river.points.iter().position(|p| p.position == w[1].position).unwrap();
                    println!("  tight {radius:.2} at {k}/{} {:?} end {:?}", river.points.len(), w[1].position, river.end);
                }
            }
        }
    }
    println!("bends tighter than 1 width: {tight}, tightest {worst:.2} widths");
    for river in continent.rivers.iter().filter(|r| matches!(r.end, worldgen::RiverEnd::Junction(_))).take(12) {
        let end = river.points.last().unwrap();
        println!("  junction at {:.1} {:.1} (width {:.2})", end.position.x, end.position.z, end.width);
    }
    for (river, mouth) in continent.mouths.iter().take(12) {
        let end = continent.rivers[*river].points.last().unwrap();
        println!("  mouth {mouth:?} at {:.1} {:.1} (width {:.2})", end.position.x, end.position.z, end.width);
    }
    let mut slopes = Vec::new();
    let mut steep = Vec::new();
    for (r, river) in continent.rivers.iter().enumerate() {
        for pair in river.points.windows(2) {
            let run = (pair[1].position - pair[0].position).xz().length().max(1e-4);
            let drop = pair[0].position.y - pair[1].position.y;
            let slope = drop / run;
            slopes.push(slope);
            if slope > 0.3 {
                steep.push((r, pair[0].position, slope, run, drop, pair[0].width));
            }
        }
    }
    slopes.sort_by(f32::total_cmp);
    let n = slopes.len();
    println!("{n} segments, {} rivers", continent.rivers.len());
    for q in [0.0, 0.5, 0.75, 0.9, 0.95, 0.99, 0.999, 1.0] {
        println!("  q{q}: {:.3}", slopes[((n - 1) as f32 * q) as usize]);
    }
    let spacing: f32 = continent.rivers.iter().flat_map(|r| r.points.windows(2))
        .map(|p| (p[1].position - p[0].position).xz().length()).sum::<f32>() / n as f32;
    println!("  mean point spacing {spacing:.3}");
    let mut depths: Vec<f32> = continent.rivers.iter().flat_map(|r| r.points.iter().map(|p| p.depth)).collect();
    depths.sort_by(f32::total_cmp);
    let mut widths: Vec<f32> = continent.rivers.iter().flat_map(|r| r.points.iter().map(|p| p.width)).collect();
    widths.sort_by(f32::total_cmp);
    for q in [0.0, 0.1, 0.5, 0.9, 1.0] {
        println!("  depth q{q}: {:.3} width {:.3}", depths[((depths.len() - 1) as f32 * q) as usize], widths[((widths.len() - 1) as f32 * q) as usize]);
    }
    println!("{} segments steeper than 0.3:", steep.len());
    for s in steep.iter().take(30) {
        println!("  river {} at {:.1} slope {:.2} run {:.2} drop {:.2} width {:.2}", s.0, s.1, s.2, s.3, s.4, s.5);
    }
}
