//! Stage A (macro shape: tectonic provinces, mountain ranges, coastline, rain) and stage B
//! (stream-power erosion).
//!
//! The continent is split into tectonic plates (warped Voronoi cells). Each plate interior is a
//! [`Landform`] province (plains, hill country or plateau). Mountain ranges rise along the
//! boundaries where plates converge, and rift valleys open where they diverge: the floor sinks
//! (in segments, holding chains of lakes) between raised shoulders (escarpments). Provinces set
//! the erosion parameters, so the same physics produces floodplains, rounded hills, canyon-cut
//! tablelands and sharp ranges.
//!
//! Lakes come out of erosion itself: basins sink (rifts, kettle hollows on plains) or are
//! overdeepened by glaciers (high mountain valleys), and erosion routes water across them to
//! their outlet but fills them only slowly with sediment, so a basin holds a lake where it is
//! sinking or being dug faster than it fills.

use std::time::{Duration, Instant};

use bevy_math::Vec2;
use rand::{Rng, RngExt};

use crate::{
    Landform, WorldgenSettings,
    flow::{FlowGraph, neighbours},
    grid::Grid,
    noise::{Noise, hash, smoothstep},
};

/// Inputs of the erosion simulation, per cell.
pub struct Shape {
    /// 0 in the sea, 1 inland.
    pub land: Vec<f32>,
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
    /// Sea floor depth (world units, negative), applied after erosion.
    pub sea_floor: Vec<f32>,
    /// Weight of each province type per cell (`[plains, hills, plateau]`), and of mountain ranges.
    pub provinces: Vec<[f32; 3]>,
    pub range: Vec<f32>,
    /// Cap level (world units) of the plateau the cell belongs to.
    pub plateau_cap: Vec<f32>,
    /// Subsidence rate (relative, like `uplift`): rift floors and kettle hollows.
    pub subsidence: Vec<f32>,
    /// Rift valley floor (0..1), for lake classification.
    pub rift: Vec<f32>,
    /// Overdeepening of glacial valleys along their course (0.4..1.6, per cell).
    pub glacial_noise: Vec<f32>,
}

impl Shape {
    /// Dominant landform of a cell.
    pub fn landform(&self, i: usize) -> Landform {
        if self.base_level[i] {
            return Landform::Sea;
        }
        let [plains, hills, plateau] = self.provinces[i];
        if self.range[i] > 0.35 {
            Landform::Mountains
        } else if plateau >= plains && plateau >= hills {
            Landform::Plateau
        } else if hills >= plains {
            Landform::Hills
        } else {
            Landform::Plains
        }
    }

    /// How rugged the surface is (0 smooth plains .. 1 sharp ranges), for fine detail.
    pub fn ruggedness(&self, i: usize) -> f32 {
        let [plains, hills, plateau] = self.provinces[i];
        let interior = plains * 0.05 + hills * 0.45 + plateau * 0.3;
        (interior + self.range[i]).clamp(0., 1.)
    }
}

struct Plate {
    center: Vec2,
    velocity: Vec2,
    /// Province type index: 0 plains, 1 hills, 2 plateau.
    kind: usize,
    /// Plateau cap level (world units), for plateau plates.
    cap: f32,
}

