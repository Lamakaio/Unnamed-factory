//! Stream-power erosion on any grid, from a [`Shape`] (built by `planet.rs`: provinces of
//! plains, hill country or plateau, mountain ranges, rifts). Provinces set the erosion
//! parameters, so the same physics produces floodplains, rounded hills, canyon-cut tablelands
//! and sharp ranges.
//!
//! Lakes come out of erosion itself: basins sink (rifts, kettle hollows on plains) or are
//! overdeepened by glaciers (high mountain valleys), and erosion routes water across them to
//! their outlet but fills them only slowly with sediment, so a basin holds a lake where it is
//! sinking or being dug faster than it fills.

use std::time::{Duration, Instant};

use crate::{
    geology::Rock,
    par,
    flow::FlowGraph,
    grid::Grid,
    noise::{hash, smoothstep},
};

/// Inputs of the erosion simulation, per cell.
pub(crate) struct Shape {
    /// Tectonic uplift rate (relative).
    pub uplift: Vec<f32>,
    /// Erodibility (relative): low values are hard rock.
    pub erodibility: Vec<f32>,
    /// Hillslope diffusion: high values round hills off.
    pub diffusion: Vec<f32>,
    /// Rain falling on each cell (relative, mean ~1 on land).
    pub rain: Vec<f32>,
    /// Sea cells: fixed at sea level (0) during erosion; every river drains to them.
    pub base_level: Vec<bool>,
    /// Initial heights (a gentle land surface, 0 in the sea).
    pub heights: Vec<f32>,
    /// Weight of each province type per cell (`[plains, hills, plateau]`), and of mountain ranges.
    pub provinces: Vec<[f32; 3]>,
    pub range: Vec<f32>,
    /// Cap level (world units) of the plateau the cell belongs to.
    pub plateau_cap: Vec<f32>,
    /// Subsidence rate (relative, like `uplift`): rift floors and kettle hollows.
    pub subsidence: Vec<f32>,
    /// Overdeepening of glacial valleys along their course (0.4..1.6, per cell).
    pub glacial_noise: Vec<f32>,
    /// What the ground is made of, and the strata offset of layered rocks (see `geology`):
    /// they scale the erodibility and diffusion by the bed at each cell's height.
    pub rock: Vec<Rock>,
    pub strata: Vec<f32>,
}

impl Shape {
    /// How rugged the surface is (0 smooth plains .. 1 sharp ranges), for fine detail.
    pub fn ruggedness(&self, i: usize) -> f32 {
        let [plains, hills, plateau] = self.provinces[i];
        let interior = plains * 0.05 + hills * 0.45 + plateau * 0.3;
        (interior + self.range[i]).clamp(0., 1.)
    }
}

/// Erosion parameters.
#[derive(Clone, Debug)]
pub(crate) struct ErosionSettings {
    pub seed: u64,
    /// Steps and their length (relative time).
    pub steps: usize,
    pub dt: f32,
    /// Stream power: erodibility and the drainage area's exponent.
    pub erodibility: f32,
    pub area_exponent: f32,
    /// Random weight on the slopes when routing (stochastic D8), against grid-aligned valleys.
    pub routing_jitter: f32,
    /// Length of a cell (in full-size cells: coarse grids have longer ones).
    pub cell_size: f32,
}

/// Share of its depth a lake fills with sediment per erosion step.
const LAKE_FILL: f32 = 0.05;
/// Glaciers: the last steps of erosion, where they flow (relative height above the 75th
/// percentile of land, upstream area in cells), and how fast they dig.
const GLACIAL_STEPS: usize = 12;
const GLACIAL_HEIGHT: (f32, f32) = (0.0, 0.25);
const GLACIAL_AREA: (f32, f32) = (40., 250.);
const GLACIAL_RATE: f32 = 0.06;

/// Largest explicit diffusion step: the cell-sized checker mode is multiplied by `1 - 8k`
/// per step, so it is damped quickly below ~0.12.
const MAX_DIFFUSION_STEP: f32 = 0.1;

/// Time spent in each part of [`erode`], summed over its steps.
#[derive(Default, Debug)]
pub(crate) struct ErosionTimings {
    /// Routing through depressions ([`FlowGraph::with_depressions`]).
    pub routing: Duration,
    pub accumulation: Duration,
    pub incision: Duration,
    pub glaciers: Duration,
    pub diffusion: Duration,
    /// Diffusion substeps per erosion step.
    pub diffusion_substeps: usize,
}

