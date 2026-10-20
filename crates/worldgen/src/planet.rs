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
    grid::Grid,
    noise::{Noise, hash, smoothstep},
    par,
    sphere::SphereGrid,
    geology::Rock,
    terrain::{self, Shape},
};

/// Frequency of the continent outline noise on the unit sphere.
const CONTINENT_FREQUENCY: f32 = 2.4;
/// How much a plate's crust raises (continental) or lowers (oceanic) the continent outline.
const CONTINENTAL_BIAS: f32 = 0.22;
/// Subsidence of rift floors (relative, like uplift).
const RIFT_SUBSIDENCE: f32 = 0.3;
/// Drainage area (cells) from which fine detail fades around rivers.
const CALM_RIVER_AREA: f32 = 150.;
/// Low flat ground along streams is wet: slopes under this, draining this much (mean cell
/// areas), below this height (world units).
const WET_SLOPE: f32 = 0.1;
const WET_DRAINAGE: f32 = 40.;
const WET_HEIGHT: f32 = 4.;
/// Uplift of the low swells over all land (relative, like the provinces' uplift).
const PLAIN_SWELLS: f32 = 0.08;
/// Distances to the coast are only needed this far (cells) inland and out at sea: beyond,
/// they are this value.
const COAST_REACH: f32 = 64.;
const SHORE_REACH: f32 = 8.;
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
    /// Shape continents and mountain belts from a tectonic history (`tectonics.rs`) instead of
    /// noise and plate-boundary rules.
    pub history: Option<crate::tectonics::TectonicSettings>,
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
    /// The first `coarse_steps` erosion steps run on a grid `coarse_factor` times coarser (the
    /// large drainage network forms there, at a fraction of the cost); the rest refine it at
    /// full resolution. 0: all at full resolution.
    pub coarse_steps: usize,
    pub coarse_factor: usize,
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
            history: Some(crate::tectonics::TectonicSettings::default()),
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
            coarse_steps: 45,
            coarse_factor: 2,
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
    /// Depth of the lake water over each cell (0: no lake), from filling the basins.
    pub lake_depth: Vec<f32>,
    /// Wet ground (0..1): closed flats and dry basins, lake shores, floodplains, delta land;
    /// and the dry basins alone (no outlet: salt flats where dry). See `planet_lakes`.
    pub wetland: Vec<f32>,
    pub basins: Vec<f32>,
    /// The rivers (see `planet_rivers`).
    pub rivers: Vec<crate::planet_rivers::PlanetRiver>,
    /// The rock of each cell (`geology::Rock` ids) and its strata offset (erosion units), and
    /// world units per erosion unit: a bed at erosion height `e` is at world height
    /// `e * strata_scale` (strata height `height / strata_scale + offset`).
    pub rock: Vec<u8>,
    pub strata: Vec<f32>,
    pub strata_scale: f32,
    /// The volcanoes (see `volcanoes`), and fresh lava over each cell (0..1, active ones).
    pub volcanoes: Vec<crate::volcanoes::Volcano>,
    pub lava: Vec<f32>,
    /// Geothermal fields (hot springs: 0..1, round young volcanoes).
    pub geothermal: Vec<f32>,
    /// Coral reef over each cell (0..1, see `reefs`).
    pub reef: Vec<f32>,
    /// How rugged the ground is (0..1, from the provinces and ranges): fine detail strength.
    pub ruggedness: Vec<f32>,
    /// 1 where fine detail must vanish (rivers and lakes), 0 elsewhere, softened.
    pub calm: Vec<f32>,
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
/// ones to a direction quickly: each coarse cell lists the points in it and in two rings of
/// cells around it.
struct PointIndex {
    coarse: SphereGrid,
    candidates: Vec<Vec<usize>>,
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
        let candidates = (0..coarse.len())
            .map(|cell| {
                let mut cells = vec![cell];
                for _ in 0..2 {
                    let ring: Vec<usize> = cells
                        .iter()
                        .flat_map(|&c| coarse.neighbours(c).map(|(j, _)| j))
                        .collect();
                    cells.extend(ring);
                    cells.sort_unstable();
                    cells.dedup();
                }
                cells.iter().flat_map(|&c| buckets[c].iter().copied()).collect()
            })
            .collect();
        Self { coarse, candidates }
    }

    /// The points near `direction` (in its coarse cell and two rings around it).
    fn candidates(&self, direction: Vec3) -> &[usize] {
        &self.candidates[self.coarse.cell_at(direction)]
    }

    /// The `K` points nearest to `direction` (by angle, converted to cells with `radius`),
    /// nearest first; fewer if there are fewer candidates.
    fn nearest<const K: usize>(
        &self,
        points: &[Region],
        direction: Vec3,
        radius: f32,
    ) -> ([(f32, usize); K], usize) {
        nearest_k(self.candidates(direction).iter().map(|&k| (points[k].center, k)), direction, radius)
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

/// The `K` nearest of some points (centre, id) to a direction: `(distance in cells, id)`,
/// nearest first, and how many were found. Ranked by dot product; only the kept ones get an
/// angle.
fn nearest_k<const K: usize>(
    points: impl Iterator<Item = (Vec3, usize)>,
    direction: Vec3,
    radius: f32,
) -> ([(f32, usize); K], usize) {
    let mut best = [(f32::MIN, usize::MAX); K];
    let mut count = 0;
    for (center, id) in points {
        let dot = center.dot(direction);
        if count == K && dot <= best[K - 1].0 {
            continue;
        }
        let mut k = count.min(K - 1);
        while k > 0 && best[k - 1].0 < dot {
            best[k] = best[k - 1];
            k -= 1;
        }
        best[k] = (dot, id);
        count = (count + 1).min(K);
    }
    for entry in &mut best[..count] {
        entry.0 = entry.0.clamp(-1., 1.).acos() * radius;
    }
    (best, count)
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

    // The tectonic history, if any: crust, uplift (blurred into belts), oceanic age.
    let history = settings.history.as_ref().map(|history| {
        let t = crate::tectonics::run(&crate::tectonics::TectonicSettings {
            seed: settings.seed,
            ..history.clone()
        });
        let young: Vec<f32> =
            (0..t.grid.len()).map(|i| t.orogeny[i] * (-t.orogeny_age[i] / YOUNG_STEPS).exp()).collect();
        let old: Vec<f32> = (0..t.grid.len())
            .map(|i| t.orogeny[i] * (1. - (-t.orogeny_age[i] / YOUNG_STEPS).exp()))
            .collect();
        let crust: Vec<f32> = t.crust.iter().map(|c| c.min(1.)).collect();
        let fields = HistoryFields {
            crust: t.grid.blur(&crust, 1),
            young: t.grid.blur(&young, 3),
            old: t.grid.blur(&old, 4),
            age: t.age.clone(),
        };
        (t, fields)
    });
    let continents = noise.layer(1);
    let peaks = noise.layer(2);
    let hills = noise.layer(3);
    let warp = noise.layer(4);
    let arcs = noise.layer(6);
    let width_at = |p: Vec3| settings.range_width * (0.8 + 0.45 * peaks.layer(1).fbm3(p / 120., 2));

    // Shaping on a grid whose cells are `scale` full-size cells long (all lengths are in
    // full-size cells: noise, plates and provinces look the same on any grid).
    let shape_on = |grid: &SphereGrid, scale: f32| -> Shaped {
    let cells = grid.len();
    // Pass 1: plates, boundaries, crust.
    let pass1: Vec<(u16, f32, Boundary)> = par::map(cells, |i| {
        let direction = grid.direction(i);
        let p = direction * radius;
        let w = (p + warp.warp3(p / 180., 3, 45.)).normalize();
        let (nearest, count) =
            nearest_k::<4>(plates.iter().enumerate().map(|(k, p)| (p.center, k)), w, radius);
        let nearest = &nearest[..count];
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
        for &(d, k) in nearest {
            let weight = (-(d - d1) / CRUST_BLEND).exp();
            base += bias(k) * weight;
            total += weight;
        }
        let base = base / total;
        let mut boundary = Boundary::default();
        // With a history: continents from its crust (noise only details the coasts), belts
        // from its uplift, mid-ocean ridges where its ocean floor is young.
        if let Some((t, fields)) = &history {
            let crust = t.grid.bilinear(&fields.crust, direction);
            let young = t.grid.bilinear(&fields.young, direction);
            let ocean_age = t.grid.bilinear(&fields.age, direction);
            let continental = crust > 0.5;
            boundary.belt = if continental { smoothstep(0.06, 0.6, young) } else { 0. };
            // (Ridges run parallel to the belt: the belt's strength falls steadily from its
            // axis, so its contours stand in for the distance across it.)
            boundary.across = (1. - smoothstep(0.03, 0.9, young)) * width * 1.4;
            boundary.foothills = if continental { smoothstep(0.01, 0.15, young) } else { 0. };
            boundary.arc = if continental { 0. } else { smoothstep(0.1, 0.6, young) };
            boundary.ridge = if continental { 0. } else { (-ocean_age / 4.).exp() };
            let peaks_here = smoothstep(0.78, 0.95, arcs.ridged3(p / 16., 2));
            let stretches = smoothstep(0.05, 0.35, arcs.layer(1).fbm3(p / 70., 2));
            let value = (crust - 0.5) * 1.2 + outline * 0.3 + boundary.arc * peaks_here * stretches * 1.2;
            let plate = t.plate[t.grid.cell_at(direction)];
            return (plate, value, boundary);
        }
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
                        let offset = (x - width * 0.7) / (width * 0.22);
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
        // Island arcs: a sparse chain of volcanic islands (scattered peaks, with long gaps
        // along the arc), rising through the crust value.
        let peaks_here = smoothstep(0.78, 0.95, arcs.ridged3(p / 16., 2));
        let stretches = smoothstep(0.05, 0.35, arcs.layer(1).fbm3(p / 70., 2));
        let volcanoes = peaks_here * stretches;
        let crust = base + outline + boundary.arc * volcanoes * 1.2;
        (a as u16, crust, boundary)
    });
    let plate_of: Vec<u16> = pass1.iter().map(|c| c.0).collect();
    let crust: Vec<f32> = pass1.iter().map(|c| c.1).collect();
    // (A selection, not a sort: only this percentile is needed.)
    let mut sorted = crust.clone();
    let k = ((1. - settings.land_fraction) * (cells - 1) as f32) as usize;
    let coast = *sorted.select_nth_unstable_by(k, f32::total_cmp).1;
    let sea: Vec<bool> = crust.iter().map(|&c| c < coast).collect();
    let continental: Vec<bool> = crust.iter().map(|&c| c >= coast - SHELF_BAND).collect();
    let to_sea: Vec<f32> = distance_to(grid, &sea, COAST_REACH / scale, SHORE_REACH / scale)
        .into_iter()
        .map(|d| d * scale)
        .collect();

    // Pass 2: landforms on land.
    let pass2: Vec<Cell> = par::map(cells, |i| {
        let b = pass1[i].2;
        // (Erosion keeps the sea at its base level: none of this is used there.)
        if sea[i] {
            return Cell {
                uplift: 0.,
                subsidence: 0.,
                provinces: [1., 0., 0.],
                plateau_cap: 0.,
                range: b.belt.min(1.),
                erodibility: 1.,
                diffusion: 0.,
                glacial_noise: 1.,
                height: 0.,
                rock: Rock::Basalt,
                strata: 0.,
            };
        }
        let direction = grid.direction(i);
        let p = direction * radius;
        let land = smoothstep(coast, coast + 0.06, crust[i]);
        // Young belts: parallel ridges, broken into peaks and passes.
        let phase = (b.across + 0.25 * settings.ridge_spacing * peaks.layer(2).fbm3(p / 50., 3))
            / settings.ridge_spacing;
        let parallel = (0.5 + 0.5 * (phase * std::f32::consts::TAU).cos()).powf(1.5);
        let summits = peaks.ridged3(p / 32., 4);
        let young = b.belt * (0.3 + 0.7 * (0.5 * parallel + 0.5 * summits));
        let foothills = b.foothills * (0.5 + 0.5 * hills.layer(5).fbm3(p / 30., 3).abs());

        // Old ranges: with a history, its old uplift (worn the more the older); else along the
        // sutures between ancient blocks.
        let w = (p + warp.layer(3).warp3(p / 140., 3, 60.)).normalize();
        let (near, count) = craton_index.nearest::<4>(&cratons, w, radius);
        let near = &near[..count];
        let (mut old, mut age) = (0f32, 0f32);
        if let Some((t, fields)) = &history {
            let uplift = t.grid.bilinear(&fields.old, direction);
            old = smoothstep(0.05, 0.6, uplift);
            age = 0.6;
        } else if near.len() >= 2 {
            let (d1, a) = near[0];
            for &(dk, k) in &near[1..] {
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
        let (near, count) = province_index.nearest::<4>(&provinces, w, radius);
        let near = &near[..count];
        let (mut weights, mut cap, mut total) = ([0f32; 3], 0., 0.);
        for &(d, k) in near {
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
        // Low swells over all land, plains and coasts included: without them, a gently tilted
        // plain drains in straight parallel lines; with them, rivers wind between the swells.
        let swells = PLAIN_SWELLS
            * (0.5 + 0.5 * hills.layer(9).fbm3(p / 24., 3))
            * (0.5 + 0.5 * hills.layer(10).fbm3(p / 70., 2));
        let uplift = (interior * (1. - 0.85 * b.rift) * (1. - b.belt * 0.5)
            + young * 1.1
            + foothills * 0.22
            + worn * 0.55
            + b.shoulders * 0.35)
            * land.powf(1.5)
            + swells * land.sqrt();
        let range = (b.belt + foothills * 0.4).max(old * 0.6).min(1.);
        let (rock, dip) = rock_of(b, old, [plains, hilly, plateau], hills.layer(12).fbm3(p / 90., 3));
        // Strata surfaces `height + offset = const`: the offset is a smooth field whose slope
        // is the dip (gentle on plateaus, steep in the foothills of ranges).
        let strata = dip * STRATA_WAVELENGTH * hills.layer(13).fbm3(p / STRATA_WAVELENGTH, 2);
        Cell {
            rock,
            strata,
            uplift,
            // Rift floors sink in segments along the axis (a chain of basins, rift lakes), as
            // on the flat map.
            subsidence: b.rift
                * smoothstep(-0.1, 0.35, hills.layer(11).fbm3(p / 45., 2))
                * RIFT_SUBSIDENCE
                * land,
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
        uplift: pass2.iter().map(|c| c.uplift).collect(),
        erodibility: pass2.iter().map(|c| c.erodibility).collect(),
        diffusion: pass2.iter().map(|c| c.diffusion).collect(),
        // (Rain in full-size cells: a coarse cell gathers `scale²` of them.)
        rain: (0..cells).map(|i| grid.area(i) * scale * scale).collect(),
        base_level: sea.clone(),
        heights: pass2.iter().map(|c| c.height).collect(),
        provinces: pass2.iter().map(|c| c.provinces).collect(),
        range: pass2.iter().map(|c| c.range).collect(),
        plateau_cap: pass2.iter().map(|c| c.plateau_cap).collect(),
        subsidence: pass2.iter().map(|c| c.subsidence).collect(),
        glacial_noise: pass2.iter().map(|c| c.glacial_noise).collect(),
        rock: pass2.iter().map(|c| c.rock).collect(),
        strata: pass2.iter().map(|c| c.strata).collect(),
    };
    Shaped {
        shape,
        boundaries: pass1.iter().map(|c| c.2).collect(),
        plate_of,
        crust,
        coast,
        sea,
        continental,
        to_sea,
    }
    };

    // Erosion, as on the flat map: first on the coarse grid, then refined at full resolution
    // from its result.
    let erosion = |steps: usize, scale: f32| WorldgenSettings {
        seed: settings.seed,
        erosion_steps: steps,
        erosion_dt: settings.erosion_dt,
        erodibility: settings.erodibility,
        area_exponent: settings.area_exponent,
        diffusion: settings.diffusion,
        routing_jitter: settings.routing_jitter,
        cell_size: scale,
        ..Default::default()
    };
    let report = |name: &'static str, t: &terrain::ErosionTimings, timings: &mut Vec<(&'static str, Duration)>| {
        timings.extend([
            (name, t.routing + t.accumulation + t.incision + t.glaciers + t.diffusion),
            ("    routing", t.routing),
            ("    accumulation", t.accumulation),
            ("    incision", t.incision),
            ("    glaciers", t.glaciers),
            ("    diffusion", t.diffusion),
        ]);
    };
    let coarse_steps = settings.coarse_steps.min(settings.erosion_steps);
    let mut fine_shape = None;
    if coarse_steps > 0 && settings.coarse_factor > 1 {
        let factor = settings.coarse_factor;
        let coarse_grid = SphereGrid::new(settings.n / factor);
        let coarse = shape_on(&coarse_grid, factor as f32);
        let fine = shape_on(&grid, 1.);
        lap("shape", &mut timings);
        let (coarse_heights, t) =
            terrain::erode_timed(&coarse_grid, &erosion(coarse_steps, factor as f32), &coarse.shape);
        report("  coarse erosion", &t, &mut timings);
        let mut fine = fine;
        // The fine grid starts from the coarse result (its land only).
        let upsampled = upsample(&coarse_grid, &coarse_heights, &grid);
        for (i, h) in fine.shape.heights.iter_mut().enumerate() {
            *h = if fine.sea[i] { 0. } else { upsampled[i].max(0.) };
        }
        fine_shape = Some(fine);
    }
    let shaped = match fine_shape {
        Some(shaped) => shaped,
        None => {
            let shaped = shape_on(&grid, 1.);
            lap("shape", &mut timings);
            shaped
        }
    };
    let Shaped { shape, boundaries, plate_of, crust, coast, mut sea, continental, to_sea } = shaped;
    let cells = grid.len();
    let (mut heights, t) =
        terrain::erode_timed(&grid, &erosion(settings.erosion_steps - coarse_steps, 1.), &shape);
    report("  fine erosion", &t, &mut timings);
    lap("erosion", &mut timings);
    // ("erosion" is the sum of both stages, listed after them.)
    let erosion_total = timings.pop().unwrap();
    let at = timings.iter().position(|t| t.0.starts_with("  coarse") || t.0.starts_with("  fine")).unwrap_or(timings.len());
    timings.insert(at, erosion_total);

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
    let strata_scale = settings.max_height / reference;
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
            let b = boundaries[i];
            let slope = smoothstep(coast - SHELF_BAND * 0.6, coast - SHELF_BAND * 1.6, crust[i]);
            let abyss = settings.sea_depth * (1. - 0.45 * b.ridge) * (1. + 0.4 * b.trench);
            -(SHELF_DEPTH + (abyss - SHELF_DEPTH) * slope)
        })
        .collect();
    let shelf_floor = grid.blur(&shelf_floor, 3);
    // Shores, as on the flat map (`terrain::shape_coast`): from a blurred signed distance to
    // the shore (so the coast doesn't follow the cells' staircase), land rises gently from
    // the water line and the sea floor slopes down from it.
    let signed: Vec<f32> = (0..cells)
        .map(|i| if sea[i] { -to_sea[i] } else { to_sea[i] })
        .collect();
    let shore = grid.blur(&grid.blur(&signed, 2), 2);
    let variation = noise.layer(10);
    for i in 0..cells {
        let p = grid.direction(i) * radius;
        let d = shore[i];
        if sea[i] {
            let open = smoothstep(0., 6., -d);
            let floor_height = (-0.3 + (shelf_floor[i] + 0.3) * open)
                * (0.9 + 0.1 * floor.fbm3(p / 60., 3));
            heights[i] = floor_height.max(0.15 + 0.35 * (d - 0.5)).min(-0.05);
        } else {
            let slope = 0.25 + 0.3 * (0.5 + 0.5 * variation.fbm3(p / 60., 3)) + 0.5 * shape.range[i];
            let ramp = 0.15 + slope * (d - 0.5);
            // (Land cells may end up just under water: the shore follows `shore`.)
            heights[i] = terrain::smooth_min(heights[i], ramp, 1.).max(-0.3);
        }
    }
    // Fjords: glacial troughs flooded by the sea at cold, mountainous coasts.
    crate::fjords::carve(&grid, radius, settings.seed, &mut heights, &mut sea);
    lap("  fjords", &mut timings);
    // Volcanoes: island and continental arcs, hotspot chains along the plates' motion.
    let velocity = |d: Vec3| match &history {
        Some((t, _)) => t.plates[t.plate[t.grid.cell_at(d)] as usize].rotation.cross(d),
        None => plates[plate_of[grid.cell_at(d)] as usize].velocity(d),
    };
    let volcanoes = crate::volcanoes::place(
        &grid,
        radius,
        settings.seed,
        |i| boundaries[i].arc,
        |i| if !sea[i] && boundaries[i].belt > 0.4 && to_sea[i] < COASTAL_ARC { 1. } else { 0. },
        velocity,
        |i| sea[i],
    );
    let volcanic = crate::volcanoes::build(&grid, radius, settings.seed, &volcanoes, &mut heights, &mut sea);
    let mut rock: Vec<u8> = shape.rock.iter().map(|&r| r as u8).collect();
    let mut strata = shape.strata.clone();
    for i in 0..cells {
        if volcanic.volcanic[i] {
            rock[i] = Rock::Basalt as u8;
            strata[i] = 0.;
        }
    }
    // Coral reefs in the tropics: fringing and barrier reefs, atolls on sunk volcanoes.
    let reef = crate::reefs::build(&grid, radius, settings.seed, &volcanoes, &mut heights, &mut sea);
    lap("  volcanoes", &mut timings);
    // Barrier islands and lagoons on low coasts with a shallow shelf.
    crate::barriers::build(&grid, radius, settings.seed, &reef, &mut heights, &mut sea, &mut rock);
    lap("  barrier islands", &mut timings);
    // Lakes (real ones, with basins of their own), filled flats and dry basins below sea level.
    let crate::planet_lakes::Hydrology { filled, drainage, lake_depth, base, mut wetland, basins } =
        crate::planet_lakes::shape_lakes(&grid, &mut heights, &mut sea);
    lap("  lakes", &mut timings);
    let ruggedness: Vec<f32> = (0..cells).map(|i| shape.ruggedness(i).max(volcanic.ruggedness[i])).collect();
    let (mut rivers, valley) =
        crate::planet_rivers::extract(&grid, radius, &heights, &filled, &base, &lake_depth);
    // Floodplains: flat valley floors where rivers meander.
    lap("  rivers", &mut timings);
    let floodplain = flatten_floodplains(&grid, radius, &mut heights, &lake_depth, &valley);
    // Wet ground: floodplains, and low flat ground along the larger streams (poorly drained
    // valley floors and plains).
    // (Slopes of the general lie of the land: on smoothed heights; near streams: their mask
    // spread a few cells.)
    let smooth = grid.blur(&heights, 2);
    let streams: Vec<f32> = drainage.iter().map(|&d| if d >= WET_DRAINAGE { 1. } else { 0. }).collect();
    let near_stream = grid.blur(&streams, 3);
    for i in 0..cells {
        let slope = grid
            .neighbours(i)
            .map(|(j, d)| (smooth[j] - smooth[i]).abs() / d)
            .sum::<f32>()
            / grid.neighbours(i).count() as f32;
        let flat_valley = smoothstep(WET_SLOPE, WET_SLOPE * 0.4, slope)
            * smoothstep(0.03, 0.15, near_stream[i])
            * smoothstep(WET_HEIGHT, WET_HEIGHT * 0.5, heights[i]);
        wetland[i] = wetland[i].max(0.9 * floodplain[i]).max(0.8 * flat_valley);
    }
    let wetland = grid.blur(&wetland, 1);
    let mut wetland = wetland;
    lap("  floodplains, wetlands", &mut timings);
    let sea_before_mouths = sea.clone();
    // Deltas and estuaries at the river mouths (deltas build land out into the sea).
    crate::planet_mouths::shape_mouths(
        &grid,
        radius,
        settings.seed,
        noise.layer(11),
        &shore,
        &mut heights,
        &mut sea,
        &mut wetland,
        &mut rivers,
    );
    // The floodplains and mouths reshaped the ground: settle the water again (the routing
    // base: the sea as it is now, and the dry basins' sinks).
    let base: Vec<bool> = (0..cells).map(|i| sea[i] || base[i] && !sea_before_mouths[i]).collect();
    lap("  mouths", &mut timings);
    let (drainage, lake_depth, flats) = crate::planet_lakes::settle(&grid, &mut heights, &base, &lake_depth);
    lap("  settle", &mut timings);
    for i in flats {
        wetland[i] = 1.;
    }
    // Rivers (cells draining enough, as drawn in the previews) and lakes, widened.
    let calm: Vec<f32> = (0..cells)
        .map(|i| {
            let river = smoothstep(CALM_RIVER_AREA * 0.5, CALM_RIVER_AREA, drainage[i]);
            let lake = smoothstep(0.02, 0.1, lake_depth[i]);
            // (Wetlands are flat: floodplains, closed flats, delta land.)
            river.max(lake).max(wetland[i]).max(basins[i])
        })
        .collect();
    let calm: Vec<f32> = grid.blur(&calm, 2).iter().map(|c| (c * 3.).min(1.)).collect();
    lap("drainage", &mut timings);

    Planet {
        settings: settings.clone(),
        grid,
        heights,
        sea,
        continental,
        range: shape.range,
        drainage,
        lake_depth,
        wetland,
        basins,
        rock,
        strata,
        lava: volcanic.lava,
        geothermal: volcanic.geothermal,
        reef,
        volcanoes,
        strata_scale,
        rivers,
        ruggedness,
        calm,
        plate: plate_of,
        continental_plate: match &history {
            // (A history's plates: continental if a good share of their crust is.)
            Some((t, _)) => (0..t.plates.len())
                .map(|k| {
                    let cells = (0..t.grid.len()).filter(|&i| t.plate[i] as usize == k);
                    let (total, land) = cells.fold((0, 0), |(n, l), i| (n + 1, l + (t.crust[i] > 0.5) as usize));
                    land * 3 > total
                })
                .collect(),
            None => plates.iter().map(|p| p.continental).collect(),
        },
        timings,
    }
}

/// A tectonic history's fields, smoothed, for shaping.
struct HistoryFields {
    crust: Vec<f32>,
    young: Vec<f32>,
    old: Vec<f32>,
    age: Vec<f32>,
}

/// Uplift younger than this many tectonic steps makes young (sharp) ranges.
const YOUNG_STEPS: f32 = 12.;

/// Shaping results on one grid.
struct Shaped {
    shape: Shape,
    boundaries: Vec<Boundary>,
    plate_of: Vec<u16>,
    crust: Vec<f32>,
    coast: f32,
    sea: Vec<bool>,
    continental: Vec<bool>,
    /// Distance to the coast (full-size cells, up to `COAST_REACH`).
    to_sea: Vec<f32>,
}

/// Values of a coarse grid sampled on a finer one (bilinear within each face, clamped at face
/// edges: the half coarse cell along them is refined by the fine steps anyway).
fn upsample(coarse: &SphereGrid, values: &[f32], fine: &SphereGrid) -> Vec<f32> {
    let (m, n) = (coarse.n(), fine.n());
    let ratio = m as f32 / n as f32;
    (0..fine.len())
        .map(|i| {
            let (face, x, z) = fine.coords(i);
            let c = |v: usize| ((v as f32 + 0.5) * ratio - 0.5).clamp(0., (m - 1) as f32);
            let (cx, cz) = (c(x), c(z));
            let (x0, z0) = (cx.floor() as usize, cz.floor() as usize);
            let (x1, z1) = ((x0 + 1).min(m - 1), (z0 + 1).min(m - 1));
            let (fx, fz) = (cx - x0 as f32, cz - z0 as f32);
            let at = |x: usize, z: usize| values[coarse.index(face, x, z)];
            let top = at(x0, z0) + (at(x1, z0) - at(x0, z0)) * fx;
            let bottom = at(x0, z1) + (at(x1, z1) - at(x0, z1)) * fx;
            top + (bottom - top) * fz
        })
        .collect()
}

/// Per-cell results of the landform pass.
struct Cell {
    rock: Rock,
    strata: f32,
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
/// mask: for land cells to the sea, up to `land_reach`, for sea cells to land, up to
/// `sea_reach` (further cells get the reach). Each cell's nearest source is propagated from its neighbours and the distance
/// measured to it directly (stepping along neighbours instead would draw star-shaped rays).
pub fn distance_to(grid: &SphereGrid, sea: &[bool], land_reach: f32, sea_reach: f32) -> Vec<f32> {
    use std::{cmp::Reverse, collections::BinaryHeap};
    let reach = |i: usize| if sea[i] { sea_reach } else { land_reach };
    let mut distance: Vec<f32> = (0..grid.len()).map(reach).collect();
    let mut source = vec![usize::MAX; grid.len()];
    let mut open = BinaryHeap::new();
    // Sources: cells next to the other side (found on all cores).
    let shore = par::map(grid.len(), |i| grid.neighbours(i).any(|(j, _)| sea[j] != sea[i]));
    for i in (0..grid.len()).filter(|&i| shore[i]) {
        distance[i] = 0.5;
        source[i] = i;
        open.push(Reverse((512u32, i)));
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
            if candidate < distance[j] && candidate < reach(j) {
                distance[j] = candidate;
                source[j] = source[i];
                open.push(Reverse(((candidate * 1024.) as u32, j)));
            }
        }
    }
    distance
}

/// Young ranges this close to the sea (cells) are continental arcs (volcanoes).
const COASTAL_ARC: f32 = 60.;

/// Strata: the scale (cells) over which their dip turns.
const STRATA_WAVELENGTH: f32 = 120.;

/// The rock of a land cell and the dip of its strata, from its boundaries (belts, arcs, rifts),
/// old ranges, provinces and a noise (`-1..1`, choosing between the rocks a province can have).
fn rock_of(b: Boundary, old: f32, [plains, hilly, plateau]: [f32; 3], noise: f32) -> (Rock, f32) {
    if b.arc > 0.3 || b.rift > 0.45 {
        // Volcanic arcs, lava flows on rift floors.
        return (Rock::Basalt, 0.);
    }
    if b.belt > 0.5 {
        // The cores of young ranges.
        return (Rock::Metamorphic, 0.);
    }
    if b.foothills > 0.3 || b.shoulders > 0.3 {
        // Strata tilted up against the range: cuestas, hogbacks.
        let rock = if noise > 0.15 { Rock::Limestone } else { Rock::Sandstone };
        return (rock, 0.25 + 0.2 * b.foothills.max(b.shoulders));
    }
    if old > 0.3 {
        // Worn ranges down to their roots.
        return (if noise > -0.35 { Rock::Granite } else { Rock::Metamorphic }, 0.);
    }
    if plateau >= plains.max(hilly) {
        // Flat-lying beds: mesas, stepped canyons.
        return (if noise > 0.45 { Rock::Limestone } else { Rock::Sandstone }, 0.02);
    }
    if hilly >= plains {
        let rock = match noise {
            n if n > 0.15 => Rock::Granite,
            n if n > -0.2 => Rock::Sandstone,
            _ => Rock::Limestone,
        };
        return (rock, 0.08);
    }
    // Plains: shale lowlands, some limestone, and old shields (granite) here and there.
    let rock = match noise {
        n if n > 0.35 => Rock::Granite,
        n if n > 0.2 => Rock::Limestone,
        _ => Rock::Shale,
    };
    (rock, 0.03)
}

/// Lake shores within this distance (world units) stay above their lake's level when
/// floodplains are flattened (as on the flat map, `LAKE_RIM`).
const LAKE_RIM: f32 = 3.;

/// Lower the valley floor to just above the water where rivers meander (as on the flat map,
/// `flatten_floodplains`), so meanders have a flat floodplain to wind through; lake shores stay
/// above their lake. Returns how much each cell is floodplain (0..1).
fn flatten_floodplains(
    grid: &SphereGrid,
    radius: f32,
    heights: &mut [f32],
    lake_depth: &[f32],
    valley: &[crate::planet_rivers::ValleyPoint],
) -> Vec<f32> {
    use crate::planet_rivers::for_cells_within;
    let cells = grid.len();
    // Lowest height allowed per cell: the level of any lake nearby.
    let mut floor = vec![f32::MIN; cells];
    for i in (0..cells).filter(|&i| lake_depth[i] > 0.05) {
        let level = heights[i] + lake_depth[i] + 0.05;
        for_cells_within(grid, radius, grid.direction(i), LAKE_RIM, |n, _| {
            floor[n] = floor[n].max(level);
        });
    }
    let mut plain = vec![0f32; cells];
    // (Every other point: they are half a cell apart, the floodplains several cells wide.)
    for point in valley.iter().step_by(2).filter(|p| p.floodplain > 0.) {
        let reach = point.floodplain * 1.5 + 2.;
        let target = point.level + 0.4;
        for_cells_within(grid, radius, point.direction, reach, |i, distance| {
            if lake_depth[i] > 0.05 || heights[i] <= target {
                return;
            }
            let t = smoothstep(reach * 0.4, reach, distance);
            let lowered = target + (heights[i] - target) * t;
            heights[i] = heights[i].min(lowered.max(floor[i]));
            plain[i] = plain[i].max(1. - t);
        });
    }
    plain
}
