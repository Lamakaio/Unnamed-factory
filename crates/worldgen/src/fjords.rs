//! Fjords: at cold, mountainous coasts, the ice ages' glaciers carved the coastal valleys into
//! deep U-shaped troughs that the sea flooded when the ice melted. Here the larger valleys
//! reaching the sea poleward of `FJORD_LATITUDE` with high ground near the coast are carved
//! along their course (traced up the drainage): below sea level, deepest a little inland, a
//! shallower sill at the mouth, rising to the shore at the head. The sea floods them; rivers
//! end at their heads.

use crate::{
    flow::{FlowGraph, priority_flood},
    grid::Grid,
    noise::{hash, smoothstep},
    sphere::SphereGrid,
};

/// Fjords grow from this latitude (degrees), fully from `FJORD_LATITUDE.1`.
const FJORD_LATITUDE: (f32, f32) = (48., 58.);
/// A valley is a fjord from this drainage (mean cell areas), with ground this high (world
/// units) within `RELIEF_RADIUS` cells of its mouth, and mouths at least `SPACING` cells apart.
const MIN_DRAINAGE: f32 = 150.;
const MIN_RELIEF: f32 = 6.;
const RELIEF_RADIUS: f32 = 25.;
const SPACING: f32 = 18.;
/// Length (cells), depth (world units) and half-width (cells) of a fjord.
const LENGTH: (f32, f32) = (25., 60.);
const DEPTH: f32 = 3.5;
const HALF_WIDTH: (f32, f32) = (1.8, 3.2);

/// Carve the fjords into the heights (world units) and flood them (`sea`).
pub fn carve(grid: &SphereGrid, radius: f32, seed: u64, heights: &mut [f32], sea: &mut [bool]) {
    let cells = grid.len();
    let cold = |i: usize| {
        let latitude = grid.direction(i).y.clamp(-1., 1.).asin().to_degrees().abs();
        smoothstep(FJORD_LATITUDE.0, FJORD_LATITUDE.1, latitude)
    };
    // Valleys: the drainage on the (filled) land.
    let filled = priority_flood(grid, heights, sea, 1e-4);
    let graph = FlowGraph::new(grid, &filled, sea);
    let drainage = graph.accumulate(|i| grid.area(i));
    let is_mouth = crate::par::map(cells, |i| {
        !sea[i]
            && drainage[i] >= MIN_DRAINAGE
            && sea[graph.receivers[i]]
            && cold(i) > hash(seed as u32 ^ 0xF10D, i as u32, 0) as f32 / u32::MAX as f32
    });
    let mut mouths: Vec<usize> = (0..cells).filter(|&i| is_mouth[i]).collect();
    mouths.sort_by(|&a, &b| drainage[b].total_cmp(&drainage[a]));
    // The highest ground near each mouth (mountains behind the coast), before any carving (it
    // only lowers valleys).
    let relief = crate::par::map(mouths.len(), |k| {
        let mut highest = f32::MIN;
        grid.for_cells_within(grid.direction(mouths[k]), RELIEF_RADIUS / radius, |i, _| highest = highest.max(heights[i]));
        highest
    });
    let mut chosen: Vec<usize> = Vec::new();
    let mut carved = vec![false; cells];
    for (k, &mouth) in mouths.iter().enumerate() {
        let d = grid.direction(mouth);
        if chosen.iter().any(|&m| grid.direction(m).distance(d) * radius < SPACING) || relief[k] < MIN_RELIEF {
            continue;
        }
        chosen.push(mouth);
        let roll = |k: u32| hash(seed as u32 ^ 0xF10D, mouth as u32, k) as f32 / u32::MAX as f32;
        let length = LENGTH.0 + (LENGTH.1 - LENGTH.0) * roll(1);
        let half = HALF_WIDTH.0 + (HALF_WIDTH.1 - HALF_WIDTH.0) * roll(2);
        // Up the valley, the main branch (most drainage) each time.
        let mut path = vec![mouth];
        let mut along = 0.;
        while along < length {
            let here = *path.last().unwrap();
            // (The neighbours draining here, the largest.)
            let Some(up) = grid
                .neighbours(here)
                .map(|(j, _)| j)
                .filter(|&j| graph.receivers[j] == here && !sea[j])
                .max_by(|&a, &b| drainage[a].total_cmp(&drainage[b]))
            else {
                break;
            };
            along += grid.direction(here).distance(grid.direction(up)) * radius;
            path.push(up);
        }
        let total = along.max(1.);
        // The trough: U-shaped across, deepest a little inland, a sill at the mouth, rising to
        // the head.
        let mut along = 0.;
        for (k, &c) in path.iter().enumerate() {
            if k > 0 {
                along += grid.direction(c).distance(grid.direction(path[k - 1])) * radius;
            }
            let t = along / total;
            let depth = DEPTH * smoothstep(0., 0.15, t) * (0.6 + 0.4 * smoothstep(0.0, 0.25, t)) * (1. - t * t)
                + 0.4 * (1. - t);
            grid.for_cells_within(grid.direction(c), half / radius, |i, angle| {
                let r = angle * radius;
                let floor = -depth * (1. - (r / half).powi(2)) + 0.05;
                if floor < heights[i] {
                    heights[i] = floor;
                    carved[i] = true;
                }
            });
        }
    }
    // The sea floods the troughs: from their cells next to the sea, through the carved cells
    // below its level.
    let mut queue: std::collections::VecDeque<usize> = (0..cells)
        .filter(|&i| carved[i] && heights[i] < 0. && grid.neighbours(i).any(|(j, _)| sea[j]))
        .collect();
    while let Some(i) = queue.pop_front() {
        if sea[i] {
            continue;
        }
        sea[i] = true;
        for (j, _) in grid.neighbours(i) {
            if !sea[j] && heights[j] < 0. && carved[j] {
                queue.push_back(j);
            }
        }
    }
}