/// [`erode`], also returning the time spent in each of its parts.
pub(crate) fn erode_timed(
    grid: &impl Grid,
    settings: &ErosionSettings,
    shape: &Shape,
) -> (Vec<f32>, ErosionTimings) {
    let mut timings = ErosionTimings::default();
    let mut heights = shape.heights.clone();
    let dt = settings.dt;
    // Hillslope diffusion acts on the land cells, between side neighbours (one cell apart):
    // listed once (compressed rows), the grid's topology doesn't change.
    let diffusing: Vec<u32> = (0..heights.len() as u32)
        .filter(|&i| !shape.base_level[i as usize])
        .collect();
    let mut side_start = Vec::with_capacity(diffusing.len() + 1);
    let mut sides = Vec::with_capacity(diffusing.len() * 4);
    side_start.push(0u32);
    for &i in &diffusing {
        sides.extend(
            grid.neighbours(i as usize)
                .filter(|&(_, distance)| distance < 1.2)
                .map(|(j, _)| j as u32),
        );
        side_start.push(sides.len() as u32);
    }
    // (Lengths in cells of `cell_size`: the Laplacian divides by its square.)
    let max_k = shape.diffusion.iter().fold(0f32, |a, &d| a.max(d)) * Rock::max_diffusion() * dt
        / settings.cell_size.powi(2);
    for step in 0..settings.steps {
        let mut clock = Instant::now();
        let mut lap = |total: &mut Duration| {
            *total += clock.elapsed();
            clock = Instant::now();
        };
        // Routing through the depressions (lakes) without filling them first.
        let seed = hash(settings.seed as u32, step as u32, 0x51ED);
        let jitter = settings.routing_jitter;
        let (graph, water) = FlowGraph::with_depressions(grid, &heights, &shape.base_level, |i, j| {
            1. + jitter * (hash(seed, i as u32, j as u32) as f32 / u32::MAX as f32)
        });
        lap(&mut timings.routing);
        let area = graph.accumulate(|i| shape.rain[i]);
        lap(&mut timings.accumulation);

        // (Implicit, from the outlets upstream: each cell after its receiver. The trees are
        // independent: on all cores. Single-cell trees are outlets: nothing to do.)
        let shared = par::Shared::new(&mut heights);
        graph.for_each_tree(|cells| {
            for &i in cells {
                if shape.base_level[i] {
                    continue;
                }
                // SAFETY: the tree's cells are this thread's alone.
                let height = unsafe { shared.get(i) };
                let r = graph.receivers[i];
                let uplifted = *height + (shape.uplift[i] - shape.subsidence[i]) * dt;
                // Under a lake: still water, sediment settling.
                let depth = water[i] - uplifted;
                if depth > 1e-3 {
                    *height = uplifted + depth * LAKE_FILL;
                    continue;
                }
                if r == i {
                    *height = uplifted;
                    continue;
                }
                // (The rock's bed at the cell's height: hard caprock, soft shale...)
                let rock = shape.rock[i].hardness(*height + shape.strata[i]).0;
                let f = settings.erodibility
                    * shape.erodibility[i]
                    * rock
                    * dt
                    * area[i].powf(settings.area_exponent)
                    / (graph.receiver_distance[i] * settings.cell_size);
                *height = (uplifted + f * water[r]) / (1. + f);
            }
        });
        lap(&mut timings.incision);

        // Glaciers in the high valleys, digging unevenly along their course (overdeepening).
        if step + GLACIAL_STEPS >= settings.steps {
            let mut land: Vec<f32> = (0..heights.len())
                .filter(|&i| !shape.base_level[i])
                .map(|i| heights[i])
                .collect();
            // (The 75th percentile and the maximum: a selection, not a sort.)
            let top = land.iter().copied().fold(f32::MIN, f32::max);
            let high = if land.is_empty() {
                0.
            } else {
                let k = land.len() * 3 / 4;
                *land.select_nth_unstable_by(k, f32::total_cmp).1
            };
            let top = if land.is_empty() { 1. } else { top }.max(high + 1e-3);
            let ice: Vec<f32> = par::map(heights.len(), |i| {
                let height = (heights[i] - high) / (top - high);
                smoothstep(GLACIAL_HEIGHT.0, GLACIAL_HEIGHT.1, height)
                    * smoothstep(GLACIAL_AREA.0, GLACIAL_AREA.1, area[i])
                    * shape.range[i].min(1.)
            });
            // Glaciers are wider than rivers: the trough spreads around the flow line.
            let ice = grid.blur(&ice, 1);
            for (i, h) in heights.iter_mut().enumerate() {
                if !shape.base_level[i] {
                    *h -= ice[i] * GLACIAL_RATE * dt * shape.glacial_noise[i];
                }
            }
        }

        lap(&mut timings.glaciers);
        // Hillslope diffusion, explicit, in substeps of at most `MAX_DIFFUSION_STEP`: closer
        // to the stability limit (0.25), the cell-sized checker mode stops being damped and
        // shows as stripes.
        let substeps = (max_k / MAX_DIFFUSION_STEP).ceil().max(1.);
        timings.diffusion_substeps = substeps as usize;
        let mut updated = vec![0f32; diffusing.len()];
        for _ in 0..substeps as usize {
            // (Each cell from the previous substep only: on all cores.)
            par::for_each_mut(&mut updated, |c, new| {
                let i = diffusing[c] as usize;
                let rock = shape.rock[i].hardness(heights[i] + shape.strata[i]).1;
                let k = shape.diffusion[i] * rock * dt / substeps / settings.cell_size.powi(2);
                let sides = &sides[side_start[c] as usize..side_start[c + 1] as usize];
                let sum: f32 = sides.iter().map(|&j| heights[j as usize]).sum();
                *new = heights[i] + k * (sum - sides.len() as f32 * heights[i]);
            });
            for (c, &i) in diffusing.iter().enumerate() {
                heights[i as usize] = updated[c];
            }
        }
        lap(&mut timings.diffusion);
    }
    (heights, timings)
}

/// Plateaus: cut the high ground of plateau provinces down to their cap level (a resistant
/// caprock), keeping the canyons below it: tablelands with escarpments.
pub(crate) fn cap_plateaus(shape: &Shape, heights: &mut [f32]) {
    for (i, h) in heights.iter_mut().enumerate() {
        let weight = shape.provinces[i][2] * (1. - shape.range[i]);
        let cap = shape.plateau_cap[i];
        if weight <= 0. || *h <= cap {
            continue;
        }
        // Soft cap: keep a little of the relief above it.
        let capped = cap + (*h - cap) * 0.08;
        *h += (capped - *h) * smoothstep(0.2, 0.7, weight);
    }
}

/// Polynomial smooth minimum: blends `a` and `b` over a band of `k` world units.
pub(crate) fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = (k - (a - b).abs()).max(0.) / k;
    a.min(b) - h * h * k / 4.
}
