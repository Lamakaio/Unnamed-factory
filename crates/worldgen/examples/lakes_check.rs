//! Lake diagnostics: each lake's size, depth, setting and outlet, and whether its rim holds
//! (on the coarse heights and the fine surface).
//!
//! `cargo run -p worldgen --release --example lakes_check -- [seed...]`

use bevy_math::{Vec2, Vec3Swizzles};
use worldgen::{RiverEnd, WorldgenSettings, generate};

fn main() {
    let seeds: Vec<u64> = std::env::args().skip(1).map(|s| s.parse().unwrap()).collect();
    let seeds = if seeds.is_empty() { vec![1082] } else { seeds };
    for seed in seeds {
        let c = generate(&WorldgenSettings {
            seed,
            ..Default::default()
        });
        let size = c.size();
        let salt = c.salt_flat.iter().filter(|&&s| s).count();
        println!("seed {seed}: {} lakes, {salt} salt flat cells", c.lakes.len());
        for (l, lake) in c.lakes.iter().enumerate() {
            let center = lake.cells.iter().map(|p| p.as_vec2()).sum::<Vec2>() / lake.cells.len() as f32;
            let i = center.y as usize * size + center.x as usize;
            let deepest = lake
                .cells
                .iter()
                .map(|cell| lake.level - c.heights[cell.y as usize * size + cell.x as usize])
                .fold(0., f32::max);
            // Rim on the fine surface: lowest bank point vs level.
            let mut rim = f32::MAX;
            for cell in &lake.cells {
                for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                    let (x, z) = (cell.x as i32 + dx, cell.y as i32 + dz);
                    if x < 0 || z < 0 || x >= size as i32 || z >= size as i32 {
                        continue;
                    }
                    if c.lake_of[z as usize * size + x as usize] == Some(l as u16) {
                        continue;
                    }
                    rim = rim.min(c.surface.bilinear(c.cell_to_world(x as usize, z as usize)));
                }
            }
            let inflows = c.rivers.iter().filter(|r| r.end == RiverEnd::Lake(l)).count();
            let outlet = c.rivers.iter().any(|r| {
                lake.cells.iter().any(|cell| {
                    c.cell_to_world(cell.x as usize, cell.y as usize)
                        .distance(r.points[0].position.xz())
                        < 2.5
                }) && r.end != RiverEnd::Lake(l)
            });
            // The outlet's levels around where it leaves the lake.
            for r in c.rivers.iter().filter(|r| r.end != RiverEnd::Lake(l)) {
                let inside: Vec<bool> = r.points.iter().map(|p| c.lake_over(p.position.xz())
                    .is_some_and(|level| (level - lake.level).abs() < 0.02)).collect();
                if !inside[0] { continue; }
                let leave = inside.iter().position(|&i| !i).unwrap_or(0);
                let ys: Vec<String> = r.points[leave.saturating_sub(2)..(leave + 8).min(r.points.len())]
                    .iter().map(|p| format!("{:.2}", p.position.y - lake.level)).collect();
                println!("    lake {l} outlet leaves at {:?}: y - level {}", r.points[leave].position.xz(), ys.join(" "));
            }
            println!(
                "  lake {l}: {:4} cells at {:?}, level {:5.2}, depth {:4.2}, rim {:+.2}, {:?}{}, {inflows} in, outlet {outlet}",
                lake.cells.len(),
                c.cell_to_world(center.x as usize, center.y as usize),
                lake.level,
                deepest,
                rim - lake.level,
                c.landform[i],
                if lake.endorheic { " ENDORHEIC" } else { "" },
            );
        }
    }
}
