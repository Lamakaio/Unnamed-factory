//! World generation for the living planet (see `docs/living-world.md`), independent of the
//! game engine: the planet ([`planet`]: tectonic history, shape, erosion, coasts, lakes,
//! rivers, mouths), its climate ([`planet_climate`]), the sea's wave field ([`planet_sea`]),
//! springs ([`planet_springs`]) and the surface the game draws ([`surface`]).
//!
//! The erosion ([`terrain`]), flow routing and noise are generic over grids ([`grid`]): the
//! planet uses the cube-sphere ([`sphere`]). ([`WorldgenSettings`] holds their parameters: the
//! former flat continent's, scaled by the planet.)

pub(crate) mod detail;
pub mod geology;
mod flow;
pub mod grid;
pub mod sphere;
pub mod surface;
pub mod tectonics;
pub mod volcanoes;
mod heightmap;
mod mouths;
pub mod noise;
mod par;
pub mod planet;
pub mod planet_climate;
mod planet_lakes;
mod planet_mouths;
pub mod planet_sea;
pub mod planet_springs;
mod reefs;
pub mod planet_rivers;
mod terrain;


use bevy_math::Vec3;


/// Generation parameters. The defaults give a 512×512 unit continent.
#[derive(Clone, Debug)]
pub struct WorldgenSettings {
    pub seed: u64,
    /// Cells per side.
    pub size: usize,
    /// Cell size, in world units.
    pub cell_size: f32,
    /// Radius of the land mass, as a fraction of the half size.
    pub land_radius: f32,
    /// Number of tectonic plates (provinces).
    pub plate_count: usize,
    /// Relative frequency of plate interiors: `[plains, hill country, plateau]`.
    pub province_weights: [f32; 3],
    /// Width (cells) of the blend between neighbouring provinces.
    pub province_blend: f32,
    /// Half-width (cells) of mountain range belts.
    pub range_width: f32,
    /// Distance (cells) between the parallel ridges of a range.
    pub ridge_spacing: f32,
    /// Height (world units) of plateau tops.
    pub plateau_level: f32,
    /// Height of the highest summit, in world units.
    pub max_height: f32,
    /// Depth of the open sea, in world units.
    pub sea_depth: f32,
    /// How much the land rises from the coast to 100 cells inland, in world units.
    pub inland_rise: f32,
    pub erosion_steps: usize,
    pub erosion_dt: f32,
    /// Stream power erodibility K.
    pub erodibility: f32,
    /// Stream power area exponent m (slope exponent n = 1).
    pub area_exponent: f32,
    /// Base hillslope diffusion D (scaled per province).
    pub diffusion: f32,
    /// Random weight (0..) on slopes when routing flow during erosion, against grid artifacts.
    pub routing_jitter: f32,
    /// Prevailing wind direction (radians); random from the seed when `None`.
    pub wind_angle: Option<f32>,
    /// Fraction of the air's moisture raining per cell on flat land.
    pub base_rain: f32,
    /// Extra rain fraction per unit of rise along the wind.
    pub orographic_rain: f32,
    /// Fraction of the rain over land that evaporates back into the air.
    pub land_evaporation: f32,
    /// Height (world units) where it gets cold enough for permanent snow.
    pub snow_line: f32,
    /// Drainage area (cells) where rivers start.
    pub river_min_area: f32,
    /// Width of a river where it starts, in cells (grows with the square root of discharge).
    pub river_width: f32,
    /// Depth of a river where it starts, in world units.
    pub river_depth: f32,
    /// River width (world units) from which mouths can build deltas.
    pub delta_min_width: f32,
    /// Meander amplitude, in channel widths.
    pub meander_amplitude: f32,
    /// Valley slope above which rivers don't meander.
    pub meander_max_slope: f32,
    /// Fine surface samples per cell side.
    pub detail_resolution: usize,
    /// Wavelength (world units) of the largest gullies.
    pub detail_wavelength: f32,
    pub detail_octaves: u32,
    /// Gully depth (world units) on the most rugged slopes.
    pub detail_amplitude: f32,
}

impl Default for WorldgenSettings {
    fn default() -> Self {
        Self {
            seed: 1082,
            size: 512,
            cell_size: 1.,
            land_radius: 0.8,
            plate_count: 10,
            province_weights: [0.35, 0.4, 0.25],
            province_blend: 20.,
            range_width: 28.,
            ridge_spacing: 13.,
            plateau_level: 17.,
            max_height: 38.,
            sea_depth: 20.,
            inland_rise: 3.,
            erosion_steps: 60,
            erosion_dt: 2.,
            erodibility: 0.25,
            area_exponent: 0.45,
            diffusion: 0.05,
            routing_jitter: 1.,
            wind_angle: None,
            base_rain: 0.012,
            orographic_rain: 0.1,
            land_evaporation: 0.6,
            snow_line: 34.,
            river_min_area: 400.,
            river_width: 0.5,
            river_depth: 0.4,
            delta_min_width: 2.,
            meander_amplitude: 2.5,
            meander_max_slope: 0.06,
            detail_resolution: 4,
            detail_wavelength: 8.,
            detail_octaves: 3,
            detail_amplitude: 1.2,
        }
    }
}

/// Biome of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Biome {
    Sea,
    Lake,
    Beach,
    /// Tropical coasts and reef islets: palms.
    TropicalBeach,
    Marsh,
    Meadow,
    Steppe,
    /// Driest warm land: sand, clay flats, cacti.
    Desert,
    DeciduousForest,
    ConiferForest,
    Alpine,
    Rock,
    Snow,
}

impl Biome {
    /// Ground color (sRGB, 0..1), shared by previews and the game.
    pub fn color(self) -> [f32; 3] {
        match self {
            Biome::Sea => [0.45, 0.42, 0.33],
            Biome::Lake => [0.35, 0.33, 0.25],
            Biome::Beach => [0.86, 0.80, 0.60],
            Biome::TropicalBeach => [0.95, 0.92, 0.84],
            Biome::Marsh => [0.36, 0.42, 0.22],
            Biome::Meadow => [0.50, 0.68, 0.30],
            Biome::Steppe => [0.72, 0.68, 0.40],
            Biome::Desert => [0.90, 0.74, 0.50],
            Biome::DeciduousForest => [0.26, 0.48, 0.18],
            Biome::ConiferForest => [0.17, 0.34, 0.20],
            Biome::Alpine => [0.55, 0.60, 0.45],
            Biome::Rock => [0.48, 0.45, 0.42],
            Biome::Snow => [0.94, 0.95, 0.97],
        }
    }
}

/// Where a river ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiverEnd {
    Sea,
    Lake(usize),
    /// Joins another river (its index, always before this one).
    Junction(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct RiverPoint {
    /// Position of the water surface at the channel center.
    pub position: Vec3,
    /// Channel width, world units.
    pub width: f32,
    /// Channel depth below the water surface, world units.
    pub depth: f32,
}
