//! Lakes: valley sites dammed by a natural sill (like a moraine), and the final lake extraction
//! with Priority-Flood.

use std::collections::VecDeque;

use bevy_math::{UVec2, Vec2};
use rand::{Rng, RngExt};

use crate::{
    Lake, WorldgenSettings,
    flow::{FlowGraph, neighbours, priority_flood},
    noise::smoothstep,
};

/// Carve lakes into `heights` (world units): pick valley sites, flood them up to a level, dam
/// the outflow at that level and deepen the bed.
pub fn carve_lakes(
    settings: &WorldgenSettings,
    heights: &mut [f32],
    base_level: &[bool],
    land: &[f32],
    rng: &mut impl Rng,
) {
    let size = settings.size;
    let graph = FlowGraph::new(size, heights, base_level);
    let area = graph.accumulate(|_| 1.);

    let mut candidates: Vec<usize> = (0..heights.len())
        .filter(|&i| {
            land[i] > 0.7
                && heights[i] > 2.
                && area[i] >= settings.lake_min_area
                && area[i] <= settings.lake_min_area * 40.
        })
        .collect();
    // Shuffle (Fisher-Yates) so sites spread over the continent.
    for i in (1..candidates.len()).rev() {
        candidates.swap(i, rng.random_range(0..=i));
    }

    let mut sites: Vec<Vec2> = Vec::new();
    for site in candidates {
        if sites.len() >= settings.lake_count {
            break;
        }
        let position = cell_position(size, site);
        if sites
            .iter()
            .any(|other| other.distance(position) < settings.lake_spacing)
        {
            continue;
        }
        let dam_distance = rng.random_range(20..40usize);
        let dam_height = rng.random_range(2.0..6.0f32);
        let max_cells = rng.random_range(150..2000usize);
        if carve_lake(
            size,
            heights,
            base_level,
            &graph.receivers,
            site,
            dam_distance,
            dam_height,
            max_cells,
        ) {
            sites.push(position);
        }
    }
}

/// Try to make a lake upstream of a natural dam below `site`; returns whether it worked.
///
/// The dam sits `dam_distance` cells downstream of the site; a disc around it is raised to the
/// lake level (growing until it seals the valley), then the valley is flooded upstream of it.
#[expect(clippy::too_many_arguments, reason = "internal helper")]
fn carve_lake(
    size: usize,
    heights: &mut [f32],
    base_level: &[bool],
    receivers: &[usize],
    site: usize,
    dam_distance: usize,
    dam_height: f32,
    max_cells: usize,
) -> bool {
    // Walk downstream to the dam.
    let mut path = vec![site];
    for _ in 0..dam_distance {
        let current = *path.last().unwrap();
        let next = receivers[current];
        if next == current || base_level[next] {
            return false;
        }
        path.push(next);
    }
    let dam = *path.last().unwrap();
    let level = heights[dam] + dam_height;
    let crest = level + 0.5;
    let dam_position = cell_position(size, dam);

    for radius in [4., 6., 9., 13., 18.] {
        let mut dammed = heights.to_vec();
        let reach = radius as usize + 1;
        let (dx, dz) = (dam % size, dam / size);
        for z in dz.saturating_sub(reach)..(dz + reach + 1).min(size) {
            for x in dx.saturating_sub(reach)..(dx + reach + 1).min(size) {
                let i = z * size + x;
                // Moraine-like profile: flat crest in the middle, smooth flanks.
                let d = cell_position(size, i).distance(dam_position);
                let raise = 1. - smoothstep(radius * 0.5, radius, d);
                dammed[i] = dammed[i].max(dammed[i] + (crest - dammed[i]) * raise);
            }
        }
        // Flood from the first valley cell upstream of the dam.
        let Some(&seed) = path
            .iter()
            .rev()
            .find(|&&i| cell_position(size, i).distance(dam_position) > radius + 1.)
        else {
            return false;
        };
        if dammed[seed] >= level {
            // The valley is already above the lake level there: too steep for this dam.
            return false;
        }
        let Some(region) = flood_region(size, &dammed, base_level, seed, level, max_cells) else {
            continue;
        };
        if region.len() < 30 {
            return false;
        }
        heights.copy_from_slice(&dammed);
        let mut in_region = vec![false; heights.len()];
        for &i in &region {
            in_region[i] = true;
        }
        // Deepen the bed towards the middle (distance from the shore, in cells).
        let distance = distance_from_shore(size, &region, &in_region);
        let max_distance = distance.iter().copied().fold(1., f32::max);
        for (&i, &d) in region.iter().zip(&distance) {
            let bed = level - 0.6 - dam_height * 0.8 * (d / max_distance).sqrt();
            heights[i] = heights[i].min(bed);
        }
        return true;
    }
    false
}

