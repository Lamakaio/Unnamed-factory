//! The planet's shape before erosion, and the erosion: plates (or a tectonic history) and
//! their boundaries (belts, arcs, rifts, ridges, trenches), provinces of plains, hills and
//! plateaus, old worn ranges; from them each cell's inputs of erosion (uplift, erodibility,
//! rock...) on a coarse grid and the full one; then erosion on the coarse grid, refined on the
//! full one.

use std::time::Duration;

use bevy_math::Vec3;
use rand::{RngExt, SeedableRng, rngs::StdRng};

use super::{Laps, PlanetSettings};
use crate::{
    geology::Rock,
    grid::Grid,
    noise::{Noise, hash, smoothstep},
    par,
    sphere::SphereGrid,
    terrain::{self, Shape},
};

/// distance (world units). (Its fronts can run straight for hundreds of cells.)
pub(super) const HISTORY_WARP: [(f32, f32); 2] = [(160., 70.), (50., 40.)];
/// Frequency of the continent outline noise on the unit sphere.
pub(super) const CONTINENT_FREQUENCY: f32 = 2.4;
/// How much a plate's crust raises (continental) or lowers (oceanic) the continent outline.
pub(super) const CONTINENTAL_BIAS: f32 = 0.22;
/// Subsidence of rift floors (relative, like uplift).
pub(super) const RIFT_SUBSIDENCE: f32 = 0.3;
/// Uplift of the low swells over all land (relative, like the provinces' uplift).
pub(super) const PLAIN_SWELLS: f32 = 0.08;
/// they are this value.
pub(super) const COAST_REACH: f32 = 64.;
pub(super) const SHORE_REACH: f32 = 8.;
/// Distance over which the plates' crust bias blends across their boundaries (cells).
pub(super) const CRUST_BLEND: f32 = 25.;
/// Crust value margin below the coast that is still continental (the shelf).
pub(super) const SHELF_BAND: f32 = 0.12;

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
                cells
                    .iter()
                    .flat_map(|&c| buckets[c].iter().copied())
                    .collect()
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
        nearest_k(
            self.candidates(direction)
                .iter()
                .map(|&k| (points[k].center, k)),
            direction,
            radius,
        )
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
pub(super) struct Boundary {
    /// Young mountain belt (0..1), and the distance across it to the boundary (cells).
    pub(super) belt: f32,
    pub(super) across: f32,
    /// Foothills around belts.
    pub(super) foothills: f32,
    /// Rift in a continent (subsiding floor, raised shoulders).
    pub(super) rift: f32,
    pub(super) shoulders: f32,
    /// Island arc (volcanic islands on the overriding ocean plate).
    pub(super) arc: f32,
    /// Sea floor: trench in front of a subduction zone, mid-ocean ridge.
    pub(super) trench: f32,
    pub(super) ridge: f32,
}

// A tectonic history's fields, smoothed, for shaping.
struct HistoryFields {
    crust: Vec<f32>,
    young: Vec<f32>,
    old: Vec<f32>,
    age: Vec<f32>,
}

/// Uplift younger than this many tectonic steps makes young (sharp) ranges.
const YOUNG_STEPS: f32 = 12.;

/// Shaping results on one grid.
pub(super) struct Shaped {
    pub(super) shape: Shape,
    pub(super) boundaries: Vec<Boundary>,
    pub(super) plate_of: Vec<u16>,
    pub(super) crust: Vec<f32>,
    pub(super) coast: f32,
    pub(super) sea: Vec<bool>,
    pub(super) continental: Vec<bool>,
    /// Distance to the coast (full-size cells, up to `COAST_REACH`).
    pub(super) to_sea: Vec<f32>,
}

/// What shapes the planet: its plates (or tectonic history), provinces and ancient blocks,
/// and the noises shaping uses.
pub(super) struct Shaper<'a> {
    settings: &'a PlanetSettings,
    /// The planet's radius (cells).
    radius: f32,
    plates: Vec<Plate>,
    provinces: Vec<Region>,
    province_index: PointIndex,
    cratons: Vec<Region>,
    craton_index: PointIndex,
    /// The tectonic history, if any, and its smoothed fields.
    history: Option<(crate::tectonics::Tectonics, HistoryFields)>,
    continents: Noise,
    peaks: Noise,
    hills: Noise,
    warp: Noise,
    arcs: Noise,
}

