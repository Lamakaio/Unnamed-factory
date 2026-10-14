//! Flow routing on a grid of cells (see [`Grid`]): depression filling (Priority-Flood),
//! steepest-descent receivers, upstream ordering and drainage accumulation.
//!
//! "Base level" cells (the sea) are outlets: they never drain anywhere and anchor every flow
//! path. On the flat map, its border cells are outlets too.

use std::{cmp::Ordering, collections::BinaryHeap};

use crate::grid::Grid;

/// The 8 neighbour offsets, and their distances (in cells).
pub const NEIGHBOURS: [(i32, i32, f32); 8] = [
    (-1, 0, 1.),
    (1, 0, 1.),
    (0, -1, 1.),
    (0, 1, 1.),
    (-1, -1, std::f32::consts::SQRT_2),
    (1, -1, std::f32::consts::SQRT_2),
    (-1, 1, std::f32::consts::SQRT_2),
    (1, 1, std::f32::consts::SQRT_2),
];

/// Iterate over the in-bounds neighbours of cell `i` of a square `size × size` grid (cells
/// indexed `z * size + x`) as `(index, distance)`.
pub fn neighbours(size: usize, i: usize) -> impl Iterator<Item = (usize, f32)> {
    let (x, z) = ((i % size) as i32, (i / size) as i32);
    NEIGHBOURS.iter().filter_map(move |&(dx, dz, distance)| {
        let (nx, nz) = (x + dx, z + dz);
        (nx >= 0 && nz >= 0 && nx < size as i32 && nz < size as i32)
            .then(|| (nz as usize * size + nx as usize, distance))
    })
}

/// A cell in the Priority-Flood queue: height and index (32 bits, to keep entries small).
#[derive(PartialEq)]
struct Entry(f32, u32);

impl Eq for Entry {}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Entry {
    // Reversed, so `BinaryHeap` pops the lowest height first.
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

/// Priority-Flood (Barnes et al. 2014): raise every depression to its spill height, so that
/// every cell can drain to a base-level cell.
///
/// With `epsilon > 0`, filled areas get a tiny slope towards their outlet, so steepest-descent
/// routing works across them; with `epsilon == 0`, lakes come out perfectly flat.
pub fn priority_flood(
    grid: &impl Grid,
    heights: &[f32],
    base_level: &[bool],
    epsilon: f32,
) -> Vec<f32> {
    priority_flood_from(grid, heights, base_level, &flood_seeds(grid, base_level), epsilon)
}

/// Where [`priority_flood`] starts: the map border, and the base level cells next to the rest
/// (the open sea beyond them would only be popped to find closed neighbours). Depends only on
/// the base level: compute it once for repeated floods.
pub fn flood_seeds(grid: &impl Grid, base_level: &[bool]) -> Vec<u32> {
    (0..base_level.len())
        .filter(|&i| {
            if base_level[i] {
                grid.neighbours(i).any(|(j, _)| !base_level[j])
            } else {
                grid.is_border(i)
            }
        })
        .map(|i| i as u32)
        .collect()
}

/// [`priority_flood`] from precomputed [`flood_seeds`].
pub fn priority_flood_from(
    grid: &impl Grid,
    heights: &[f32],
    base_level: &[bool],
    seeds: &[u32],
    epsilon: f32,
) -> Vec<f32> {
    let mut filled = heights.to_vec();
    let mut closed = base_level.to_vec();
    let mut open = BinaryHeap::with_capacity(seeds.len());
    for &i in seeds {
        closed[i as usize] = true;
        open.push(Entry(filled[i as usize], i));
    }
    while let Some(Entry(height, i)) = open.pop() {
        let i = i as usize;
        for (n, _) in grid.neighbours(i) {
            if closed[n] {
                continue;
            }
            closed[n] = true;
            if filled[n] <= height + epsilon {
                filled[n] = height + epsilon;
            }
            open.push(Entry(filled[n], n as u32));
        }
    }
    filled
}

/// Flow graph: each cell's receiver (steepest descent neighbour, itself for outlets), and the
/// cells ordered from outlets upstream (each cell comes after its receiver).
pub struct FlowGraph {
    pub receivers: Vec<usize>,
    pub receiver_distance: Vec<f32>,
    pub order: Vec<usize>,
}

impl FlowGraph {
    /// Route flow on `heights`, which must have no depressions (see [`priority_flood`]).
    pub fn new(grid: &impl Grid, heights: &[f32], base_level: &[bool]) -> Self {
        Self::with_weights(grid, heights, base_level, |_, _| 1.)
    }

    /// Like [`FlowGraph::new`], with the slope towards neighbour `j` of cell `i` multiplied by
    /// `weight(i, j)` when choosing receivers. Random weights avoid the straight, grid-aligned
    /// valleys of pure steepest descent (stochastic D8).
    pub fn with_weights(
        grid: &impl Grid,
        heights: &[f32],
        base_level: &[bool],
        weight: impl Fn(usize, usize) -> f32,
    ) -> Self {
        let n = heights.len();
        let mut receivers: Vec<usize> = (0..n).collect();
        let mut receiver_distance = vec![1.; n];
        for i in 0..n {
            if base_level[i] {
                continue;
            }
            let mut best_slope = 0.;
            for (j, distance) in grid.neighbours(i) {
                // (Only downhill neighbours can be receivers: skip weighing the others.)
                if heights[j] >= heights[i] {
                    continue;
                }
                let slope = (heights[i] - heights[j]) / distance * weight(i, j);
                if slope > best_slope {
                    best_slope = slope;
                    receivers[i] = j;
                    receiver_distance[i] = distance;
                }
            }
        }

        // Donor lists in compressed form, then a stack from the outlets upstream.
        let mut donor_count = vec![0u32; n + 1];
        for i in 0..n {
            if receivers[i] != i {
                donor_count[receivers[i] + 1] += 1;
            }
        }
        for i in 0..n {
            donor_count[i + 1] += donor_count[i];
        }
        let mut fill = donor_count.clone();
        let mut donors = vec![0usize; donor_count[n] as usize];
        for i in 0..n {
            let r = receivers[i];
            if r != i {
                donors[fill[r] as usize] = i;
                fill[r] += 1;
            }
        }
        let mut order = Vec::with_capacity(n);
        let mut stack = Vec::new();
        for root in (0..n).filter(|&i| receivers[i] == i) {
            stack.push(root);
            while let Some(i) = stack.pop() {
                order.push(i);
                stack.extend(&donors[donor_count[i] as usize..donor_count[i + 1] as usize]);
            }
        }
        Self {
            receivers,
            receiver_distance,
            order,
        }
    }

    /// Upstream drainage area of every cell, with `rain[i]` falling on each cell.
    pub fn accumulate(&self, rain: impl Fn(usize) -> f32) -> Vec<f32> {
        let mut area: Vec<f32> = (0..self.receivers.len()).map(rain).collect();
        for &i in self.order.iter().rev() {
            let r = self.receivers[i];
            if r != i {
                area[r] += area[i];
            }
        }
        area
    }
}
