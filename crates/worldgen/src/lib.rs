//! World generation for the living planet (see `docs/living-world.md`), independent of the
//! game engine: the planet ([`planet`]: tectonic history, shape, erosion, coasts, lakes,
//! rivers, mouths), its climate ([`climate`]), the sea's wave field ([`sea`]),
//! springs ([`springs`]) and the surface the game draws ([`surface`]).
//!
//! Its features each have a module: rock types ([`geology`]), volcanoes, reefs, barrier
//! islands, fjords, glaciers, deltas and estuaries (`mouths`, with `channels`). The erosion
//! (`terrain`), flow routing (`flow`) and the parallel passes (`par`) work on the cube-sphere
//! grid ([`sphere`], through the [`grid::Grid`] trait).

pub mod climate;
pub mod geology;
pub mod glaciers;
pub mod grid;
pub mod noise;
pub mod planet;
pub mod rivers;
pub mod sea;
pub mod sphere;
pub mod springs;
pub mod surface;
pub mod tectonics;
pub mod volcanoes;

mod barriers;
mod channels;
mod detail;
mod fjords;
mod flow;
mod lakes;
mod mouths;
mod par;
mod reefs;
mod terrain;

use bevy_math::Vec3;

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
    /// Stony desert (reg): gravel, clay flats, shrubs, cacti.
    Desert,
    /// Sand sea (erg): dunes, almost bare.
    Erg,
    /// Badlands: soft rock gullied bare in dry climates, a few shrubs.
    Badlands,
    DeciduousForest,
    ConiferForest,
    /// Tropical rainforest (Af, Am): jungle trees, bamboo, ferns.
    Rainforest,
    /// Tropical mountain forest (cool and wet at altitude): jungle trees and conifers.
    CloudForest,
    /// Tropical wet and dry (Aw): grass with scattered acacias.
    Savanna,
    /// River banks in dry country: gallery forests (poplars, willows) and, where it is hot,
    /// palm groves.
    Riparian,
    Oasis,
    /// Sheltered tropical shores: mangroves standing in the shallows.
    Mangrove,
    /// Polar lowlands (ET): low shrubs, moss, stones; no trees.
    Tundra,
    /// Above the tree line on mountains (ET away from the poles): grass, flowers, rocks.
    AlpineMeadow,
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
            Biome::Desert => [0.70, 0.58, 0.44],
            Biome::Erg => [0.92, 0.74, 0.48],
            Biome::Badlands => [0.74, 0.50, 0.38],
            Biome::DeciduousForest => [0.26, 0.48, 0.18],
            Biome::ConiferForest => [0.17, 0.34, 0.20],
            Biome::Rainforest => [0.14, 0.38, 0.14],
            Biome::CloudForest => [0.20, 0.42, 0.24],
            Biome::Savanna => [0.66, 0.64, 0.32],
            Biome::Mangrove => [0.30, 0.40, 0.22],
            Biome::Riparian => [0.34, 0.52, 0.24],
            Biome::Oasis => [0.38, 0.54, 0.24],
            Biome::Tundra => [0.56, 0.58, 0.48],
            Biome::AlpineMeadow => [0.52, 0.62, 0.38],
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

/// A point of a river channel (for meshing it, and where a delta starts).
#[derive(Clone, Copy, Debug)]
pub struct RiverPoint {
    /// Position of the water surface at the channel center.
    pub position: Vec3,
    /// Channel width, world units.
    pub width: f32,
    /// Channel depth below the water surface, world units.
    pub depth: f32,
}