impl<'a> Shaper<'a> {
    pub(super) fn new(
        settings: &'a PlanetSettings,
        grid: &SphereGrid,
        radius: f32,
        noise: Noise,
    ) -> Self {
        let cells = grid.len();
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
        let regions =
            |count: usize, kinds: &[f32], rng: &mut StdRng, value: &dyn Fn(&mut StdRng) -> f32| {
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
            let young: Vec<f32> = (0..t.grid.len())
                .map(|i| t.orogeny[i] * (-t.orogeny_age[i] / YOUNG_STEPS).exp())
                .collect();
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
        Self {
            settings,
            radius,
            plates,
            provinces,
            province_index,
            cratons,
            craton_index,
            history,
            continents: noise.layer(1),
            peaks: noise.layer(2),
            hills: noise.layer(3),
            warp: noise.layer(4),
            arcs: noise.layer(6),
        }
    }

    /// Half-width of the mountain belts at a point (world units).
    fn width_at(&self, p: Vec3) -> f32 {
        self.settings.range_width * (0.8 + 0.45 * self.peaks.layer(1).fbm3(p / 120., 2))
    }

    /// Where to sample the (coarse) tectonic history for a point (world units): warped, so its
    /// fields' straight bilinear contours wander.
    fn history_direction(&self, p: Vec3) -> Vec3 {
        let wide = self
            .warp
            .layer(7)
            .warp3(p / HISTORY_WARP[0].0, 3, HISTORY_WARP[0].1);
        let short = self
            .warp
            .layer(8)
            .warp3(p / HISTORY_WARP[1].0, 2, HISTORY_WARP[1].1);
        (p + wide + short).normalize()
    }

    /// The surface velocity of the plate under a direction (`plate_of` from shaping).
    pub(super) fn velocity(&self, grid: &SphereGrid, plate_of: &[u16], d: Vec3) -> Vec3 {
        match &self.history {
            Some((t, _)) => t.plates[t.plate[t.grid.cell_at(d)] as usize]
                .rotation
                .cross(d),
            None => self.plates[plate_of[grid.cell_at(d)] as usize].velocity(d),
        }
    }

    /// Whether each plate is continental (a history's: if a good share of its crust is).
    pub(super) fn continental_plates(&self) -> Vec<bool> {
        match &self.history {
            Some((t, _)) => (0..t.plates.len())
                .map(|k| {
                    let cells = (0..t.grid.len()).filter(|&i| t.plate[i] as usize == k);
                    let (total, land) =
                        cells.fold((0, 0), |(n, l), i| (n + 1, l + (t.crust[i] > 0.5) as usize));
                    land * 3 > total
                })
                .collect(),
            None => self.plates.iter().map(|p| p.continental).collect(),
        }
    }

    /// Shaping on a grid whose cells are `scale` full-size cells long (all lengths are in
    /// full-size cells: noise, plates and provinces look the same on any grid).
    pub(super) fn shape(&self, grid: &SphereGrid, scale: f32) -> Shaped {
        let Shaper {
            settings,
            plates,
            provinces,
            province_index,
            cratons,
            craton_index,
            history,
            ..
        } = self;
        let (radius, continents, peaks, hills, warp, arcs) = (
            self.radius,
            self.continents,
            self.peaks,
            self.hills,
            self.warp,
            self.arcs,
        );
        let width_at = |p: Vec3| self.width_at(p);
        let history_direction = |p: Vec3| self.history_direction(p);
        let cells = grid.len();
        // Pass 1: plates, boundaries, crust.
        let pass1: Vec<(u16, f32, Boundary)> = par::map(cells, |i| {
            let direction = grid.direction(i);
            let p = direction * radius;
            let w = (p + warp.warp3(p / 180., 3, 45.)).normalize();
            let (nearest, count) = nearest_k::<4>(
                plates.iter().enumerate().map(|(k, p)| (p.center, k)),
                w,
                radius,
            );
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
            let bias = |k: usize| {
                if plates[k].continental {
                    CONTINENTAL_BIAS
                } else {
                    -CONTINENTAL_BIAS
                }
            };
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
                // (Sampled at a warped direction, and blended across the continents' edge: the
                // coarse history's fields, bilinear, have long straight contours.)
                let h = history_direction(p);
                let crust = t.grid.bilinear(&fields.crust, h);
                let young = t.grid.bilinear(&fields.young, h);
                let ocean_age = t.grid.bilinear(&fields.age, h);
                let continental = smoothstep(0.4, 0.6, crust);
                boundary.belt = continental * smoothstep(0.06, 0.6, young);
                // (Ridges run parallel to the belt: the belt's strength falls steadily from its
                // axis, so its contours stand in for the distance across it.)
                boundary.across = (1. - smoothstep(0.03, 0.9, young)) * width * 1.4;
                boundary.foothills = continental * smoothstep(0.01, 0.15, young);
                boundary.arc = (1. - continental) * smoothstep(0.1, 0.6, young);
                boundary.ridge = (1. - continental) * (-ocean_age / 4.).exp();
                let peaks_here = smoothstep(0.78, 0.95, arcs.ridged3(p / 16., 2));
                let stretches = smoothstep(0.05, 0.35, arcs.layer(1).fbm3(p / 70., 2));
                let value = (crust - 0.5) * 1.2
                    + outline * 0.3
                    + boundary.arc * peaks_here * stretches * 1.2;
                let plate = t.plate[t.grid.cell_at(direction)];
                return (plate, value, boundary);
            }
            let here_continental = base + outline > 0.;
            for &(dk, k) in &nearest[1..] {
                let x = (dk - d1) / 2.;
                let axis = (plates[k].center - plates[a].center)
                    .reject_from(direction)
                    .normalize_or_zero();
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
                    boundary.rift = boundary
                        .rift
                        .max(divergence * (-(x / (width * 0.5)).powi(2)).exp());
                    boundary.shoulders = boundary
                        .shoulders
                        .max(divergence * (-((x - width * 0.8) / (width * 0.25)).powi(2)).exp());
                } else {
                    boundary.ridge = boundary
                        .ridge
                        .max(divergence * (-(x / (width * 2.)).powi(2)).exp());
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
            let phase = (b.across
                + 0.25 * settings.ridge_spacing * peaks.layer(2).fbm3(p / 50., 3))
                / settings.ridge_spacing;
            let parallel = (0.5 + 0.5 * (phase * std::f32::consts::TAU).cos()).powf(1.5);
            let summits = peaks.ridged3(p / 32., 4);
            let young = b.belt * (0.3 + 0.7 * (0.5 * parallel + 0.5 * summits));
            let foothills = b.foothills * (0.5 + 0.5 * hills.layer(5).fbm3(p / 30., 3).abs());

            // Old ranges: with a history, its old uplift (worn the more the older); else along the
            // sutures between ancient blocks.
            let w = (p + warp.layer(3).warp3(p / 140., 3, 60.)).normalize();
            let (near, count) = craton_index.nearest::<4>(cratons, w, radius);
            let near = &near[..count];
            let (mut old, mut age) = (0f32, 0f32);
            if let Some((t, fields)) = &history {
                let uplift = t.grid.bilinear(&fields.old, history_direction(p));
                old = smoothstep(0.05, 0.6, uplift);
                age = 0.6;
            } else if near.len() >= 2 {
                let (d1, a) = near[0];
                for &(dk, k) in &near[1..] {
                    let (lo, hi) = (a.min(k) as u32, a.max(k) as u32);
                    let suture =
                        hash(settings.seed as u32 ^ 0x01D, lo, hi) as f32 / u32::MAX as f32;
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
            let w =
                (p + warp.layer(1).warp3(p / 160., 3, 70.) + warp.layer(2).warp3(p / 50., 2, 15.))
                    .normalize();
            let (near, count) = province_index.nearest::<4>(provinces, w, radius);
            let near = &near[..count];
            let (mut weights, mut cap, mut total) = ([0f32; 3], 0., 0.);
            for &(d, k) in near {
                let weight = (-(d - near[0].0) / settings.province_blend).exp();
                weights[provinces[k].kind] += weight;
                cap += provinces[k].value * weight;
                total += weight;
            }
            let [mut plains, mut hilly, mut plateau] =
                weights.map(|weight| weight / total.max(1e-6));
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
            let noise = hills.layer(12).fbm3(p / 90., 3);
            let (mut rock, dip) = rock_of(b, old, [plains, hilly, plateau], noise);
            // Limestone is laid down by life (shells, reefs, microbial mats): without it,
            // sandstone and shale.
            if rock == Rock::Limestone && !self.settings.world.life {
                rock = if noise > 0.3 {
                    Rock::Sandstone
                } else {
                    Rock::Shale
                };
            }
            // Strata surfaces `height + offset = const`: the offset is a smooth field whose slope
            // is the dip (gentle on plateaus, steep in the foothills of ranges).
            let strata = dip * STRATA_WAVELENGTH * hills.layer(13).fbm3(p / STRATA_WAVELENGTH, 2);
            Cell {
                rock,
                strata,
                uplift,
                // Rift floors sink in segments along the axis (a chain of basins, rift lakes).
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
                height: if sea[i] {
                    0.
                } else {
                    land * (0.5 + hills.layer(8).fbm3(p / 40., 4) * 0.3)
                },
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
    }
}

/// Shape the planet and erode it: the first `coarse_steps` on a grid `coarse_factor` times
/// coarser (the large drainage network forms there, at a fraction of the cost), upsampled,
/// the rest at full resolution. Returns the full grid's shaping and the eroded heights
/// (erosion units).
pub(super) fn shape_and_erode(
    settings: &PlanetSettings,
    grid: &SphereGrid,
    shaper: &Shaper,
    laps: &mut Laps,
) -> (Shaped, Vec<f32>) {
    let erosion = |steps: usize, scale: f32| terrain::ErosionSettings {
        seed: settings.seed,
        steps,
        dt: settings.erosion_dt,
        erodibility: settings.erodibility,
        area_exponent: settings.area_exponent,
        routing_jitter: settings.routing_jitter,
        cell_size: scale,
    };
    let report = |name: &'static str, t: &terrain::ErosionTimings, laps: &mut Laps| {
        laps.timings.extend([
            (
                name,
                t.routing + t.accumulation + t.incision + t.glaciers + t.diffusion,
            ),
            ("    routing", t.routing),
            ("    accumulation", t.accumulation),
            ("    incision", t.incision),
            ("    glaciers", t.glaciers),
            ("    diffusion", t.diffusion),
        ]);
    };
    let coarse_steps = settings.coarse_steps.min(settings.erosion_steps);
    let mut shaped = None;
    if coarse_steps > 0 && settings.coarse_factor > 1 {
        let factor = settings.coarse_factor;
        let coarse_grid = SphereGrid::new(settings.n / factor);
        let coarse = shaper.shape(&coarse_grid, factor as f32);
        let mut fine = shaper.shape(grid, 1.);
        laps.lap("shape");
        let (coarse_heights, t) = terrain::erode_timed(
            &coarse_grid,
            &erosion(coarse_steps, factor as f32),
            &coarse.shape,
        );
        report("  coarse erosion", &t, laps);
        // The fine grid starts from the coarse result (its land only).
        let upsampled = upsample(&coarse_grid, &coarse_heights, grid);
        for (i, h) in fine.shape.heights.iter_mut().enumerate() {
            *h = if fine.sea[i] {
                0.
            } else {
                upsampled[i].max(0.)
            };
        }
        shaped = Some(fine);
    }
    let shaped = shaped.unwrap_or_else(|| {
        let shaped = shaper.shape(grid, 1.);
        laps.lap("shape");
        shaped
    });
    let (heights, t) = terrain::erode_timed(
        grid,
        &erosion(settings.erosion_steps - coarse_steps, 1.),
        &shaped.shape,
    );
    report("  fine erosion", &t, laps);
    laps.lap("erosion");
    // ("erosion" is the sum of both stages, listed before them.)
    let total: Duration = laps.timings.pop().unwrap().1;
    let at = laps
        .timings
        .iter()
        .position(|t| t.0.starts_with("  coarse") || t.0.starts_with("  fine"))
        .unwrap_or(laps.timings.len());
    laps.timings.insert(at, ("erosion", total));
    (shaped, heights)
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
fn distance_to(grid: &SphereGrid, sea: &[bool], land_reach: f32, sea_reach: f32) -> Vec<f32> {
    use std::{cmp::Reverse, collections::BinaryHeap};
    let reach = |i: usize| if sea[i] { sea_reach } else { land_reach };
    let mut distance: Vec<f32> = (0..grid.len()).map(reach).collect();
    let mut source = vec![usize::MAX; grid.len()];
    let mut open = BinaryHeap::new();
    // Sources: cells next to the other side (found on all cores).
    let shore = par::map(grid.len(), |i| {
        grid.neighbours(i).any(|(j, _)| sea[j] != sea[i])
    });
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
        let rock = if noise > 0.15 {
            Rock::Limestone
        } else {
            Rock::Sandstone
        };
        return (rock, 0.25 + 0.2 * b.foothills.max(b.shoulders));
    }
    if old > 0.3 {
        // Worn ranges down to their roots.
        return (
            if noise > -0.35 {
                Rock::Granite
            } else {
                Rock::Metamorphic
            },
            0.,
        );
    }
    if plateau >= plains.max(hilly) {
        // Flat-lying beds: mesas, stepped canyons.
        return (
            if noise > 0.45 {
                Rock::Limestone
            } else {
                Rock::Sandstone
            },
            0.02,
        );
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
