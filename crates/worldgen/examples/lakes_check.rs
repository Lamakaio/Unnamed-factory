//! Lake diagnostics: rim leaks, rivers through lakes, outlets.
//!
//! `cargo run -p worldgen --release --example lakes_check -- [seed]`

use bevy_math::{Vec2, Vec3Swizzles};
use worldgen::{RiverEnd, WorldgenSettings, generate};

fn main() {
    let seed = std::env::args().nth(1).map_or(1082, |s| s.parse().unwrap());
    let c = generate(&WorldgenSettings { seed, ..Default::default() });
    let size = c.size();
    for r in [6usize, 12, 1] {
        println!("river {r} end {:?}", c.rivers[r].end);
        for p in &c.rivers[r].points {
            let cell = c.world_to_cell(p.position.xz()).unwrap();
            let i = cell.1 * size + cell.0;
            if (p.position - c.rivers[r].points[0].position).length() > 6. && (p.position - c.rivers[r].points.last().unwrap().position).length() > 3. { continue; }
            println!("r{r} {:.1} {:.1} water {:.2} heights {:.2} surface {:.2} lake {:?}", p.position.x, p.position.z, p.position.y, c.heights[i], c.surface.bilinear(p.position.xz()), c.lake_of[i]);
        }
    }
    for (l, lake) in c.lakes.iter().enumerate() {
        if lake.cells.len() < 200 { continue; }
        let center = lake.cells.iter().map(|p| p.as_vec2()).sum::<Vec2>() / lake.cells.len() as f32;
        // Rim: non-lake neighbours; lowest fine surface on the rim.
        let mut lowest = (f32::MAX, Vec2::ZERO);
        let mut low_count = 0;
        for cell in &lake.cells {
            for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let (x, z) = (cell.x as i32 + dx, cell.y as i32 + dz);
                if x < 0 || z < 0 || x >= size as i32 || z >= size as i32 { continue; }
                let i = z as usize * size + x as usize;
                if c.lake_of[i] == Some(l as u16) { continue; }
                let w = c.cell_to_world(x as usize, z as usize);
                let h = c.surface.bilinear(w);
                if h < lake.level { low_count += 1; }
                if h < lowest.0 { lowest = (h, w); }
            }
        }
        println!("lake {l}: {} cells, level {:.2}, center {:?}, rim min {:.2} at {:?}, {} rim cells below level",
            lake.cells.len(), lake.level, c.cell_to_world(center.x as usize, center.y as usize), lowest.0, lowest.1, low_count);
        // Rivers starting next to the lake (outlets).
        for (r, river) in c.rivers.iter().enumerate() {
            let first = river.points[0].position.xz();
            let near = lake.cells.iter().any(|cell| c.cell_to_world(cell.x as usize, cell.y as usize).distance(first) < 3.);
            if near { println!("  outlet river {r} from {:.1} {:.1} y {:.2}", first.x, first.y, river.points[0].position.y); }
        }
        // Rivers ending here, starting in/near it, passing through it.
        for (r, river) in c.rivers.iter().enumerate() {
            let inside = river.points.iter().filter(|p| c.world_to_cell(p.position.xz())
                .is_some_and(|(x, z)| c.lake_of[z * size + x] == Some(l as u16))).count();
            if inside == 0 && river.end != RiverEnd::Lake(l) { continue; }
            let first = river.points[0].position;
            let last = river.points.last().unwrap().position;
            println!("  river {r}: {} pts, {inside} inside, end {:?}, first {:.1} {:.1} y {:.2}, last {:.1} {:.1} y {:.2}",
                river.points.len(), river.end, first.x, first.z, first.y, last.x, last.z, last.y);
        }
    }
}
