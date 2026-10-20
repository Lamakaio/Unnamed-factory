//! Flow routing on a grid of cells (see [`Grid`]): depression filling (Priority-Flood),
//! steepest-descent receivers, upstream ordering and drainage accumulation.
//!
//! "Base level" cells (the sea) are outlets: they never drain anywhere and anchor every flow
//! path. So are a grid's border cells, if it has any (`Grid::is_border`; the sphere has none).

#[cfg(test)]
use std::{cmp::Ordering, collections::BinaryHeap};

use crate::{grid::Grid, par};

/// A cell in the Priority-Flood queue: height and index (32 bits, to keep entries small).
#[cfg(test)]
#[derive(PartialEq)]
struct Entry(f32, u32);

#[cfg(test)]
impl Eq for Entry {}

#[cfg(test)]
impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
impl Ord for Entry {
    // Reversed, so `BinaryHeap` pops the lowest height first.
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

/// Depression filling: raise every depression to its spill height, so that every cell can
/// drain to a base-level cell. The result of Priority-Flood (Barnes, Lehman & Mulla 2014),
/// computed on all cores: the spill levels come from routing through the depressions
/// ([`FlowGraph::with_depressions`]'s water surface, the filling without `epsilon`), then the
/// `epsilon` slope is relaxed in parallel rounds.
///
/// With `epsilon > 0`, filled areas get a tiny slope towards their outlet (each cell at least
/// `epsilon` above a neighbour: the least `f` with `f(i) = max(h(i), min f(j) + epsilon)`, as
/// Priority-Flood+ε makes it), so steepest-descent routing works across them; with
/// `epsilon == 0`, lakes come out perfectly flat.
pub fn priority_flood(
    grid: &impl Grid,
    heights: &[f32],
    base_level: &[bool],
    epsilon: f32,
) -> Vec<f32> {
    let (_, _, mut filled) = route_depressions(grid, heights, base_level, |_, _| 1.);
    if epsilon <= 0. {
        return filled;
    }
    let outlet = |i: usize| base_level[i] || grid.is_border(i);
    // The level a cell must reach: `epsilon` above its lowest neighbour (none for outlets).
    let target = |filled: &[f32], i: usize| -> Option<f32> {
        if outlet(i) {
            return None;
        }
        let lowest = grid
            .neighbours(i)
            .map(|(j, _)| filled[j])
            .fold(f32::MAX, f32::min);
        let level = (lowest + epsilon).max(heights[i]);
        // (Not for float rounding: `lowest + epsilon` can come out a hair above a cell already
        // `epsilon` up, and raising by hairs would ripple through the flats for hundreds of
        // rounds.)
        (level > filled[i] + epsilon * 0.01).then_some(level)
    };
    // Flats first (cells with no lower neighbour, all at one level: filled lakes and dead-flat
    // ground): `epsilon` per cell of their distance from where they spill, by a breadth-first
    // walk from those cells.
    let water = filled.clone();
    let flat = par::map(heights.len(), |i| {
        !outlet(i) && grid.neighbours(i).all(|(j, _)| water[j] >= water[i])
    });
    let mut queue: std::collections::VecDeque<u32> = par::map(heights.len(), |i| {
        flat[i]
            && grid
                .neighbours(i)
                .any(|(j, _)| !flat[j] && water[j] <= water[i])
    })
    .iter()
    .enumerate()
    .filter_map(|(i, &exit)| exit.then_some(i as u32))
    .collect();
    let mut steps = vec![0u32; heights.len()];
    for &i in &queue {
        steps[i as usize] = 1;
    }
    while let Some(i) = queue.pop_front() {
        let i = i as usize;
        for (j, _) in grid.neighbours(i) {
            if flat[j] && steps[j] == 0 {
                steps[j] = steps[i] + 1;
                queue.push_back(j as u32);
            }
        }
    }
    for i in 0..heights.len() {
        if steps[i] > 0 {
            filled[i] = water[i] + epsilon * steps[i] as f32;
        }
    }
    // Then what is left (gentle slopes under `epsilon` per cell, ground next to the raised
    // flats): Jacobi rounds from below, each raising the cells short of their level, then
    // looking again at their neighbours.
    let mut active: Vec<u32> = par::map(heights.len(), |i| target(&filled, i).is_some())
        .iter()
        .enumerate()
        .filter_map(|(i, &a)| a.then_some(i as u32))
        .collect();
    let mut stamp = vec![0u32; heights.len()];
    let mut round = 0;
    while !active.is_empty() {
        round += 1;
        let raised: Vec<(u32, f32)> = par::map(active.len(), |k| {
            let i = active[k] as usize;
            target(&filled, i).map(|level| (i as u32, level))
        })
        .into_iter()
        .flatten()
        .collect();
        for &(i, level) in &raised {
            filled[i as usize] = level;
        }
        active.clear();
        for &(i, _) in &raised {
            for (j, _) in grid
                .neighbours(i as usize)
                .chain(std::iter::once((i as usize, 0.)))
            {
                if stamp[j] != round && !outlet(j) {
                    stamp[j] = round;
                    active.push(j as u32);
                }
            }
        }
    }
    filled
}

/// The classic sequential Priority-Flood (a priority queue from the outlets), to check
/// [`priority_flood`] against.
#[cfg(test)]
pub fn priority_flood_reference(
    grid: &impl Grid,
    heights: &[f32],
    base_level: &[bool],
    epsilon: f32,
) -> Vec<f32> {
    let mut filled = heights.to_vec();
    let mut closed = base_level.to_vec();
    let mut open = BinaryHeap::new();
    for i in 0..heights.len() {
        if base_level[i] && grid.neighbours(i).any(|(j, _)| !base_level[j])
            || !base_level[i] && grid.is_border(i)
        {
            closed[i] = true;
            open.push(Entry(filled[i], i as u32));
        }
    }
    while let Some(Entry(height, i)) = open.pop() {
        for (n, _) in grid.neighbours(i as usize) {
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

/// Each cell's steepest-descent receiver (itself for base level cells and pits) and the
/// distance to it, with the slope towards neighbour `j` of cell `i` weighted by `weight(i, j)`.
fn steepest_receivers(
    grid: &impl Grid,
    heights: &[f32],
    base_level: &[bool],
    weight: impl Fn(usize, usize) -> f32 + Sync,
) -> (Vec<usize>, Vec<f32>) {
    // (Each cell on its own: on all cores.)
    par::map(heights.len(), |i| {
        let mut best = (i, 1.);
        if base_level[i] {
            return best;
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
                best = (j, distance);
            }
        }
        best
    })
    .into_iter()
    .unzip()
}

/// Flow graph: each cell's receiver (steepest descent neighbour, itself for outlets), and the
/// cells ordered from outlets upstream (each cell comes after its receiver).
pub struct FlowGraph {
    pub receivers: Vec<usize>,
    pub receiver_distance: Vec<f32>,
    pub order: Vec<usize>,
    /// The trees of more than one cell (a root and all that drains to it), as ranges of
    /// `order`: independent, so walks up or down them can run on all cores (see
    /// [`FlowGraph::for_each_tree`]).
    pub trees: Vec<(usize, usize)>,
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
        weight: impl Fn(usize, usize) -> f32 + Sync,
    ) -> Self {
        let (receivers, receiver_distance) = steepest_receivers(grid, heights, base_level, weight);
        Self::from_receivers(receivers, receiver_distance)
    }

    /// Route flow on `heights`, depressions included, without filling them (Cordonnier, Bovy
    /// & Braun 2019, "A versatile, linear complexity algorithm for flow routing in topographies
    /// with depressions"): steepest descent (weighted as in [`FlowGraph::with_weights`]) leaves
    /// pits, each the bottom of a basin; basins are linked through their lowest passes, and
    /// the minimum spanning tree of those links, rooted at the outlets (base level and map
    /// border), says where each basin spills. Each pit is then routed out over its pass by
    /// reversing the flow path from the pass down to the pit ("carving"), so the outflow
    /// crosses the basin's sill as it would over a filled lake.
    ///
    /// Also returns the water surface: the heights, raised inside each basin to the level it
    /// spills at (the same as [`priority_flood`] without epsilon).
    pub fn with_depressions(
        grid: &impl Grid,
        heights: &[f32],
        base_level: &[bool],
        weight: impl Fn(usize, usize) -> f32 + Sync,
    ) -> (Self, Vec<f32>) {
        let (receivers, receiver_distance, water) =
            route_depressions(grid, heights, base_level, weight);
        (Self::from_receivers(receivers, receiver_distance), water)
    }

    /// The flow graph of given receivers: donor lists, then the cells ordered from the
    /// outlets upstream.
    fn from_receivers(receivers: Vec<usize>, receiver_distance: Vec<f32>) -> Self {
        let n = receivers.len();
        // Donor lists in compressed form, then a stack from the outlets upstream. (Three plain
        // passes: cheaper than finding each cell's donors among its neighbours, on all cores.)
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
        for (i, &r) in receivers.iter().enumerate() {
            if r != i {
                donors[fill[r] as usize] = i;
                fill[r] += 1;
            }
        }
        let mut order = Vec::with_capacity(n);
        let mut trees = Vec::new();
        let mut stack = Vec::new();
        for root in (0..n).filter(|&i| receivers[i] == i) {
            let start = order.len();
            stack.push(root);
            while let Some(i) = stack.pop() {
                order.push(i);
                stack.extend(&donors[donor_count[i] as usize..donor_count[i + 1] as usize]);
            }
            if order.len() > start + 1 {
                trees.push((start, order.len()));
            }
        }
        Self {
            receivers,
            receiver_distance,
            order,
            trees,
        }
    }

    /// `f(cells)` for each tree's cells (outlet first, each cell after its receiver), on all
    /// cores. `f` may write anything belonging to the tree's cells (see [`par::Shared`]): the
    /// trees share none.
    pub fn for_each_tree(&self, f: impl Fn(&[usize]) + Sync) {
        par::for_each_group(&self.trees, |a, b| f(&self.order[a..b]));
    }

    /// Upstream drainage area of every cell, with `rain[i]` falling on each cell.
    pub fn accumulate(&self, rain: impl Fn(usize) -> f32 + Sync) -> Vec<f32> {
        let mut area = par::map(self.receivers.len(), rain);
        let shared = par::Shared::new(&mut area);
        self.for_each_tree(|cells| {
            for &i in cells.iter().rev() {
                let r = self.receivers[i];
                if r != i {
                    // SAFETY: `i` and its receiver are in this tree, which no other thread
                    // walks.
                    unsafe { *shared.get(r) += *shared.get(i) };
                }
            }
        });
        area
    }
}

/// [`FlowGraph::with_depressions`]'s receivers (carved through the pits) and their distances,
/// and the water surface.
fn route_depressions(
    grid: &impl Grid,
    heights: &[f32],
    base_level: &[bool],
    weight: impl Fn(usize, usize) -> f32 + Sync,
) -> (Vec<usize>, Vec<f32>, Vec<f32>) {
    let n = heights.len();

    let (mut receivers, mut receiver_distance) =
        steepest_receivers(grid, heights, base_level, weight);
    // Basin of each cell (0: the outlets), by following receivers down to their root.
    const UNSET: u32 = u32::MAX;
    let mut basin = vec![UNSET; n];
    let mut pits = vec![usize::MAX];
    let mut path = Vec::new();
    for start in 0..n {
        let mut i = start;
        while basin[i] == UNSET && receivers[i] != i {
            path.push(i);
            i = receivers[i];
        }
        if basin[i] == UNSET {
            basin[i] = if base_level[i] || grid.is_border(i) {
                0
            } else {
                pits.push(i);
                (pits.len() - 1) as u32
            };
        }
        let b = basin[i];
        for k in path.drain(..) {
            basin[k] = b;
        }
    }
    // Cells in the pits' basins (the only ones the basin graph looks at), in index order.
    let in_pits: Vec<usize> = (0..n).filter(|&i| basin[i] != 0).collect();
    let mut water = heights.to_vec();
    if pits.len() > 1 {
        // Lowest pass between each pair of neighbouring basins: (height, cell on each side).
        // (In runs of cells on all cores, merged in order: the first lowest found wins, as in
        // one pass.)
        type Passes = std::collections::HashMap<(u32, u32), (f32, u32, u32)>;
        let keep = |passes: &mut Passes, key: (u32, u32), pass: (f32, u32, u32)| {
            let entry = passes.entry(key).or_insert((f32::MAX, 0, 0));
            if pass.0 < entry.0 {
                *entry = pass;
            }
        };
        let run = in_pits.len().div_ceil(par::threads() * 4).max(1);
        let runs = par::map(in_pits.len().div_ceil(run), |k| {
            let mut passes = Passes::default();
            for &i in &in_pits[k * run..((k + 1) * run).min(in_pits.len())] {
                let bi = basin[i];
                for (j, _) in grid.neighbours(i) {
                    let bj = basin[j];
                    if bj == bi {
                        continue;
                    }
                    let height = heights[i].max(heights[j]);
                    let (a, b) = if bi < bj { (i, j) } else { (j, i) };
                    keep(
                        &mut passes,
                        (bi.min(bj), bi.max(bj)),
                        (height, a as u32, b as u32),
                    );
                }
            }
            passes
        });
        let mut runs = runs.into_iter();
        let mut passes = runs.next().unwrap_or_default();
        for run in runs {
            for (key, pass) in run {
                keep(&mut passes, key, pass);
            }
        }
        // Minimum spanning tree (Kruskal).
        let mut links: Vec<_> = passes.into_iter().collect();
        links.sort_by(|a, b| a.1.0.total_cmp(&b.1.0).then(a.0.cmp(&b.0)));
        let mut parent: Vec<u32> = (0..pits.len() as u32).collect();
        fn find(parent: &mut [u32], mut x: u32) -> u32 {
            while parent[x as usize] != x {
                parent[x as usize] = parent[parent[x as usize] as usize];
                x = parent[x as usize];
            }
            x
        }
        let mut tree: Vec<Vec<(u32, f32, u32, u32)>> = vec![Vec::new(); pits.len()];
        for ((ba, bb), (height, a, b)) in links {
            let (ra, rb) = (find(&mut parent, ba), find(&mut parent, bb));
            if ra == rb {
                continue;
            }
            parent[ra as usize] = rb;
            // (Cell `a` is in basin `ba`, cell `b` in basin `bb`.)
            tree[ba as usize].push((bb, height, a, b));
            tree[bb as usize].push((ba, height, b, a));
        }
        // Rooted at the outlets: each basin spills over the link to its parent, at the
        // pass height or its parent's level, whichever is higher.
        let mut level = vec![f32::MIN; pits.len()];
        let mut visited = vec![false; pits.len()];
        visited[0] = true;
        let mut queue = std::collections::VecDeque::from([0u32]);
        let mut spills = Vec::new();
        while let Some(b) = queue.pop_front() {
            for &(child, height, here, there) in &tree[b as usize] {
                if visited[child as usize] {
                    continue;
                }
                visited[child as usize] = true;
                level[child as usize] = height.max(level[b as usize]);
                // (`there` is the pass cell in the child basin, `here` in the parent.)
                spills.push((child, there as usize, here as usize));
                queue.push_back(child);
            }
        }
        for &i in &in_pits {
            water[i] = water[i].max(level[basin[i] as usize]);
        }
        // Carve: reverse the receivers from the pass cell down to the pit, so the pit
        // drains through the pass into the downstream basin.
        for (child, inside, outside) in spills {
            let pit = pits[child as usize];
            let mut previous = outside;
            let mut previous_distance = grid
                .neighbours(inside)
                .find(|&(j, _)| j == outside)
                .map_or(1., |(_, d)| d);
            let mut cell = inside;
            loop {
                let next = receivers[cell];
                let next_distance = receiver_distance[cell];
                receivers[cell] = previous;
                receiver_distance[cell] = previous_distance;
                if cell == pit {
                    break;
                }
                (previous, previous_distance, cell) = (cell, next_distance, next);
            }
        }
    }
    (receivers, receiver_distance, water)
}