/// Stage A: plates, provinces, ranges, coastline and rain.
pub fn shape(settings: &WorldgenSettings, noise: Noise, rng: &mut impl Rng) -> Shape {
    let size = settings.size;
    let half = size as f32 / 2.;
    let n = size * size;
    let plates = make_plates(settings, half, rng);

    let coast = noise.layer(1);
    let warp = noise.layer(2);
    let peaks = noise.layer(3);
    let hills_noise = noise.layer(4);
    let surface = noise.layer(5);

    let mut shape = Shape {
        land: vec![0.; n],
        uplift: vec![0.; n],
        erodibility: vec![1.; n],
        diffusion: vec![0.; n],
        rain: vec![1.; n],
        base_level: vec![false; n],
        heights: vec![0.; n],
        sea_floor: vec![0.; n],
        provinces: vec![[0.; 3]; n],
        range: vec![0.; n],
        plateau_cap: vec![0.; n],
        subsidence: vec![0.; n],
        rift: vec![0.; n],
        glacial_noise: vec![1.; n],
    };
    let basins = noise.layer(7);

    for i in 0..n {
        let (x, z) = (i % size, i / size);
        // Position relative to the continent center, in cells.
        let p = Vec2::new(x as f32 - half, z as f32 - half);
        let border = x.min(z).min(size - 1 - x).min(size - 1 - z) as f32;

        // Coastline: a warped disc with noisy edges, always sea near the borders.
        let q = p + coast.warp(p / 140., 3, 70.);
        let radial = 1. - q.length() / (half * settings.land_radius);
        let land = radial + coast.fbm(q / 110., 5) * 0.45;
        let land = smoothstep(0., 0.3, land) * smoothstep(4., 24., border);
        shape.land[i] = land;
        shape.base_level[i] = land < 0.02;
        // Deeper away from the coast (`radial` keeps decreasing offshore).
        shape.sea_floor[i] = -settings.sea_depth * smoothstep(0., -0.25, radial.min(land));

        // Plates: warped Voronoi, blended softly between neighbouring provinces.
        let w = p + warp.warp(p / 160., 3, 70.) + warp.layer(1).warp(p / 50., 2, 15.);
        let mut distances: Vec<(f32, usize)> = plates
            .iter()
            .enumerate()
            .map(|(k, plate)| (plate.center.distance(w), k))
            .collect();
        distances.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (d1, a) = distances[0];
        let mut weights = [0f32; 3];
        let mut total = 0.;
        let mut cap = 0.;
        for &(d, k) in distances.iter().take(4) {
            let weight = (-(d - d1) / settings.province_blend).exp();
            weights[plates[k].kind] += weight;
            cap += plates[k].cap * weight;
            total += weight;
        }
        let provinces = weights.map(|weight| weight / total);
        shape.provinces[i] = provinces;
        shape.plateau_cap[i] = cap / total;

        // Boundaries of the nearest plate with its neighbours. `(d_k - d1) / 2` is about the
        // distance to the boundary with plate k, and stays continuous where the nearest plate
        // changes.
        let width = settings.range_width * (0.8 + 0.45 * peaks.layer(1).fbm(w / 120., 2));
        let (mut belt, mut foothills, mut rift, mut across) = (0f32, 0f32, 0f32, 0f32);
        let (mut rift_floor, mut shoulders) = (0f32, 0f32);
        for &(dk, k) in &distances[1..4] {
            let x = (dk - d1) / 2.;
            let axis = (plates[k].center - plates[a].center).normalize_or_zero();
            let convergence = (plates[a].velocity - plates[k].velocity).dot(axis);
            let collision = smoothstep(0.45, 1.1, convergence);
            // Ranges: a broad belt of uplift along converging boundaries...
            let here = collision * smoothstep(1., 0.15, x / width);
            if here > belt {
                (belt, across) = (here, x);
            }
            foothills = foothills.max(collision * smoothstep(2.2, 0.6, x / width));
            // ...and rift basins along diverging ones.
            let divergence = smoothstep(-0.2, -0.8, convergence);
            rift = rift.max(divergence * (-(x / (width * 0.6)).powi(2)).exp());
            // The rift floor (a narrow graben) sinks; its shoulders rise into escarpments.
            rift_floor = rift_floor.max(divergence * (-(x / (width * RIFT_FLOOR)).powi(2)).exp());
            shoulders = shoulders
                .max(divergence * (-((x - width * RIFT_SHOULDER) / (width * 0.25)).powi(2)).exp());
        }
        // ...made of several parallel ridges (crests every `ridge_spacing` cells across the
        // belt, wavy), broken into peaks and passes along them.
        let phase = (across + 0.25 * settings.ridge_spacing * peaks.layer(2).fbm(w / 50., 3))
            / settings.ridge_spacing;
        let parallel = (0.5 + 0.5 * (phase * std::f32::consts::TAU).cos()).powf(1.5);
        let summits = peaks.ridged(w / 32., 4);
        let range = belt * (0.3 + 0.7 * (0.5 * parallel + 0.5 * summits));
        let foothills = foothills * (0.5 + 0.5 * hills_noise.layer(5).fbm(w / 30., 3).abs());
        shape.range[i] = (belt + foothills * 0.4).min(1.);

        // Province uplift, rock hardness and hillslope diffusion.
        let [plains, hills, plateau] = provinces;
        let rolling = 0.6 + 0.4 * hills_noise.fbm(w / 55., 4);
        let undulation = 0.5 + 0.5 * hills_noise.layer(3).fbm(w / 35., 3);
        let interior = plains * 0.2 * undulation + hills * 0.9 * rolling + plateau * 1.0;
        let uplift = (interior * (1. - 0.85 * rift) * (1. - belt * 0.5)
            + range * 1.1
            + foothills * 0.22
            + shoulders * RIFT_SHOULDER_UPLIFT)
            * land.powf(1.5);
        shape.uplift[i] = uplift;
        // Rift floors sink in segments along the axis (a chain of basins), plains in scattered
        // kettle hollows.
        let segments = smoothstep(-0.1, 0.35, basins.fbm(w / 45., 2));
        let kettles = plains
            * (1. - belt)
            * smoothstep(KETTLE_THRESHOLD, KETTLE_THRESHOLD + 0.12, basins.layer(1).fbm(w / 9., 2))
            * smoothstep(0.3, 0.6, basins.layer(2).fbm(w / 120., 2) + 0.5);
        shape.subsidence[i] =
            (rift_floor * segments * RIFT_SUBSIDENCE + kettles * KETTLE_SUBSIDENCE) * land;
        shape.rift[i] = rift_floor;
        shape.glacial_noise[i] = 1. + 0.6 * basins.layer(3).fbm(w / 14., 2);
        shape.erodibility[i] = plains * 1.2 + hills * 1.3 + plateau * 0.12;
        shape.erodibility[i] = shape.erodibility[i] * (1. - belt) + 0.75 * belt;
        shape.diffusion[i] = settings.diffusion
            * (plains * 0.6 + hills * 2.5 + plateau * 0.15)
            * (1. - belt * 0.4);

        shape.heights[i] = if shape.base_level[i] {
            0.
        } else {
            land * (0.5 + surface.fbm(p / 40., 4) * 0.3)
        };
    }

    shape.rain = rainfall(settings, &shape, noise);
    shape
}

