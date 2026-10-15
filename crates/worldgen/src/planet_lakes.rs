//! The planet's lakes and closed basins, from the eroded heights:
//!
//! - Small patches of sea cut off from the ocean (inside continents) become land below sea
//!   level. Those little water drains into stay dry: their lowest cell is a sink (the water
//!   soaks away or evaporates), so no lake fills them.
//! - Every other depression is a lake only if it is a real one: big, deep enough, and large
//!   next to the water flowing into it (a big river needs a very large lake to end in).
//!   Others are filled: flat, poorly drained ground the river crosses (wetland potential).
//! - Kept lakes get a basin of their own instead of flooding whatever the erosion left: a
//!   rounded outline (thin arms dropped, inlets filled in), a bowl deepening away from the
//!   shore, banks just above the water, and the outlet left open.
//!
//! Returns the drainage on the result, and where the ground is wet (closed flats, lake shores)
//! for the wetlands.

use std::collections::VecDeque;

use crate::{
    flow::{FlowGraph, priority_flood},
    grid::Grid,
    sphere::SphereGrid,
};

/// Sea patches smaller than this share of the planet's cells, apart from the ocean, become
/// land.
const SEA_MIN_SHARE: f32 = 5e-4;
/// A dry basin (a former sea patch) stays a sink while less than this drains into it (mean
/// cell areas); more, and it is an ordinary depression.
const LOWLAND_MAX_INFLOW: f32 = 1500.;
/// Lowest a dry basin's floor goes (world units).
const LOWLAND_FLOOR: f32 = -1.5;
/// Water depth from which a cell counts as under a lake.
const LAKE_EPS: f32 = 0.02;
/// A depression is a lake from this many cells, this deep, and with at least this many cells
/// per mean cell area drained into it.
const LAKE_MIN_CELLS: usize = 40;
const LAKE_MIN_DEPTH: f32 = 0.3;
const LAKE_AREA_PER_INFLOW: f32 = 0.01;
/// Lake beds: depth at the shore and its increase per cell from it, and the deepest point
/// (`LAKE_MAX_DEPTH` × √cells, clamped).
const SHORE_DEPTH: f32 = 0.25;
const DEPTH_SLOPE: f32 = 0.3;
const LAKE_MAX_DEPTH: (f32, f32, f32) = (0.15, 0.8, 6.);
/// Banks: cells this close outside a lake are kept this far above it (plus a rise per cell).
const BANK_CELLS: f32 = 3.;
const BANK_HEIGHT: (f32, f32) = (0.1, 0.12);
/// Banks are not raised where this much drains through (mean cell areas): streams flowing in.
const BANK_MAX_DRAINAGE: f32 = 50.;
/// Cells of the outlet's path left open.
const OUTLET_CELLS: usize = 6;
/// Lake shores are wet within this many cells, up to this height above the lake.
const WET_SHORE: (f32, f32) = (3., 0.6);

pub struct Hydrology {
    /// The depression-filled surface, the drainage on it and the lake water over each cell.
    pub filled: Vec<f32>,
    pub drainage: Vec<f32>,
    pub lake_depth: Vec<f32>,
    /// Where water leaves the land: the sea and the dry basins' sinks.
    pub base: Vec<bool>,
    /// Wet ground (0..1): closed flats and dry basins, lake shores.
    pub wetland: Vec<f32>,
    /// Closed flats and dry basins (0..1), where water stands or evaporates: salt flats in
    /// dry climates.
    pub basins: Vec<f32>,
}

/// Connected components (side neighbours) of the cells where `mask` is set.
fn components(grid: &SphereGrid, mask: &[bool]) -> Vec<Vec<usize>> {
    let mut seen = vec![false; mask.len()];
    let mut out = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        let mut cells = Vec::new();
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        while let Some(i) = queue.pop_front() {
            cells.push(i);
            for (j, d) in grid.neighbours(i) {
                if d < 1.2 && mask[j] && !seen[j] {
                    seen[j] = true;
                    queue.push_back(j);
                }
            }
        }
        out.push(cells);
    }
    out
}

