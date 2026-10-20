//! The planet's sea wave field, as on the flat map (`map::sea` in the game): per cell, the
//! swell's travel time to the shore (waves slow down in shallow water, so fronts bend around
//! headlands into bays and bunch up near the shore), the water depth, the exposure (how much
//! the waves reaching a cell come with the wind, and how strong that wind is: the climate's
//! storm belts send big swell, the calm belts little) and the wind speed.
//!
//! Travel times come from a Dijkstra search on the sphere's grid from the shore, like the fast
//! marching solution of the eikonal equation for wave fronts (Sethian; see CREDITS.md),
//! stopped at [`MAX_TRAVEL`]: only the coastal band matters.

use std::{cmp::Ordering, collections::BinaryHeap};

use bevy_math::{Vec2, Vec3};

use crate::{grid::Grid, noise::smoothstep, par, planet::Planet, surface::PlanetSurface};

/// Waves reach full (deep-water) speed in water this deep (world units).
const DEEP: f32 = 2.;
/// Slowest wave speed, relative to deep water.
const MIN_SPEED: f32 = 0.2;
/// Travel times beyond this are stored as this (the shader fades shore waves out before it).
pub const MAX_TRAVEL: f32 = 60.;
const MAX_DEPTH: f32 = 30.;
/// Openness (share of sea around, from a blur of this many passes) from which swell reaches a
/// cell: a straight coast is about 0.5 open, a narrow channel much less.
const OPEN_RADIUS: usize = 6;
const OPEN_WATER: (f32, f32) = (0.3, 0.55);
/// Wind strengths (mean speed, m/s) from calm seas to full swell.
const SWELL_WIND: (f32, f32) = (4., 11.);

/// The wave field of every cell of the planet's grid: (travel time, depth, exposure, wind
/// speed). Land below sea level not open to the sea (dry basins) has depth -1: no sea there.
/// (Shores dipping under the water line along the coast stay sea.)
pub fn sea_field(planet: &Planet, surface: &PlanetSurface) -> Vec<[f32; 4]> {
    let grid = &planet.grid;
    let cells = grid.len();
    let sea = &planet.sea;
    let depth: Vec<f32> = planet.heights.iter().map(|h| (-h).max(0.)).collect();
    // The sea reaches the land below its level connected to it.
    let mut reached = sea.clone();
    let mut queue: std::collections::VecDeque<usize> = (0..cells).filter(|&i| sea[i]).collect();
    while let Some(i) = queue.pop_front() {
        for (j, _) in grid.neighbours(i) {
            if !reached[j] && planet.heights[j] < 0. {
                reached[j] = true;
                queue.push_back(j);
            }
        }
    }
    let speed: Vec<f32> = depth.iter().map(|d| (d / DEEP).sqrt().clamp(MIN_SPEED, 1.)).collect();

    // Travel time from the shore (land counts as shore).
    let mut travel = vec![MAX_TRAVEL; cells];
    // (With the shore cell each time comes from and the path's length, to undo the grid's
    // metric below.)
    let mut source: Vec<usize> = (0..cells).collect();
    let mut path = vec![0f32; cells];
    let mut heap = BinaryHeap::new();
    for i in 0..cells {
        if !sea[i] || grid.neighbours(i).any(|(j, d)| d < 1.2 && !sea[j]) {
            travel[i] = 0.;
            heap.push(Visit(0., i));
        }
    }
    while let Some(Visit(time, i)) = heap.pop() {
        if time > travel[i] {
            continue;
        }
        for (j, length) in grid.neighbours(i) {
            if !sea[j] {
                continue;
            }
            let t = time + length * 2. / (speed[i] + speed[j]);
            if t < travel[j] {
                travel[j] = t;
                source[j] = source[i];
                path[j] = path[i] + length;
                heap.push(Visit(t, j));
            }
        }
    }
    // Paths on the grid's 8 directions are up to 8 % longer than straight lines, and
    // differently so by direction: fronts come out polygonal (chevrons along straight coasts).
    // Rescale each time by the straight distance to its shore cell over the path's length.
    let cells_per_radian = 2. * grid.n() as f32 / std::f32::consts::PI;
    for i in 0..cells {
        if path[i] > 0. && travel[i] < MAX_TRAVEL {
            let straight = grid.direction(i).angle_between(grid.direction(source[i])) * cells_per_radian;
            travel[i] *= (straight / path[i]).clamp(0.85, 1.);
        }
    }
    // Smooth the grid's artifacts, keeping the shore at 0.
    let mut travel = grid.blur(&travel, 3);
    for (t, &sea) in travel.iter_mut().zip(sea) {
        if !sea {
            *t = 0.;
        }
    }

    // Exposure: waves travel down the travel time gradient, toward the shore; they are big when
    // they come with the wind and the wind is strong.
    let open: Vec<f32> = sea.iter().map(|&s| if s { 1. } else { 0. }).collect();
    let open = grid.blur(&open, OPEN_RADIUS);
    let wind: Vec<(Vec3, f32)> = par::map(cells, |i| {
        if !sea[i] {
            return (Vec3::ZERO, 0.);
        }
        let (face, x, z) = grid.coords(i);
        surface.wind_at(face, Vec2::new(x as f32, z as f32))
    });
    let exposure: Vec<f32> = par::map(cells, |i| {
        if !sea[i] {
            return 0.;
        }
        let here = grid.direction(i);
        let mut gradient = Vec3::ZERO;
        for (j, _) in grid.neighbours(i) {
            let step = grid.direction(j) - here;
            gradient += step * (travel[j] - travel[i]) / step.length_squared().max(1e-12);
        }
        let heading = -gradient.normalize_or_zero();
        let (wind, strength) = wind[i];
        let facing = heading.dot(wind.normalize_or_zero());
        let along = 0.2 + 0.8 * smoothstep(-0.3, 0.7, facing);
        let energy = 0.35 + 0.65 * smoothstep(SWELL_WIND.0, SWELL_WIND.1, strength);
        along * energy * smoothstep(OPEN_WATER.0, OPEN_WATER.1, open[i])
    });
    let exposure = grid.blur(&grid.blur(&exposure, 3), 3);

    (0..cells)
        .map(|i| {
            let depth = if reached[i] { depth[i].min(MAX_DEPTH) } else if planet.heights[i] < 0. { -1. } else { 0. };
            [travel[i].min(MAX_TRAVEL), depth, exposure[i], wind[i].1]
        })
        .collect()
}

/// A cell to visit, by travel time (a min-heap through the reversed order).
struct Visit(f32, usize);

impl PartialEq for Visit {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Visit {}

impl PartialOrd for Visit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Visit {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}
