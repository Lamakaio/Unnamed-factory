//! The planet (milestone 1 of `docs/living-world.md`): generation on the cube-sphere grid.
//!
//! Plates are rigid and either **continental or oceanic**. Continents ride on the continental
//! plates: where a continent ends inside its plate the margin is passive (a wide shelf,
//! lowlands); where it reaches a plate boundary the margin is active and the boundary shapes
//! it. At converging boundaries, continent against continent raises great ranges, an ocean
//! plate diving under a continent raises a coastal cordillera with a trench offshore, and
//! ocean against ocean makes an island arc. Diverging boundaries open rifts in continents and
//! mid-ocean ridges in the sea floor. Inside continents, old sutures between ancient blocks are
//! worn-down ranges: the older, the lower and rounder (more hillslope diffusion, standing in
//! for a long erosion history). The interiors are provinces of plains, broad hills and
//! plateaus (plateaus away from the coasts, lowlands toward them). Then the same stream power
//! erosion as the flat map (`terrain::erode`, on the [`SphereGrid`]).
//!
//! Lengths are in cells, as on the flat map, so the landforms keep their size; the sphere's
//! radius is `2n/π` cells. Planet-scale features (continents) are sized relative to it.

use std::time::{Duration, Instant};

use bevy_math::Vec3;
use rand::{RngExt, SeedableRng, rngs::StdRng};

use crate::{
    WorldgenSettings,
    flow::{FlowGraph, priority_flood},
    grid::Grid,
    noise::{Noise, hash, smoothstep},
    sphere::SphereGrid,
    terrain::{self, Shape},
};

/// Frequency of the continent outline noise on the unit sphere.
const CONTINENT_FREQUENCY: f32 = 2.4;
/// How much a plate's crust raises (continental) or lowers (oceanic) the continent outline.
const CONTINENTAL_BIAS: f32 = 0.22;
/// Distance over which the plates' crust bias blends across their boundaries (cells).
const CRUST_BLEND: f32 = 25.;
/// Crust value margin below the coast that is still continental (the shelf).
const SHELF_BAND: f32 = 0.12;
/// Depth of the continental shelf (world units).
const SHELF_DEPTH: f32 = 1.5;

/// Planet generation parameters.
#[derive(Clone, Debug)]
pub struct PlanetSettings {
    pub seed: u64,
    /// Cells along a cube face edge (the planet has `6 n²` cells).
    pub n: usize,
    /// Share of the surface above the sea.
    pub land_fraction: f32,
    /// Number of tectonic plates, and the share of them that are continental.
    pub plate_count: usize,
    pub continental_plates: f32,
    /// Mean area of a province (cells), and the share of each province type
    /// (`[plains, hills, plateau]`) and how softly they blend (cells), as on the flat map.
    pub province_cells: f32,
    pub province_weights: [f32; 3],
    pub province_blend: f32,
    /// Level of plateau caps (world units).
    pub plateau_level: f32,
    /// Mean area of the ancient blocks whose sutures are old, worn ranges (cells), and the
    /// share of sutures that are ranges.
    pub craton_cells: f32,
    pub old_range_share: f32,
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
            plate_count: 12,
            continental_plates: 0.4,
            province_cells: 8_000.,
            province_weights: [0.35, 0.45, 0.2],
            province_blend: flat.province_blend,
            plateau_level: flat.plateau_level,
            craton_cells: 20_000.,
            old_range_share: 0.5,
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
    /// Continental crust (land and shelves).
    pub continental: Vec<bool>,
    /// Mountain belts (0..1), young and old.
    pub range: Vec<f32>,
    /// Upstream drainage area of each cell (in mean cell areas).
    pub drainage: Vec<f32>,
    /// Plate of each cell, and whether each plate is continental.
    pub plate: Vec<u16>,
    pub continental_plate: Vec<bool>,
    /// Duration of each generation stage.
    pub timings: Vec<(&'static str, Duration)>,
}

impl Planet {
    /// The planet's radius, in cells.
    pub fn radius(&self) -> f32 {
        2. * self.settings.n as f32 / std::f32::consts::PI
    }
}

/// A rigid plate: its centre, its rotation (axis × angular speed, in radians per unit time)
/// about the planet's centre, and its crust.
struct Plate {
    center: Vec3,
    rotation: Vec3,
    continental: bool,
}

impl Plate {
    /// Surface velocity at a point of the unit sphere.
    fn velocity(&self, p: Vec3) -> Vec3 {
        self.rotation.cross(p)
    }
}

/// A region of the surface (province or ancient block): its centre, its type and a value
/// (a province's plateau cap level).
struct Region {
    center: Vec3,
    kind: usize,
    value: f32,
}

/// Points on the sphere, bucketed by the cells of a coarse cube-sphere, to find the nearest
/// ones to a direction quickly.
struct PointIndex {
    coarse: SphereGrid,
    buckets: Vec<Vec<usize>>,
}

impl PointIndex {
    /// `spacing`: about the distance between points, as a fraction of a face's width.
    fn new(points: &[Vec3], spacing: f32) -> Self {
        let m = ((1. / spacing) as usize).clamp(1, 64);
        let coarse = SphereGrid::new(m);
        let mut buckets = vec![Vec::new(); coarse.len()];
        for (k, &p) in points.iter().enumerate() {
            buckets[coarse.cell_at(p)].push(k);
        }
        Self { coarse, buckets }
    }

