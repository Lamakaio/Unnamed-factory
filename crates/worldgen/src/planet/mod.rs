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
//! plateaus (plateaus away from the coasts, lowlands toward them). Then stream power erosion
//! (`terrain::erode_timed`, on the [`SphereGrid`]).
//!
//! Lengths are in cells, so the landforms keep their size; the sphere's
//! radius is `2n/π` cells. Planet-scale features (continents) are sized relative to it.

mod coast;
mod shape;
mod water;

use std::time::{Duration, Instant};

use bevy_math::Vec3;

use crate::{geology::Rock, grid::Grid, noise::Noise, sphere::SphereGrid};

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
    /// (`[plains, hills, plateau]`) and how softly they blend (cells).
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
    /// sea (world units, about one per cell).
    pub max_height: f32,
    pub sea_depth: f32,
    /// Erosion: steps, time step, erodibility, drainage area exponent, hillslope diffusion,
    /// routing jitter (see `terrain::ErosionSettings`).
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
        Self {
            seed: 1082,
            n: 256,
            land_fraction: 0.3,
            plate_count: 12,
            continental_plates: 0.4,
            history: Some(crate::tectonics::TectonicSettings::default()),
            province_cells: 8_000.,
            province_weights: [0.35, 0.45, 0.2],
            province_blend: 20.,
            plateau_level: 17.,
            craton_cells: 20_000.,
            old_range_share: 0.5,
            range_width: 28.,
            ridge_spacing: 13.,
            max_height: 38.,
            sea_depth: 20.,
            erosion_steps: 60,
            coarse_steps: 45,
            coarse_factor: 2,
            erosion_dt: 2.,
            erodibility: 0.25,
            area_exponent: 0.45,
            diffusion: 0.05,
            routing_jitter: 1.,
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
    /// and the dry basins alone (no outlet: salt flats where dry). See `lakes`.
    pub wetland: Vec<f32>,
    pub basins: Vec<f32>,
    /// The rivers (see `rivers`).
    pub rivers: Vec<crate::rivers::PlanetRiver>,
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

/// The generation's stages and how long each took.
struct Laps {
    timings: Vec<(&'static str, Duration)>,
    stage: Instant,
}

impl Laps {
    fn new() -> Self {
        Self {
            timings: Vec::new(),
            stage: Instant::now(),
        }
    }

    /// The stage `name` is done.
    fn lap(&mut self, name: &'static str) {
        self.timings.push((name, self.stage.elapsed()));
        self.stage = Instant::now();
    }
}

/// Generate the planet: shape and erosion (`shape`), the coasts and sea floor (`coast`), the
/// features on them (fjords, volcanoes, reefs, barrier islands), then its water (`water`).
pub fn generate_planet(settings: &PlanetSettings) -> Planet {
    let mut laps = Laps::new();
    let grid = SphereGrid::new(settings.n);
    let radius = 2. * settings.n as f32 / std::f32::consts::PI;
    let noise = Noise::new(settings.seed as u32);

    let shaper = shape::Shaper::new(settings, &grid, radius, noise);
    let (shaped, heights) = shape::shape_and_erode(settings, &grid, &shaper, &mut laps);
    let coast::Coasts {
        mut heights,
        strata_scale,
        shore,
    } = coast::shape_coasts(settings, &grid, radius, noise, &shaped, heights);
    let mut sea = shaped.sea.clone();
    let Features {
        volcanoes,
        volcanic,
        rock,
        strata,
        reef,
    } = add_features(
        settings,
        &grid,
        radius,
        &shaper,
        &shaped,
        &mut heights,
        &mut sea,
        &mut laps,
    );
    let ruggedness: Vec<f32> = (0..grid.len())
        .map(|i| shaped.shape.ruggedness(i).max(volcanic.ruggedness[i]))
        .collect();
    let water::Water {
        drainage,
        lake_depth,
        wetland,
        basins,
        rivers,
        calm,
    } = water::shape_water(
        &grid,
        radius,
        settings.seed,
        noise,
        &shore,
        &mut heights,
        &mut sea,
        &mut laps,
    );

    Planet {
        settings: settings.clone(),
        grid,
        heights,
        sea,
        continental: shaped.continental,
        range: shaped.shape.range,
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
        plate: shaped.plate_of,
        continental_plate: shaper.continental_plates(),
        timings: laps.timings,
    }
}

/// Young ranges this close to the sea (cells) are continental arcs (volcanoes).
const COASTAL_ARC: f32 = 60.;

/// What the features leave: the volcanoes and what they built, the rock of each cell and its
/// strata offset (basalt and flat on volcanoes), the coral reefs.
struct Features {
    volcanoes: Vec<crate::volcanoes::Volcano>,
    volcanic: crate::volcanoes::Built,
    rock: Vec<u8>,
    strata: Vec<f32>,
    reef: Vec<f32>,
}

/// Fjords, volcanoes, coral reefs and barrier islands, on the shaped coasts.
fn add_features(
    settings: &PlanetSettings,
    grid: &SphereGrid,
    radius: f32,
    shaper: &shape::Shaper,
    shaped: &shape::Shaped,
    heights: &mut [f32],
    sea: &mut [bool],
    laps: &mut Laps,
) -> Features {
    let shape::Shaped {
        shape,
        boundaries,
        plate_of,
        to_sea,
        ..
    } = shaped;
    let cells = grid.len();
    // Fjords: glacial troughs flooded by the sea at cold, mountainous coasts.
    crate::fjords::carve(grid, radius, settings.seed, heights, sea);
    laps.lap("  fjords");
    // Volcanoes: island and continental arcs, hotspot chains along the plates' motion.
    let velocity = |d: Vec3| shaper.velocity(grid, plate_of, d);
    let volcanoes = crate::volcanoes::place(
        grid,
        radius,
        settings.seed,
        |i| boundaries[i].arc,
        |i| {
            if !sea[i] && boundaries[i].belt > 0.4 && to_sea[i] < COASTAL_ARC {
                1.
            } else {
                0.
            }
        },
        velocity,
        |i| sea[i],
    );
    let volcanic = crate::volcanoes::build(grid, radius, settings.seed, &volcanoes, heights, sea);
    let mut rock: Vec<u8> = shape.rock.iter().map(|&r| r as u8).collect();
    let mut strata = shape.strata.clone();
    for i in 0..cells {
        if volcanic.volcanic[i] {
            rock[i] = Rock::Basalt as u8;
            strata[i] = 0.;
        }
    }
    // Coral reefs in the tropics: fringing and barrier reefs, atolls on sunk volcanoes.
    let reef = crate::reefs::build(grid, radius, settings.seed, &volcanoes, heights, sea);
    laps.lap("  volcanoes");
    // Barrier islands and lagoons on low coasts with a shallow shelf.
    crate::barriers::build(grid, radius, settings.seed, &reef, heights, sea, &mut rock);
    laps.lap("  barrier islands");
    Features {
        volcanoes,
        volcanic,
        rock,
        strata,
        reef,
    }
}
