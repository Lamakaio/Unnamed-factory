//! Junction diagnostics: tributaries whose join point is far from their parent's course.
//!
//! `cargo run -p worldgen --release --example junctions_check -- [seed...]`

use bevy_math::Vec3Swizzles;
use worldgen::{RiverEnd, WorldgenSettings, generate};

fn main() {
    let seeds: Vec<u64> = std::env::args().skip(1).map(|s| s.parse().unwrap()).collect();
    // `DUMP=river,river...`: print those tributaries' ends and their parent nearby.
    let dump: Vec<usize> = std::env::var("DUMP").map_or(Vec::new(), |d| d.split(',').map(|s| s.parse().unwrap()).collect());
    for seed in seeds {
        let c = generate(&WorldgenSettings { seed, ..Default::default() });
        println!("seed {seed}: {} rivers", c.rivers.len());
        for (index, river) in c.rivers.iter().enumerate() {
            let RiverEnd::Junction(parent) = river.end else { continue };
            let Some(end) = river.points.last() else { continue };
            let parent = &c.rivers[parent];
            let distance = parent
                .points
                .iter()
                .map(|p| p.position.xz().distance(end.position.xz()))
                .fold(f32::MAX, f32::min);
            let half = parent.points.iter().map(|p| p.width).fold(0., f32::max) / 2.;
            if dump.contains(&index) {
                let fmt = |p: &worldgen::RiverPoint| format!("({:.1},{:.1} y{:.2} w{:.1})", p.position.x, p.position.z, p.position.y, p.width);
                let n = river.points.len();
                println!("  river {index} -> {:?}, end distance {distance:.2}, parent half width {half:.1}", river.end);
                println!("    trib: {}", river.points[n.saturating_sub(10)..].iter().map(fmt).collect::<Vec<_>>().join(" "));
                let near: Vec<String> = parent.points.iter().filter(|p| p.position.xz().distance(end.position.xz()) < 4.).map(fmt).collect();
                println!("    parent near: {} (parent {} points, ends {:?})", near.join(" "), parent.points.len(), parent.end);
            }
            if distance > half + 1. {
                println!(
                    "  river {index} ({} points) joins {:?} at {:?}, {distance:.1} from it (parent: {} points, ends {:?} at {:?}, ground {:.2})",
                    river.points.len(),
                    river.end,
                    end.position.xz(),
                    parent.points.len(),
                    parent.end,
                    parent.points.last().map(|p| p.position.xz()),
                    c.surface.bilinear(end.position.xz()),
                );
                let fmt = |p: &worldgen::RiverPoint| format!("({:.1},{:.1} y{:.2} w{:.1} g{:.2})", p.position.x, p.position.z, p.position.y, p.width, c.surface.bilinear(p.position.xz()));
                println!("    trib: {}", river.points.iter().map(fmt).collect::<Vec<_>>().join(" "));
                let n = parent.points.len();
                println!("    parent tail: {}", parent.points[n.saturating_sub(8)..].iter().map(fmt).collect::<Vec<_>>().join(" "));
            }
        }
    }
}
