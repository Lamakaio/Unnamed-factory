//! Probe a lake's outlet: along each river leaving it, the lake mask, ground and water levels.
//!
//! `cargo run -p worldgen --release --example outlet_probe -- <seed> <lake>`

use bevy_math::{Vec2, Vec3Swizzles};
use worldgen::{WorldgenSettings, generate};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (seed, lake): (u64, usize) = (args[0].parse().unwrap(), args[1].parse().unwrap());
    let c = generate(&WorldgenSettings { seed, ..Default::default() });
    let level = c.lakes[lake].level;
    println!("lake {lake}: level {level:.3}, fine cell {}", c.surface.cell);
    for (index, river) in c.rivers.iter().enumerate() {
        let Some(first) = river.points.first() else { continue };
        if c.lake_mask.at(first.position.xz()) != Some(lake) {
            continue;
        }
        println!("river {index} ({:?}):", river.end);
        let leave = river.points.iter().position(|p| c.lake_mask.at(p.position.xz()) != Some(lake)).unwrap_or(0);
        for (k, p) in river.points.iter().enumerate().skip(leave.saturating_sub(8)).take(24) {
            let q = p.position.xz();
            // Ground across the channel (left to right, in widths).
            let along = river.points.get(k + 1).map_or(q - river.points[k - 1].position.xz(), |n| n.position.xz() - q).normalize_or_zero();
            let across: Vec<String> = [-3., -2., -1., -0.5, 0., 0.5, 1., 2., 3.]
                .iter()
                .map(|s| {
                    let r = q + along.perp() * s * p.width.max(1.);
                    let m = if c.lake_mask.at(r) == Some(lake) { "L" } else { " " };
                    format!("{m}{:+.2}", c.surface.bilinear(r) - level)
                })
                .collect();
            println!(
                "  {k:3} {:?} w{:.1} water{:+.3} mask {} | ground-level across: {}",
                (q * 10.).round() / 10.,
                p.width,
                p.position.y - level,
                if c.lake_mask.at(q) == Some(lake) { "L" } else { "-" },
                across.join(" ")
            );
        }
    }
    let _ = Vec2::ZERO;
}