fn make_plates(settings: &WorldgenSettings, half: f32, rng: &mut impl Rng) -> Vec<Plate> {
    let count = settings.plate_count.max(3);
    // Province types: every type at least once, the rest by weight.
    let weights = settings.province_weights;
    let mut kinds: Vec<usize> = vec![0, 1, 2];
    while kinds.len() < count {
        let roll = rng.random_range(0.0..weights.iter().sum::<f32>());
        let mut acc = 0.;
        let kind = (0..3)
            .find(|&k| {
                acc += weights[k];
                roll < acc
            })
            .unwrap_or(1);
        kinds.push(kind);
    }
    for i in (1..kinds.len()).rev() {
        kinds.swap(i, rng.random_range(0..=i));
    }
    kinds
        .into_iter()
        .map(|kind| {
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let radius = half * settings.land_radius * rng.random_range(0.0..1.0f32).sqrt();
            Plate {
                center: Vec2::from_angle(angle) * radius,
                velocity: Vec2::from_angle(rng.random_range(0.0..std::f32::consts::TAU))
                    * rng.random_range(0.4..1.0f32),
                kind,
                cap: settings.plateau_level * rng.random_range(0.7..1.15f32),
            }
        })
        .collect()
}

/// Orographic rain: moist air comes from the sea along the prevailing wind, and rains out as
/// it is pushed up over high ground, leaving dry rain shadows downwind of ranges.
///
/// `elevation` is any height-like field (uplift before erosion, terrain after).
pub fn rain_from(settings: &WorldgenSettings, elevation: &[f32], sea: &[bool], scale: f32) -> Vec<f32> {
    let size = settings.size;
    let wind = Vec2::from_angle(settings.wind_angle.unwrap_or(0.));
    let mut order: Vec<usize> = (0..size * size).collect();
    let projection =
        |i: usize| wind.dot(Vec2::new((i % size) as f32, (i / size) as f32));
    order.sort_by(|&a, &b| projection(a).total_cmp(&projection(b)));

    // Rain responds to the large-scale relief, not to every small bump.
    let elevation = &blur(size, elevation, 4);
    let mut humidity = vec![1f32; size * size];
    let mut rain = vec![0f32; size * size];
    let sample = |field: &[f32], p: Vec2| {
        let max = size as i64 - 1;
        let at = |x: i64, z: i64| field[z.clamp(0, max) as usize * size + x.clamp(0, max) as usize];
        let (c, t) = (p.floor(), p - p.floor());
        let (x, z) = (c.x as i64, c.y as i64);
        let bottom = at(x, z) + (at(x + 1, z) - at(x, z)) * t.x;
        let top = at(x, z + 1) + (at(x + 1, z + 1) - at(x, z + 1)) * t.x;
        bottom + (top - bottom) * t.y
    };
    for i in order {
        if sea[i] {
            humidity[i] = 1.;
            continue;
        }
        let p = Vec2::new((i % size) as f32, (i / size) as f32);
        let upwind = p - wind;
        // Air mixes sideways as it goes, so rain shadows don't leave sharp streaks.
        let side = wind.perp() * 1.5;
        let incoming = 0.5 * sample(&humidity, upwind)
            + 0.25 * (sample(&humidity, upwind + side) + sample(&humidity, upwind - side));
        let rise = (elevation[i] - sample(elevation, upwind)).max(0.) * scale;
        let fallen = incoming * (settings.base_rain + settings.orographic_rain * rise).min(1.);
        rain[i] = fallen;
        // Some of the rain evaporates back (vegetation, rivers), keeping the interior wetter.
        humidity[i] = incoming - fallen * (1. - settings.land_evaporation);
    }
    // Normalize to a mean of 1 on land.
    let rain = blur(size, &rain, 3);
    let land: Vec<f32> = rain.iter().zip(sea).filter(|(_, s)| !**s).map(|(r, _)| *r).collect();
    let mean = (land.iter().sum::<f32>() / land.len().max(1) as f32).max(1e-6);
    rain.iter().map(|r| (r / mean).clamp(0.05, 4.)).collect()
}

