//! The planet (milestone 1 of `docs/living-world.md`): generation on the cube-sphere grid.
//!
//! A first version, with the flat map's approach carried onto the sphere: continents from
//! noise, rigid plates whose converging boundaries raise mountain belts (made of parallel,
//! broken ridges) and whose diverging ones open rifts, then the same stream power erosion as
//! the flat map (`terrain::erode`, run on the [`SphereGrid`]). Lengths are in cells, as on
//! the flat map, so the landforms keep their size; the sphere's radius is `2n/π` cells.

use std::time::{Duration, Instant};

use bevy_math::Vec3;
use rand::{RngExt, SeedableRng, rngs::StdRng};

use crate::{
    WorldgenSettings,
    flow::{FlowGraph, priority_flood},
    grid::Grid,
    noise::{Noise, smoothstep},
    sphere::SphereGrid,
    terrain::{self, Shape},
};

/// Frequency of the continent noise on the unit sphere (higher: more, smaller continents).
const CONTINENT_FREQUENCY: f32 = 1.6;

/// Planet generation parameters.
#[derive(Clone, Debug)]
pub struct PlanetSettings {
    pub seed: u64,
    /// Cells along a cube face edge (the planet has `6 n²` cells).
    pub n: usize,
    /// Share of the surface above the sea.
    pub land_fraction: f32,
    /// Number of tectonic plates.
    pub plate_count: usize,
    /// Width of mountain belts and spacing of their parallel ridges (cells).
    pub range_width: f32,
    pub ridge_spacing: f32,
    /// Height of the highest ranges (99th percentile of range summits) and depth of the open
    /// sea (world units, one per cell like the flat map).
    pub max_height: f32,
    pub sea_depth: f32,
    /// Erosion: steps, time step, erodibility, drainage area exponent, hillslope diffusion,
    /// routing jitter (as in [`WorldgenSettings`]).
    pub erosion_steps: usize,
    pub erosion_dt: f32,
    pub erodibility: f32,
    pub area_exponent: f32,
    pub diffusion: f32,
    pub routing_jitter: f32,
}

impl Default for PlanetSettings {
    fn default() -> Self {
        let flat = WorldgenSettings::default();
        Self {
            seed: 1082,
            n: 256,
            land_fraction: 0.3,
            plate_count: 14,
            range_width: flat.range_width,
            ridge_spacing: flat.ridge_spacing,
            max_height: flat.max_height,
            sea_depth: flat.sea_depth,
            erosion_steps: flat.erosion_steps,
            erosion_dt: flat.erosion_dt,
            erodibility: flat.erodibility,
            area_exponent: flat.area_exponent,
            diffusion: flat.diffusion,
            routing_jitter: flat.routing_jitter,
        }
    }
}