    /// Points in the coarse cell of `direction` and its neighbours (two rings).
    fn candidates(&self, direction: Vec3, out: &mut Vec<usize>) {
        out.clear();
        let cell = self.coarse.cell_at(direction);
        let mut cells = vec![cell];
        for _ in 0..2 {
            let ring: Vec<usize> = cells.iter().flat_map(|&c| self.coarse.neighbours(c).map(|(j, _)| j)).collect();
            cells.extend(ring);
            cells.sort_unstable();
            cells.dedup();
        }
        for c in cells {
            out.extend(&self.buckets[c]);
        }
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

/// `f` over `0..len`, on all cores.
fn par_map<T: Send>(len: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = len.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..len)
            .step_by(chunk)
            .map(|start| {
                let f = &f;
                scope.spawn(move || (start..(start + chunk).min(len)).map(f).collect::<Vec<T>>())
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

/// The nearest plates to a (warped) direction: `(distance in cells, plate)`, nearest first.
fn nearest_plates(plates: &[Plate], w: Vec3, radius: f32) -> Vec<(f32, usize)> {
    let mut distances: Vec<(f32, usize)> = plates
        .iter()
        .enumerate()
        .map(|(k, plate)| (plate.center.angle_between(w) * radius, k))
        .collect();
    distances.sort_by(|a, b| a.0.total_cmp(&b.0));
    distances.truncate(4);
    distances
}

/// What a plate boundary does near a cell, from the cell's side.
#[derive(Default, Clone, Copy)]
struct Boundary {
    /// Young mountain belt (0..1), and the distance across it to the boundary (cells).
    belt: f32,
    across: f32,
    /// Foothills around belts.
    foothills: f32,
    /// Rift in a continent (subsiding floor, raised shoulders).
    rift: f32,
    shoulders: f32,
    /// Island arc (volcanic islands on the overriding ocean plate).
    arc: f32,
    /// Sea floor: trench in front of a subduction zone, mid-ocean ridge.
    trench: f32,
    ridge: f32,
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

    // Plates: random centres (boundaries warped), each turning about a random axis; a share
    // of them continental.
    let mut plates: Vec<Plate> = (0..settings.plate_count)
        .map(|_| Plate {
            center: random_direction(&mut rng),
            rotation: random_direction(&mut rng) * rng.random_range(0.5..1.5),
            continental: false,
        })
        .collect();
    let continental_count =
        ((settings.plate_count as f32 * settings.continental_plates).round() as usize).max(1);
    for plate in plates.iter_mut().take(continental_count) {
        plate.continental = true;
    }
    let regions = |count: usize, kinds: &[f32], rng: &mut StdRng, value: &dyn Fn(&mut StdRng) -> f32| {
        let total: f32 = kinds.iter().sum();
        (0..count)
            .map(|_| {
                let mut roll = rng.random_range(0.0..1.0f32) * total;
                let kind = kinds
                    .iter()
                    .position(|&w| {
                        roll -= w;
                        roll < 0.
                    })
                    .unwrap_or(kinds.len() - 1);
                Region {
                    center: random_direction(rng),
                    kind,
                    value: value(rng),
                }
            })
            .collect::<Vec<Region>>()
    };
    let plateau_level = settings.plateau_level;
    let provinces = regions(
        ((cells as f32 / settings.province_cells) as usize).max(12),
        &settings.province_weights,
        &mut rng,
        &|rng| plateau_level * rng.random_range(0.7..1.15f32),
    );
    let cratons = regions(
        ((cells as f32 / settings.craton_cells) as usize).max(8),
        &[1.],
        &mut rng,
        &|rng| rng.random_range(0.0..1.0f32),
    );
    let province_index = PointIndex::new(
        &provinces.iter().map(|r| r.center).collect::<Vec<_>>(),
        settings.province_cells.sqrt() / settings.n as f32,
    );
    let craton_index = PointIndex::new(
        &cratons.iter().map(|r| r.center).collect::<Vec<_>>(),
        settings.craton_cells.sqrt() / settings.n as f32,
    );

    let continents = noise.layer(1);
    let peaks = noise.layer(2);
    let hills = noise.layer(3);
    let warp = noise.layer(4);
    let arcs = noise.layer(6);
    let width_at = |p: Vec3| settings.range_width * (0.8 + 0.45 * peaks.layer(1).fbm3(p / 120., 2));

    // Pass 1: plates, boundaries, crust.
    let pass1: Vec<(u16, f32, Boundary)> = par_map(cells, |i| {
        let direction = grid.direction(i);
        let p = direction * radius;
        let w = (p + warp.warp3(p / 180., 3, 45.)).normalize();
        let nearest = nearest_plates(&plates, w, radius);
        let (d1, a) = nearest[0];
        let width = width_at(p);
        // Crust: continental plates carry continents, outlined by noise (so continents end
        // inside their plate on some sides: passive margins); oceanic plates hold the odd
        // microcontinent.
        let outline = continents.fbm3(
            (direction + continents.warp3(direction * 2.2, 3, 0.15)) * CONTINENT_FREQUENCY,
            6,
        );
        // (The plate's crust only biases the outline: coasts mostly follow the noise, and
        // follow plate boundaries only where they cut through a continent.)
        // Blended across boundaries (over `CRUST_BLEND` cells), so the bias has no step.
        let bias = |k: usize| if plates[k].continental { CONTINENTAL_BIAS } else { -CONTINENTAL_BIAS };
        let (mut base, mut total) = (0., 0.);
        for &(d, k) in &nearest {
            let weight = (-(d - d1) / CRUST_BLEND).exp();
            base += bias(k) * weight;
            total += weight;
        }
        let base = base / total;
        let mut boundary = Boundary::default();
        let here_continental = base + outline > 0.;
        for &(dk, k) in &nearest[1..] {
            let x = (dk - d1) / 2.;
            let axis = (plates[k].center - plates[a].center).reject_from(direction).normalize_or_zero();
            let convergence =
                (plates[a].velocity(direction) - plates[k].velocity(direction)).dot(axis);
            let collision = smoothstep(0.4, 1.0, convergence);
            let divergence = smoothstep(-0.2, -0.8, convergence);
            let there_continental = plates[k].continental;
            let near = |scale: f32| smoothstep(1., 0.15, x / (width * scale));
            match (here_continental, there_continental) {
                // Continent against continent: the widest, highest ranges.
                (true, true) => {
                    let here = collision * near(1.4);
                    if here > boundary.belt {
                        (boundary.belt, boundary.across) = (here, x);
                    }
                    boundary.foothills = boundary
                        .foothills
                        .max(collision * smoothstep(2.6, 0.7, x / width));
                }
                // An ocean plate diving under this continent: a coastal cordillera.
                (true, false) => {
                    let here = collision * near(0.9);
                    if here > boundary.belt {
                        (boundary.belt, boundary.across) = (here, x);
                    }
                    boundary.foothills = boundary
                        .foothills
                        .max(collision * smoothstep(1.8, 0.6, x / width));
                }
                // This ocean plate diving under a continent: a trench.
                (false, true) => {
                    boundary.trench = boundary
                        .trench
                        .max(collision * (-(x / (width * 0.35)).powi(2)).exp());
                }
                // Ocean against ocean: the younger plate (by index, deterministically)
                // overrides and grows an island arc behind the trench.
                (false, false) => {
                    if a < k {
                        boundary.trench = boundary
                            .trench
                            .max(collision * (-(x / (width * 0.35)).powi(2)).exp());
                    } else {
                        let offset = (x - width * 0.7) / (width * 0.35);
                        boundary.arc = boundary.arc.max(collision * (-offset * offset).exp());
                    }
                }
            }
            if here_continental {
                boundary.rift = boundary.rift.max(divergence * (-(x / (width * 0.5)).powi(2)).exp());
                boundary.shoulders = boundary.shoulders.max(
                    divergence * (-((x - width * 0.8) / (width * 0.25)).powi(2)).exp(),
                );
            } else {
                boundary.ridge = boundary.ridge.max(divergence * (-(x / (width * 2.)).powi(2)).exp());
            }
        }
        // Island arcs: a chain of volcanic islands, rising through the crust value.
        let volcanoes = smoothstep(0.55, 0.85, arcs.ridged3(p / 14., 3));
        let crust = base + outline + boundary.arc * volcanoes * 1.2;
        (a as u16, crust, boundary)
    });
    let plate_of: Vec<u16> = pass1.iter().map(|c| c.0).collect();
    let crust: Vec<f32> = pass1.iter().map(|c| c.1).collect();
    let mut sorted = crust.clone();
    sorted.sort_by(f32::total_cmp);
    let coast = sorted[((1. - settings.land_fraction) * (cells - 1) as f32) as usize];
    let sea: Vec<bool> = crust.iter().map(|&c| c < coast).collect();
    let continental: Vec<bool> = crust.iter().map(|&c| c >= coast - SHELF_BAND).collect();
    let to_sea = distance_to(&grid, &sea);
    lap("plates", &mut timings);

    // Pass 2: landforms on land.
    let pass2: Vec<Cell> = par_map(cells, |i| {
        let direction = grid.direction(i);
        let p = direction * radius;
        let b = pass1[i].2;
        let land = smoothstep(coast, coast + 0.06, crust[i]);
        // Young belts: parallel ridges, broken into peaks and passes.
        let phase = (b.across + 0.25 * settings.ridge_spacing * peaks.layer(2).fbm3(p / 50., 3))
            / settings.ridge_spacing;
        let parallel = (0.5 + 0.5 * (phase * std::f32::consts::TAU).cos()).powf(1.5);
        let summits = peaks.ridged3(p / 32., 4);
        let young = b.belt * (0.3 + 0.7 * (0.5 * parallel + 0.5 * summits));
        let foothills = b.foothills * (0.5 + 0.5 * hills.layer(5).fbm3(p / 30., 3).abs());

        // Old ranges along the sutures between ancient blocks: lower and rounder with age.
        let w = (p + warp.layer(3).warp3(p / 140., 3, 60.)).normalize();
        let mut candidates = Vec::new();
        craton_index.candidates(w, &mut candidates);
        let mut near: Vec<(f32, usize)> = candidates
            .iter()
            .map(|&k| (cratons[k].center.angle_between(w) * radius, k))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut old, mut age) = (0f32, 0f32);
        if near.len() >= 2 {
            let (d1, a) = near[0];
            for &(dk, k) in &near[1..near.len().min(4)] {
                let (lo, hi) = (a.min(k) as u32, a.max(k) as u32);
                let suture = hash(settings.seed as u32 ^ 0x01D, lo, hi) as f32 / u32::MAX as f32;
                if suture > settings.old_range_share {
                    continue;
                }
                let pair_age = 0.3 + 0.7 * (suture / settings.old_range_share);
                let x = (dk - d1) / 2.;
                let here = smoothstep(1., 0.1, x / (settings.range_width * 1.3));
                if here > old {
                    (old, age) = (here, pair_age);
                }
            }
        }
        let worn = old * (1. - age * 0.7) * (0.5 + 0.5 * hills.layer(6).fbm3(p / 45., 3).abs());

        // Provinces, blended from the nearest few.
        let w = (p + warp.layer(1).warp3(p / 160., 3, 70.) + warp.layer(2).warp3(p / 50., 2, 15.))
            .normalize();
        province_index.candidates(w, &mut candidates);
        let mut near: Vec<(f32, usize)> = candidates
            .iter()
            .map(|&k| (provinces[k].center.angle_between(w) * radius, k))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut weights, mut cap, mut total) = ([0f32; 3], 0., 0.);
        for &(d, k) in near.iter().take(4) {
            let weight = (-(d - near[0].0) / settings.province_blend).exp();
            weights[provinces[k].kind] += weight;
            cap += provinces[k].value * weight;
            total += weight;
        }
        let [mut plains, mut hilly, mut plateau] = weights.map(|weight| weight / total.max(1e-6));
        // Near the sea: lowlands and hills, no high plateaus.
        let inland = smoothstep(4., 40., to_sea[i]);
        let plateau_kept = plateau * smoothstep(20., 60., to_sea[i]);
        let hills_kept = hilly * (0.5 + 0.5 * inland);
        plains += (plateau - plateau_kept) + (hilly - hills_kept);
        (hilly, plateau) = (hills_kept, plateau_kept);

        let rolling = 0.6 + 0.4 * hills.layer(1).fbm3(p / 80., 4);
        let undulation = 0.5 + 0.5 * hills.layer(3).fbm3(p / 35., 3);
        let interior = (plains * 0.2 * undulation + hilly * 0.9 * rolling + plateau)
            * (0.35 + 0.65 * inland);
        let uplift = (interior * (1. - 0.85 * b.rift) * (1. - b.belt * 0.5)
            + young * 1.1
            + foothills * 0.22
            + worn * 0.55
            + b.shoulders * 0.35)
            * land.powf(1.5);
        let range = (b.belt + foothills * 0.4).max(old * 0.6).min(1.);
        Cell {
            land,
            uplift,
            subsidence: b.rift * 0.4 * land,
            provinces: [plains, hilly, plateau],
            plateau_cap: cap / total.max(1e-6),
            range,
            erodibility: (plains * 1.2 + hilly * 1.3 + plateau * 0.12) * (1. - b.belt)
                + 0.75 * b.belt,
            // Old ranges are rounded: more hillslope diffusion the older they are.
            diffusion: settings.diffusion
                * (plains * 0.6 + hilly * 2.5 + plateau * 0.15)
                * (1. - b.belt * 0.4)
                * (1. + 3. * old * age),
            glacial_noise: 1. + 0.6 * hills.layer(7).fbm3(p / 14., 2),
            height: if sea[i] { 0. } else { land * (0.5 + hills.layer(8).fbm3(p / 40., 4) * 0.3) },
        }
    });
    let shape = Shape {
        land: pass2.iter().map(|c| c.land).collect(),
        uplift: pass2.iter().map(|c| c.uplift).collect(),
        erodibility: pass2.iter().map(|c| c.erodibility).collect(),
        diffusion: pass2.iter().map(|c| c.diffusion).collect(),
        rain: (0..cells).map(|i| grid.area(i)).collect(),
        base_level: sea.clone(),
        heights: pass2.iter().map(|c| c.height).collect(),
        sea_floor: vec![0.; cells],
        provinces: pass2.iter().map(|c| c.provinces).collect(),
        range: pass2.iter().map(|c| c.range).collect(),
        plateau_cap: pass2.iter().map(|c| c.plateau_cap).collect(),
        subsidence: pass2.iter().map(|c| c.subsidence).collect(),
        rift: pass1.iter().map(|c| c.2.rift).collect(),
        glacial_noise: pass2.iter().map(|c| c.glacial_noise).collect(),
    };
    drop(pass2);
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

    // World units: the ranges' summits set the scale; plateaus are cut at their cap level.
    let mut summits: Vec<f32> = (0..cells)
        .filter(|&i| !sea[i] && shape.range[i] > 0.5)
        .map(|i| heights[i])
        .collect();
    if summits.is_empty() {
        summits = heights.clone();
    }
    summits.sort_by(f32::total_cmp);
    let reference = summits[(summits.len() - 1) * 99 / 100].max(1e-6);
    for i in 0..cells {
        if !sea[i] {
            heights[i] *= settings.max_height / reference;
        }
    }
    terrain::cap_plateaus(&shape, &mut heights);
    // The sea floor: a shelf over continental crust, the continental slope where the crust
    // thins, the abyss over oceanic crust, raised along mid-ocean ridges, deepened in trenches.
    let floor = noise.layer(9);
    let shelf_floor: Vec<f32> = (0..cells)
        .map(|i| {
            let b = pass1[i].2;
            let slope = smoothstep(coast - SHELF_BAND * 0.6, coast - SHELF_BAND * 1.6, crust[i]);
            let abyss = settings.sea_depth * (1. - 0.45 * b.ridge) * (1. + 0.4 * b.trench);
            -(SHELF_DEPTH + (abyss - SHELF_DEPTH) * slope)
        })
        .collect();
    let shelf_floor = grid.blur(&shelf_floor, 3);
    for i in 0..cells {
        if sea[i] {
            let p = grid.direction(i) * radius;
            // (At sea, `to_sea` is the distance to land.)
            let shore = smoothstep(0., 6., to_sea[i]);
            heights[i] = (-0.3 + (shelf_floor[i] + 0.3) * shore)
                * (0.9 + 0.1 * floor.fbm3(p / 60., 3));
        }
    }
    let filled = priority_flood(&grid, &heights, &sea, 1e-4);
    let drainage = FlowGraph::new(&grid, &filled, &sea).accumulate(|i| grid.area(i));
    lap("drainage", &mut timings);

    Planet {
        settings: settings.clone(),
        grid,
        heights,
        sea,
        continental,
        range: shape.range,
        drainage,
        plate: plate_of,
        continental_plate: plates.iter().map(|p| p.continental).collect(),
        timings,
    }
}

/// Per-cell results of the landform pass.
struct Cell {
    land: f32,
    uplift: f32,
    subsidence: f32,
    provinces: [f32; 3],
    plateau_cap: f32,
    range: f32,
    erodibility: f32,
    diffusion: f32,
    glacial_noise: f32,
    height: f32,
}

/// Distance (in cells) from each cell to the nearest cell on the other side of the `sea`
/// mask: for land cells to the sea, for sea cells to land. Each cell's nearest source is
/// propagated from its neighbours and the distance measured to it directly (stepping along
/// neighbours instead would draw star-shaped rays).
pub fn distance_to(grid: &SphereGrid, sea: &[bool]) -> Vec<f32> {
    use std::{cmp::Reverse, collections::BinaryHeap};
    let mut distance = vec![f32::MAX; grid.len()];
    let mut source = vec![usize::MAX; grid.len()];
    let mut open = BinaryHeap::new();
    // Sources: cells next to the other side.
    for i in 0..grid.len() {
        if grid.neighbours(i).any(|(j, _)| sea[j] != sea[i]) {
            distance[i] = 0.5;
            source[i] = i;
            open.push(Reverse((512u32, i)));
        }
    }
    // (Distances as fixed point, so the heap orders integers.)
    while let Some(Reverse((d, i))) = open.pop() {
        if d as f32 / 1024. > distance[i] {
            continue;
        }
        for (j, _) in grid.neighbours(i) {
            if sea[j] != sea[i] {
                continue;
            }
            let candidate = 0.5 + grid.distance(j, source[i]);
            if candidate < distance[j] {
                distance[j] = candidate;
                source[j] = source[i];
                open.push(Reverse(((candidate * 1024.) as u32, j)));
            }
        }
    }
    distance
}