fn rainfall(settings: &WorldgenSettings, shape: &Shape, noise: Noise) -> Vec<f32> {
    // Before erosion, uplift stands in for elevation.
    let rain = rain_from(settings, &shape.uplift, &shape.base_level, 40.);
    // A little noise so rain bands aren't perfectly straight.
    let size = settings.size;
    rain.iter()
        .enumerate()
        .map(|(i, r)| {
            let p = Vec2::new((i % size) as f32, (i / size) as f32);
            r * (1. + 0.25 * noise.layer(6).fbm(p / 70., 3))
        })
        .collect()
}

/// Rift valleys: floor width and shoulder position (fractions of the range belt width), shoulder
/// uplift and floor subsidence (relative, like uplift).
const RIFT_FLOOR: f32 = 0.35;
const RIFT_SHOULDER: f32 = 0.8;
const RIFT_SHOULDER_UPLIFT: f32 = 0.35;
const RIFT_SUBSIDENCE: f32 = 0.5;
/// Kettle hollows on plains: noise threshold (higher: fewer) and subsidence.
const KETTLE_THRESHOLD: f32 = 0.42;
const KETTLE_SUBSIDENCE: f32 = 0.12;
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

/// Stage B: stream-power-law erosion with an implicit solver (Braun & Willett 2013) and hillslope
/// diffusion. Returns heights in the simulation's relative units (scaled later).
///
/// Every step: flow is routed through the depressions without filling them (basin graph,
/// [`FlowGraph::with_depressions`]) by steepest descent with random slope weights (stochastic
/// D8), the water surface standing at each basin's spill point,
/// drainage accumulated from the rain, then `dh/dt = U - K A^m S` is solved implicitly from the
/// outlets upstream (rivers flowing into a lake erode down to its surface), and `D ∇²h` smooths
/// hillslopes. Depressions are lakes: no incision under them, and sediment fills a share of
/// their depth each step, so only basins that keep sinking (or are dug by glaciers) hold one.
/// Glaciers overdeepen high valleys during the last steps.
///
/// Returns heights in the simulation's relative units (scaled later), with their lake basins.
pub fn erode(grid: &impl Grid, settings: &WorldgenSettings, shape: &Shape) -> Vec<f32> {
    erode_timed(grid, settings, shape).0
}