/// A generated planet.
pub struct Planet {
    pub settings: PlanetSettings,
    pub grid: SphereGrid,
    /// Height of each cell (world units; the sea is at 0).
    pub heights: Vec<f32>,
    /// Sea cells.
    pub sea: Vec<bool>,
    /// Mountain belts (0..1).
    pub range: Vec<f32>,
    /// Upstream drainage area of each cell (in mean cell areas).
    pub drainage: Vec<f32>,
    /// Plate of each cell.
    pub plate: Vec<u16>,
    /// Duration of each generation stage.
    pub timings: Vec<(&'static str, Duration)>,
}

impl Planet {
    /// The planet's radius, in cells.
    pub fn radius(&self) -> f32 {
        2. * self.settings.n as f32 / std::f32::consts::PI
    }
}

/// A rigid plate: its centre, and its rotation (axis × angular speed, in radians per unit
/// time) about the planet's centre.
struct Plate {
    center: Vec3,
    rotation: Vec3,
}

impl Plate {
    /// Surface velocity at a point of the unit sphere.
    fn velocity(&self, p: Vec3) -> Vec3 {
        self.rotation.cross(p)
    }
}

/// A uniformly random unit vector.
fn random_direction(rng: &mut StdRng) -> Vec3 {
    loop {
        let v = Vec3::new(
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
        );
        let length = v.length();
        if length > 0.1 && length <= 1. {
            return v / length;
        }
    }
}

pub fn generate_planet(settings: &PlanetSettings) -> Planet {
    let mut timings = Vec::new();
    let mut stage = Instant::now();
    let mut lap = |name, timings: &mut Vec<(&'static str, Duration)>| {
        timings.push((name, stage.elapsed()));
        stage = Instant::now();
    };
    let grid = SphereGrid::new(settings.n);
    let cells = grid.len();
    let radius = 2. * settings.n as f32 / std::f32::consts::PI;
    let noise = Noise::new(settings.seed as u32);
    let mut rng = StdRng::seed_from_u64(settings.seed);

    // Continents: low-frequency noise, cut at the level that leaves `land_fraction` above.
    // They are planet-scale features, sized relative to the radius (everything else is sized
    // in cells, like the flat map's landforms).
    let continents = noise.layer(1);
    let continent_value: Vec<f32> = (0..cells)
        .map(|i| {
            let d = grid.direction(i);
            let w = d + continents.warp3(d * 2.2, 3, 0.18);
            continents.fbm3(w * CONTINENT_FREQUENCY, 6)
        })
        .collect();
    let mut sorted = continent_value.clone();
    sorted.sort_by(f32::total_cmp);
    let coast = sorted[((1. - settings.land_fraction) * (cells - 1) as f32) as usize];
    let land: Vec<f32> = continent_value
        .iter()
        .map(|&c| smoothstep(coast, coast + 0.08, c))
        .collect();
    let sea: Vec<bool> = continent_value.iter().map(|&c| c < coast).collect();

    // Plates: random centres (boundaries warped), each turning about a random axis.
    let plates: Vec<Plate> = (0..settings.plate_count)
        .map(|_| Plate {
            center: random_direction(&mut rng),
            rotation: random_direction(&mut rng) * rng.random_range(0.5..1.5),
        })
        .collect();
    let peaks = noise.layer(2);
    let hills = noise.layer(3);
    let warp = noise.layer(4);
    let mut shape = Shape {
        land: land.clone(),
        uplift: vec![0.; cells],
        erodibility: vec![1.; cells],
        diffusion: vec![0.; cells],
        rain: (0..cells).map(|i| grid.area(i)).collect(),
        base_level: sea.clone(),
        heights: vec![0.; cells],
        sea_floor: vec![0.; cells],
        provinces: vec![[0.; 3]; cells],
        range: vec![0.; cells],
        plateau_cap: vec![0.; cells],
        subsidence: vec![0.; cells],
        rift: vec![0.; cells],
        glacial_noise: vec![1.; cells],
    };
    let mut plate_of = vec![0u16; cells];
    for i in 0..cells {
        let direction = grid.direction(i);
        let p = direction * radius;
        // Plate boundaries wander: plates are found from a warped position.
        let w = (p + warp.warp3(p / 180., 3, 45.)).normalize();
        let mut distances: Vec<(f32, usize)> = plates
            .iter()
            .enumerate()
            .map(|(k, plate)| (plate.center.angle_between(w) * radius, k))
            .collect();
        distances.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (d1, a) = distances[0];
        plate_of[i] = a as u16;
        let width = settings.range_width * (0.8 + 0.45 * peaks.layer(1).fbm3(p / 120., 2));
        let (mut belt, mut foothills, mut rift, mut across) = (0f32, 0f32, 0f32, 0f32);
        for &(dk, k) in &distances[1..4] {
            // About the distance to the boundary with plate k (cells).
            let x = (dk - d1) / 2.;
            // Across the boundary, from plate a towards plate k, in the tangent plane.
            let axis = (plates[k].center - plates[a].center).reject_from(direction).normalize_or_zero();
            let convergence = (plates[a].velocity(direction) - plates[k].velocity(direction)).dot(axis);
            let collision = smoothstep(0.45, 1.1, convergence);
            let here = collision * smoothstep(1., 0.15, x / width);
            if here > belt {
                (belt, across) = (here, x);
            }
            foothills = foothills.max(collision * smoothstep(2.2, 0.6, x / width));
            let divergence = smoothstep(-0.2, -0.8, convergence);
            rift = rift.max(divergence * (-(x / (width * 0.6)).powi(2)).exp());
        }
        // Parallel ridges across the belt, broken into peaks and passes.
        let phase = (across + 0.25 * settings.ridge_spacing * peaks.layer(2).fbm3(p / 50., 3))
            / settings.ridge_spacing;
        let parallel = (0.5 + 0.5 * (phase * std::f32::consts::TAU).cos()).powf(1.5);
        let summits = peaks.ridged3(p / 32., 4);
        let range = belt * (0.3 + 0.7 * (0.5 * parallel + 0.5 * summits));
        let foothills = foothills * (0.5 + 0.5 * hills.layer(5).fbm3(p / 30., 3).abs());
        shape.range[i] = (belt + foothills * 0.4).min(1.);

        // Interior provinces: rolling hills where the noise is high, plains elsewhere.
        let hilly = smoothstep(-0.1, 0.35, hills.fbm3(p / 160., 3));
        let rolling = 0.6 + 0.4 * hills.layer(1).fbm3(p / 55., 4);
        let undulation = 0.5 + 0.5 * hills.layer(3).fbm3(p / 35., 3);
        let interior = (1. - hilly) * 0.2 * undulation + hilly * 0.9 * rolling;
        let l = land[i];
        shape.uplift[i] = (interior * (1. - 0.85 * rift) * (1. - belt * 0.5)
            + range * 1.1
            + foothills * 0.22)
            * l.powf(1.5);
        shape.provinces[i] = [1. - hilly, hilly, 0.];
        shape.erodibility[i] = ((1. - hilly) * 1.2 + hilly * 1.3) * (1. - belt) + 0.75 * belt;
        shape.diffusion[i] =
            settings.diffusion * ((1. - hilly) * 0.6 + hilly * 2.5) * (1. - belt * 0.4);
        shape.glacial_noise[i] = 1. + 0.6 * hills.layer(7).fbm3(p / 14., 2);
        shape.heights[i] = if sea[i] { 0. } else { l * (0.5 + hills.layer(8).fbm3(p / 40., 4) * 0.3) };
    }
    lap("shape", &mut timings);

    // Erosion, as on the flat map.
    let erosion = WorldgenSettings {
        seed: settings.seed,
        erosion_steps: settings.erosion_steps,
        erosion_dt: settings.erosion_dt,
        erodibility: settings.erodibility,
        area_exponent: settings.area_exponent,
        diffusion: settings.diffusion,
        routing_jitter: settings.routing_jitter,
        ..Default::default()
    };
    let mut heights = terrain::erode(&grid, &erosion, &shape);
    lap("erosion", &mut timings);

    // World units: the ranges' summits set the scale; the sea floor deepens away from land.
    let mut summits: Vec<f32> = (0..cells)
        .filter(|&i| !sea[i] && shape.range[i] > 0.5)
        .map(|i| heights[i])
        .collect();
    if summits.is_empty() {
        summits = heights.clone();
    }
    summits.sort_by(f32::total_cmp);
    let reference = summits[(summits.len() - 1) * 99 / 100].max(1e-6);
    let to_land = distance_to(&grid, &sea.iter().map(|s| !s).collect::<Vec<_>>());
    let floor = noise.layer(9);
    for i in 0..cells {
        heights[i] = if sea[i] {
            let p = grid.direction(i) * radius;
            let depth = 0.3 + settings.sea_depth * smoothstep(0., 40., to_land[i]);
            -depth * (0.85 + 0.15 * floor.fbm3(p / 60., 3))
        } else {
            heights[i] * settings.max_height / reference
        };
    }
    let filled = priority_flood(&grid, &heights, &sea, 1e-4);
    let drainage = FlowGraph::new(&grid, &filled, &sea).accumulate(|i| grid.area(i));
    lap("drainage", &mut timings);

    Planet {
        settings: settings.clone(),
        grid,
        heights,
        sea,
        range: shape.range,
        drainage,
        plate: plate_of,
        timings,
    }
}

/// Distance (in cells, along neighbour steps) from each cell to the nearest cell of `mask`
/// (Dijkstra).
pub fn distance_to(grid: &impl Grid, mask: &[bool]) -> Vec<f32> {
    use std::{cmp::Reverse, collections::BinaryHeap};
    let mut distance = vec![f32::MAX; grid.len()];
    let mut open = BinaryHeap::new();
    for i in (0..grid.len()).filter(|&i| mask[i]) {
        distance[i] = 0.;
        open.push(Reverse((0u32, i)));
    }
    // (Distances as fixed point, so the heap orders integers.)
    while let Some(Reverse((d, i))) = open.pop() {
        let d = d as f32 / 1024.;
        if d > distance[i] {
            continue;
        }
        for (j, step) in grid.neighbours(i) {
            let candidate = d + step;
            if candidate < distance[j] {
                distance[j] = candidate;
                open.push(Reverse(((candidate * 1024.) as u32, j)));
            }
        }
    }
    distance
}