/// Cells connected to `site` lower than `level`, or `None` if that region is too big or
/// reaches the sea.
fn flood_region(
    size: usize,
    heights: &[f32],
    base_level: &[bool],
    site: usize,
    level: f32,
    max_cells: usize,
) -> Option<Vec<usize>> {
    let mut visited = vec![false; heights.len()];
    let mut region = vec![site];
    let mut queue = VecDeque::from([site]);
    visited[site] = true;
    while let Some(i) = queue.pop_front() {
        for (n, _) in neighbours(size, i) {
            if visited[n] || heights[n] >= level {
                continue;
            }
            if base_level[n] || region.len() >= max_cells {
                return None;
            }
            visited[n] = true;
            region.push(n);
            queue.push_back(n);
        }
    }
    Some(region)
}

/// Distance (in cells, 4-connected) of each region cell to the nearest cell outside.
fn distance_from_shore(size: usize, region: &[usize], in_region: &[bool]) -> Vec<f32> {
    let mut distance = vec![f32::MAX; in_region.len()];
    let mut queue = VecDeque::new();
    for &i in region {
        if neighbours(size, i).any(|(n, _)| !in_region[n]) {
            distance[i] = 1.;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        for (n, d) in neighbours(size, i) {
            if in_region[n] && d <= 1. && distance[n] > distance[i] + 1. {
                distance[n] = distance[i] + 1.;
                queue.push_back(n);
            }
        }
    }
    region.iter().map(|&i| distance[i]).collect()
}

/// Find the lakes of the final terrain: Priority-Flood without slope gives flat water surfaces;
/// connected flooded areas big and deep enough are lakes. Returns the lakes and each cell's
/// lake index.
pub fn find_lakes(
    size: usize,
    heights: &[f32],
    base_level: &[bool],
    min_cells: usize,
) -> (Vec<Lake>, Vec<Option<u16>>) {
    let water = priority_flood(size, heights, base_level, 0.);
    let flooded: Vec<bool> = (0..heights.len())
        .map(|i| !base_level[i] && water[i] - heights[i] > 0.1)
        .collect();
    let mut lake_of = vec![None; heights.len()];
    let mut lakes = Vec::new();
    let mut seen = vec![false; heights.len()];
    for start in 0..heights.len() {
        if !flooded[start] || seen[start] {
            continue;
        }
        let mut cells = vec![start];
        seen[start] = true;
        let mut k = 0;
        while k < cells.len() {
            let i = cells[k];
            k += 1;
            for (n, _) in neighbours(size, i) {
                if flooded[n] && !seen[n] {
                    seen[n] = true;
                    cells.push(n);
                }
            }
        }
        if cells.len() < min_cells {
            continue;
        }
        let id = lakes.len() as u16;
        for &i in &cells {
            lake_of[i] = Some(id);
        }
        lakes.push(Lake {
            level: water[start],
            cells: cells
                .iter()
                .map(|&i| UVec2::new((i % size) as u32, (i / size) as u32))
                .collect(),
        });
    }
    (lakes, lake_of)
}

fn cell_position(size: usize, i: usize) -> Vec2 {
    Vec2::new((i % size) as f32, (i / size) as f32)
}