/// Time spent in each part of [`erode`], summed over its steps.
#[derive(Default, Debug)]
pub struct ErosionTimings {
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
pub fn erode_timed(
    grid: &impl Grid,
    settings: &WorldgenSettings,
    shape: &Shape,
) -> (Vec<f32>, ErosionTimings) {
    let mut timings = ErosionTimings::default();
    let mut heights = shape.heights.clone();
    let dt = settings.erosion_dt;
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
    let max_k =
        shape.diffusion.iter().fold(0f32, |a, &d| a.max(d)) * dt / settings.cell_size.powi(2);
    let mut previous = heights.clone();
    for step in 0..settings.erosion_steps {
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

        for &i in &graph.order {
            if shape.base_level[i] {
                continue;
            }
            let r = graph.receivers[i];
            let uplifted = heights[i] + (shape.uplift[i] - shape.subsidence[i]) * dt;
            // Under a lake: still water, sediment settling.
            let depth = water[i] - uplifted;
            if depth > 1e-3 {
                heights[i] = uplifted + depth * LAKE_FILL;
                continue;
            }
            if r == i {
                heights[i] = uplifted;
                continue;
            }
            let f = settings.erodibility
                * shape.erodibility[i]
                * dt
                * area[i].powf(settings.area_exponent)
                / (graph.receiver_distance[i] * settings.cell_size);
            heights[i] = (uplifted + f * water[r]) / (1. + f);
        }
        lap(&mut timings.incision);

        // Glaciers in the high valleys, digging unevenly along their course (overdeepening).
        if step + GLACIAL_STEPS >= settings.erosion_steps {
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
            let ice: Vec<f32> = (0..heights.len())
                .map(|i| {
                    let height = (heights[i] - high) / (top - high);
                    smoothstep(GLACIAL_HEIGHT.0, GLACIAL_HEIGHT.1, height)
                        * smoothstep(GLACIAL_AREA.0, GLACIAL_AREA.1, area[i])
                        * shape.range[i].min(1.)
                })
                .collect();
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
        for _ in 0..substeps as usize {
            previous.copy_from_slice(&heights);
            for (c, &i) in diffusing.iter().enumerate() {
                let i = i as usize;
                let k = shape.diffusion[i] * dt / substeps / settings.cell_size.powi(2);
                let sides = &sides[side_start[c] as usize..side_start[c + 1] as usize];
                let sum: f32 = sides.iter().map(|&j| previous[j as usize]).sum();
                heights[i] += k * (sum - sides.len() as f32 * previous[i]);
            }
        }
        lap(&mut timings.diffusion);
    }
    (heights, timings)
}

/// Plateaus: cut the high ground of plateau provinces down to their cap level (a resistant
/// caprock), keeping the canyons below it: tablelands with escarpments.
pub fn cap_plateaus(shape: &Shape, heights: &mut [f32]) {
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

/// Shores: land rises gradually from the water line (gentle on plains, steeper by ranges) and
/// the sea floor slopes down from it as a shelf, so the coast never drops straight from the
/// eroded land to the sea floor. The whole land also rises gently inland, so lowlands away
/// from the coast stand a few units above the sea.
///
/// Returns the distance to the shore (cells, negative at sea), smoothed so the shore doesn't
/// follow the cell staircase of the land mask.
pub fn shape_coast(
    settings: &WorldgenSettings,
    shape: &Shape,
    noise: Noise,
    heights: &mut [f32],
) -> Vec<f32> {
    let size = settings.size;
    let to_sea = distance_field(size, &shape.base_level);
    let shore = shore_distance(size, &shape.base_level);
    let variation = noise.layer(30);
    for (i, h) in heights.iter_mut().enumerate() {
        let p = Vec2::new((i % size) as f32, (i / size) as f32);
        let d = shore[i];
        if shape.base_level[i] {
            // A shallow shelf, then the open sea floor.
            *h = h.max(0.15 + 0.35 * (d - 0.5)).min(-0.05);
        } else {
            let slope = (0.25 + 0.3 * (0.5 + 0.5 * variation.fbm(p / 60., 3)) + 0.5 * shape.range[i])
                * settings.cell_size;
            let ramp = 0.15 + slope * (d - 0.5);
            let inland = settings.inland_rise * smoothstep(0., 100., to_sea[i]);
            // Land cells may end up just under water: the shore follows `shore`, not the mask.
            *h = smooth_min(*h + inland, ramp, 1.).max(-0.3);
        }
    }
    shore
}

/// Distance to the shore (cells, positive on land, negative at sea), blurred so it doesn't
/// follow the cell staircase of the `sea` mask.
pub fn shore_distance(size: usize, sea: &[bool]) -> Vec<f32> {
    let to_sea = distance_field(size, sea);
    let land: Vec<bool> = sea.iter().map(|sea| !sea).collect();
    let to_land = distance_field(size, &land);
    let signed: Vec<f32> = to_sea.iter().zip(&to_land).map(|(s, l)| s - l).collect();
    blur(size, &blur(size, &signed, 2), 2)
}

/// Collapse slopes steeper than the rock can hold (thermal erosion): material slides from
/// higher cells to their lower neighbours. Ranges hold steeper slopes, plateau escarpments the
/// steepest.
pub fn limit_slopes(settings: &WorldgenSettings, shape: &Shape, heights: &mut [f32]) {
    let size = settings.size;
    let talus: Vec<f32> = (0..size * size)
        .map(|i| {
            let plateau = shape.provinces[i][2] * (1. - shape.range[i]);
            (0.45 + 0.25 * shape.range[i] + 0.8 * plateau) * settings.cell_size
        })
        .collect();
    for _ in 0..200 {
        let mut delta = vec![0f32; heights.len()];
        for i in 0..heights.len() {
            if shape.base_level[i] {
                continue;
            }
            for (j, distance) in neighbours(size, i) {
                let excess = heights[i] - heights[j] - talus[i] * distance;
                if excess > 0. {
                    // A small fraction per step and neighbour keeps it stable.
                    let moved = excess * 0.04;
                    delta[i] -= moved;
                    if !shape.base_level[j] {
                        delta[j] += moved;
                    }
                }
            }
        }
        for (h, d) in heights.iter_mut().zip(delta) {
            *h += d;
        }
    }
}

/// Remove cell-sized noise left by erosion (flow routing on near-flat ground leaves a checker
/// pattern): blend land toward a 3×3 binomial blur.
pub fn smooth_cells(settings: &WorldgenSettings, shape: &Shape, heights: &mut [f32]) {
    let size = settings.size;
    let source = heights.to_vec();
    let at = |x: usize, z: usize| source[z * size + x];
    for z in 1..size - 1 {
        for x in 1..size - 1 {
            let i = z * size + x;
            if shape.base_level[i] {
                continue;
            }
            let row = |z: usize| at(x - 1, z) + 2. * at(x, z) + at(x + 1, z);
            let blurred = (row(z - 1) + 2. * row(z) + row(z + 1)) / 16.;
            heights[i] = 0.3 * source[i] + 0.7 * blurred;
        }
    }
}

/// Distance (cells) from each cell to the nearest cell of `mask` (chamfer approximation).
pub fn distance_field(size: usize, mask: &[bool]) -> Vec<f32> {
    let mut distance: Vec<f32> = mask.iter().map(|&m| if m { 0. } else { f32::MAX }).collect();
    let diagonal = std::f32::consts::SQRT_2;
    let forward = [(-1, 0, 1.), (0, -1, 1.), (-1, -1, diagonal), (1, -1, diagonal)];
    let backward = [(1, 0, 1.), (0, 1, 1.), (1, 1, diagonal), (-1, 1, diagonal)];
    let mut relax = |x: usize, z: usize, offsets: &[(i64, i64, f32)]| {
        let i = z * size + x;
        for &(dx, dz, cost) in offsets {
            let (nx, nz) = (x as i64 + dx, z as i64 + dz);
            if nx >= 0 && nz >= 0 && (nx as usize) < size && (nz as usize) < size {
                let candidate = distance[nz as usize * size + nx as usize] + cost;
                distance[i] = distance[i].min(candidate);
            }
        }
    };
    for z in 0..size {
        for x in 0..size {
            relax(x, z, &forward);
        }
    }
    for z in (0..size).rev() {
        for x in (0..size).rev() {
            relax(x, z, &backward);
        }
    }
    distance
}

/// Polynomial smooth minimum: blends `a` and `b` over a band of `k` world units.
fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = (k - (a - b).abs()).max(0.) / k;
    a.min(b) - h * h * k / 4.
}

/// Separable box blur of radius `radius` (cells).
pub fn blur(size: usize, values: &[f32], radius: usize) -> Vec<f32> {
    let pass = |input: &[f32], horizontal: bool| -> Vec<f32> {
        let mut output = vec![0.; input.len()];
        for line in 0..size {
            let at = |k: usize| {
                if horizontal {
                    input[line * size + k]
                } else {
                    input[k * size + line]
                }
            };
            let mut sum: f32 = (0..=radius.min(size - 1)).map(at).sum();
            let mut count = radius.min(size - 1) + 1;
            for k in 0..size {
                let index = if horizontal { line * size + k } else { k * size + line };
                output[index] = sum / count as f32;
                if k + radius + 1 < size {
                    sum += at(k + radius + 1);
                    count += 1;
                }
                if k >= radius {
                    sum -= at(k - radius);
                    count -= 1;
                }
            }
        }
        output
    };
    pass(&pass(values, true), false)
}
