//! Barrier islands: on low coasts with a gentle, shallow shelf, waves pile the sand into long
//! narrow islands a little offshore, parallel to the coast, with a shallow lagoon behind them
//! and tidal inlets breaking them up. Here, in stretches along such coasts (outside the reefs'
//! tropics and the polar seas): a sand bar at `BAR_DISTANCE` from the shore rising just above
//! the water, gaps where a noise is low (inlets), and the sea floor behind it raised into a
//! shallow lagoon.

use std::collections::VecDeque;

use crate::{
    grid::Grid,
    noise::{Noise, smoothstep},
    sphere::SphereGrid,
};

/// The bar's distance from the shore (cells, varying along the coast), its half-width (cells)
/// and its crest (world units above the sea).
const BAR_DISTANCE: (f32, f32) = (3.5, 6.);
const BAR_HALF_WIDTH: f32 = 1.1;
const BAR_CREST: f32 = 0.35;
/// Slopes (world units per cell) of the bar's seaward shoreface and of its back.
const SHOREFACE: f32 = 0.45;
const BACK: f32 = 0.8;
/// The lagoon behind it (world units under the sea).
const LAGOON_FLOOR: f32 = -0.35;
/// Only where the sea at the bar is shallower than this, and the land near the shore lower
/// than `LOW_COAST` (world units) within `LOW_COAST_RADIUS` cells.
const MAX_DEPTH: f32 = 2.5;
const LOW_COAST: f32 = 2.;
const LOW_COAST_RADIUS: f32 = 6.;
/// Between these latitudes (degrees): not in the reefs' tropics, nor in the polar seas.
const LATITUDE: (f32, f32) = (24., 60.);
/// Share of the eligible coasts with barrier islands (a noise over `STRETCH` world units), and
/// how often inlets break them (higher: more).
const STRETCH: f32 = 70.;
const COVER: f32 = 0.12;
const INLETS: f32 = 0.5;

/// Raise barrier islands and their lagoons on the heights (world units; cells raised above the
/// sea become land), their sand from the mainland's rock (not the sea floor's basalt).
/// `reef`: the coral reefs (none where they grow).
pub fn build(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    reef: &[f32],
    heights: &mut [f32],
    sea: &mut [bool],
    rock: &mut [u8],
) {
    let raised = raise(grid, radius, seed, reef, heights, sea);
    for (i, height, shore) in raised {
        heights[i] = height;
        sea[i] = height <= 0.;
        rock[i] = rock[shore];
    }
}

/// The new heights of the cells the barriers raise, and their nearest shore cell.
fn raise(grid: &SphereGrid, radius: f32, seed: u64, reef: &[f32], heights: &[f32], sea: &[bool]) -> Vec<(usize, f32, usize)> {
    let cells = grid.len();
    let noise = Noise::new(seed as u32 ^ 0xBA22);
    // Distance from the land over the sea (cells), and the nearest shore cell.
    let mut distance = vec![f32::INFINITY; cells];
    let mut shore = vec![usize::MAX; cells];
    let mut queue: VecDeque<usize> = (0..cells).filter(|&i| !sea[i] && grid.neighbours(i).any(|(j, _)| sea[j])).collect();
    for &i in &queue {
        distance[i] = 0.;
        shore[i] = i;
    }
    let reach = BAR_DISTANCE.1 + BAR_HALF_WIDTH + (BAR_CREST + MAX_DEPTH) / SHOREFACE;
    while let Some(i) = queue.pop_front() {
        for (j, d) in grid.neighbours(i) {
            let next = distance[i] + d;
            if sea[j] && next < distance[j] && next <= reach {
                distance[j] = next;
                shore[j] = shore[i];
                queue.push_back(j);
            }
        }
    }
    // Low coasts: the highest land near each shore cell.
    let low = |i: usize| {
        let centre = grid.direction(i);
        let mut seen = std::collections::HashSet::from([i]);
        let mut queue = VecDeque::from([i]);
        while let Some(c) = queue.pop_front() {
            if heights[c] > LOW_COAST {
                return false;
            }
            for (j, _) in grid.neighbours(c) {
                if !sea[j] && grid.direction(j).distance(centre) * radius <= LOW_COAST_RADIUS && seen.insert(j) {
                    queue.push_back(j);
                }
            }
        }
        true
    };
    let mut low_shore = std::collections::HashMap::new();
    let original = heights;
    let mut raised = Vec::new();
    for i in 0..cells {
        if !sea[i] || !distance[i].is_finite() || reef[i] > 0.01 || original[i] < -MAX_DEPTH * 2. {
            continue;
        }
        let d = grid.direction(i);
        let latitude = d.y.clamp(-1., 1.).asin().to_degrees().abs();
        let band = smoothstep(LATITUDE.0, LATITUDE.0 + 4., latitude) * smoothstep(LATITUDE.1, LATITUDE.1 - 4., latitude);
        let p = d * radius;
        let stretch = smoothstep(0.5 - COVER, 0.6 - COVER, 0.5 + 0.5 * noise.fbm3(p / STRETCH, 2)) * band;
        if stretch <= 0. {
            continue;
        }
        if !*low_shore.entry(shore[i]).or_insert_with(|| low(shore[i])) {
            continue;
        }
        let at = BAR_DISTANCE.0 + (BAR_DISTANCE.1 - BAR_DISTANCE.0) * (0.5 + 0.5 * noise.layer(1).fbm3(p / 25., 2));
        // Shallow enough where the bar stands.
        let dx = distance[i] - at;
        if original[i] < -MAX_DEPTH && dx.abs() < BAR_HALF_WIDTH {
            continue;
        }
        // The bar: a flat top, a gentle shoreface down to the sea, a steeper back to the
        // lagoon; inlets break it.
        let inlet = smoothstep(INLETS, INLETS - 0.12, 0.5 + 0.5 * noise.layer(2).fbm3(p / 14., 2));
        let down = if dx > 0. { SHOREFACE * (dx - BAR_HALF_WIDTH).max(0.) } else { BACK * (-dx - BAR_HALF_WIDTH).max(0.) };
        let top = BAR_CREST * smoothstep(BAR_HALF_WIDTH * 1.4, 0., dx.abs()).sqrt();
        let bar = top - down;
        let lagoon = smoothstep(-BAR_HALF_WIDTH, -BAR_HALF_WIDTH * 2., dx) * stretch;
        let floor = original[i].max(LAGOON_FLOOR) * lagoon + original[i] * (1. - lagoon);
        let weight = stretch * (1. - inlet);
        raised.push((i, floor + (bar - floor).max(0.) * weight, shore[i]));
    }
    raised
}