/// Distance (cells, along the grid) from the `sources`, up to `limit` (beyond: infinite).
fn distance_from(grid: &SphereGrid, sources: &[usize], limit: f32, allowed: impl Fn(usize) -> bool) -> Vec<f32> {
    let mut distance = vec![f32::INFINITY; grid.len()];
    let mut queue = VecDeque::new();
    for &i in sources {
        distance[i] = 0.;
        queue.push_back(i);
    }
    // (Breadth first with re-queuing: fine for these small reaches.)
    while let Some(i) = queue.pop_front() {
        for (j, d) in grid.neighbours(i) {
            let next = distance[i] + d;
            if next < distance[j] && next <= limit && allowed(j) {
                distance[j] = next;
                queue.push_back(j);
            }
        }
    }
    distance
}

pub fn shape_lakes(grid: &SphereGrid, heights: &mut [f32], sea: &mut [bool]) -> Hydrology {
    let cells = grid.len();
    let area = |i: usize| grid.area(i);
    let mut basins = vec![0f32; cells];

    // Small seas inside continents: land below sea level, dry basins while little drains in.
    let mut seas = components(grid, sea);
    seas.sort_by_key(|c| std::cmp::Reverse(c.len()));
    let mut lowlands = Vec::new();
    for patch in seas.into_iter().skip(1) {
        if patch.len() as f32 >= SEA_MIN_SHARE * cells as f32 {
            continue;
        }
        for &i in &patch {
            sea[i] = false;
            heights[i] = heights[i].max(LOWLAND_FLOOR);
        }
        let sink = *patch.iter().min_by(|&&a, &&b| heights[a].total_cmp(&heights[b])).unwrap();
        lowlands.push((patch, sink));
    }
    let mut base = sea.to_vec();
    for (_, sink) in &lowlands {
        base[*sink] = true;
    }
    let flow = |heights: &[f32], base: &[bool]| {
        let filled = priority_flood(grid, heights, base, 1e-4);
        let graph = FlowGraph::new(grid, &filled, base);
        let drainage = graph.accumulate(area);
        (filled, graph, drainage)
    };
    let (_, _, drainage) = flow(heights, &base);
    for (patch, sink) in &lowlands {
        if drainage[*sink] > LOWLAND_MAX_INFLOW {
            base[*sink] = false;
        } else {
            for &i in patch {
                basins[i] = 1.;
            }
        }
    }

    // Depressions: real lakes, or filled flats. (Twice: reshaping the lakes may leave new
    // hollows next to them.)
    let mut wetland = vec![0f32; cells];
    for _ in 0..2 {
        let (filled, graph, drainage) = flow(heights, &base);
        let under: Vec<bool> = (0..cells).map(|i| !base[i] && filled[i] - heights[i] > LAKE_EPS).collect();
        let mut lakes = Vec::new();
        for lake in components(grid, &under) {
            let depth = lake.iter().map(|&i| filled[i] - heights[i]).fold(0f32, f32::max);
            let outlet = *lake.iter().max_by(|&&a, &&b| drainage[a].total_cmp(&drainage[b])).unwrap();
            let level = lake.iter().map(|&i| filled[i]).fold(f32::MIN, f32::max);
            let keep = lake.len() >= LAKE_MIN_CELLS
                && depth >= LAKE_MIN_DEPTH
                && lake.len() as f32 >= LAKE_AREA_PER_INFLOW * drainage[outlet];
            if keep {
                lakes.push((lake, level, outlet));
            } else {
                for &i in &lake {
                    heights[i] = filled[i];
                    basins[i] = 1.;
                }
            }
        }

        // Kept lakes: rounded outlines (the blurred mask), bowl beds, banks, open outlets.
        let mut lake_of = vec![u32::MAX; cells];
        let mut mask = vec![0f32; cells];
        for (k, (lake, _, _)) in lakes.iter().enumerate() {
            for &i in lake {
                lake_of[i] = k as u32;
                mask[i] = 1.;
            }
        }
        let rounded: Vec<bool> = grid.blur(&mask, 2).iter().map(|&m| m > 0.5).collect();
        // Cells of the rounded outlines take the level of the nearest lake.
        let mut queue: VecDeque<usize> = (0..cells).filter(|&i| lake_of[i] != u32::MAX).collect();
        while let Some(i) = queue.pop_front() {
            for (j, d) in grid.neighbours(i) {
                if d < 1.2 && rounded[j] && lake_of[j] == u32::MAX && !base[j] {
                    lake_of[j] = lake_of[i];
                    queue.push_back(j);
                }
            }
        }
        // (The outlet's path stays as it was: the lake keeps its level.)
        let mut outlet_path = vec![false; cells];
        for (_, _, outlet) in &lakes {
            let mut i = *outlet;
            for _ in 0..OUTLET_CELLS {
                outlet_path[i] = true;
                i = graph.receivers[i];
            }
        }
        let inside = |i: usize| rounded[i] && lake_of[i] != u32::MAX && !outlet_path[i];
        let shore: Vec<usize> =
            (0..cells).filter(|&i| inside(i) && grid.neighbours(i).any(|(j, _)| !inside(j))).collect();
        let from_shore = distance_from(grid, &shore, f32::INFINITY, inside);
        let outside: Vec<usize> =
            (0..cells).filter(|&i| !inside(i) && grid.neighbours(i).any(|(j, _)| inside(j))).collect();
        let from_lake = distance_from(grid, &outside, BANK_CELLS.max(WET_SHORE.0), |i| !inside(i));
        // The nearest lake of the cells just outside (for their banks).
        let mut near_lake = vec![u32::MAX; cells];
        let mut queue: VecDeque<usize> = (0..cells).filter(|&i| inside(i)).collect();
        while let Some(i) = queue.pop_front() {
            let k = if inside(i) { lake_of[i] } else { near_lake[i] };
            for (j, _) in grid.neighbours(i) {
                if !inside(j) && near_lake[j] == u32::MAX && from_lake[j].is_finite() {
                    near_lake[j] = k;
                    queue.push_back(j);
                }
            }
        }
        for i in 0..cells {
            if inside(i) {
                let (lake, level, _) = &lakes[lake_of[i] as usize];
                let deepest = (LAKE_MAX_DEPTH.0 * (lake.len() as f32).sqrt()).clamp(LAKE_MAX_DEPTH.1, LAKE_MAX_DEPTH.2);
                let depth = (SHORE_DEPTH + DEPTH_SLOPE * from_shore[i]).min(deepest);
                heights[i] = level - depth;
            } else if near_lake[i] != u32::MAX {
                let level = lakes[near_lake[i] as usize].1;
                let d = from_lake[i];
                // (Never across a river: that would dam it.)
                if d <= BANK_CELLS && !outlet_path[i] && drainage[i] < BANK_MAX_DRAINAGE {
                    heights[i] = heights[i].max(level + BANK_HEIGHT.0 + BANK_HEIGHT.1 * d);
                }
                if d <= WET_SHORE.0 && heights[i] < level + WET_SHORE.1 {
                    wetland[i] = wetland[i].max(0.8 * (1. - d / WET_SHORE.0));
                }
            }
        }

    }
    for (w, b) in wetland.iter_mut().zip(&basins) {
        *w = w.max(*b);
    }
    // Last, fill what small hollows the reshaping left (filling makes no new ones).
    let (filled, _, _) = flow(heights, &base);
    let under: Vec<bool> = (0..cells).map(|i| !base[i] && filled[i] - heights[i] > LAKE_EPS).collect();
    for hollow in components(grid, &under).into_iter().filter(|c| c.len() < LAKE_MIN_CELLS) {
        for i in hollow {
            heights[i] = filled[i];
            basins[i] = 1.;
            wetland[i] = 1.;
        }
    }
    let (filled, _, drainage) = flow(heights, &base);
    let lake_depth = (0..cells).map(|i| (filled[i] - heights[i]).max(0.)).collect();
    Hydrology { filled, drainage, lake_depth, base, wetland, basins }
}
