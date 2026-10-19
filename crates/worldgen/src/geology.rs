//! Rock types: what the land is made of, from the planet's tectonic history and provinces
//! (`planet.rs`), and how it resists erosion (`terrain.rs`).
//!
//! Each rock scales the erodibility (how fast rivers cut it) and the hillslope diffusion (how
//! fast its slopes wear back). Sedimentary rocks are layered: beds of hard rock (sandstone,
//! limestone) alternate with soft ones (shale) along strata surfaces `height + offset = const`,
//! where `offset` is a smooth field whose slope is the strata's dip. So the same erosion carves
//! stepped canyons and mesas under hard caprock, wide valleys in soft rock, narrow gorges in
//! massive hard rock, and cuestas or hogbacks where the strata are tilted. The game draws the
//! rock and its strata (bands along the same surfaces) in the terrain shader.
//!
//! Heights here are in erosion units (before the planet's heights are scaled to world units:
//! `Planet::strata_scale`).

/// A rock type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Rock {
    /// Crystalline shields: old continental cores, exposed roots of worn ranges. Massive, hard:
    /// rounded domes and inselbergs, narrow gorges.
    Granite = 0,
    /// Schist and gneiss of mountain belts. Massive, hard, steep.
    Metamorphic = 1,
    /// Lava flows of arcs, hotspots and rifts. Massive, hard: dark cliffs.
    Basalt = 2,
    /// Sandstone beds between shale: plateaus, mesas, stepped canyons.
    Sandstone = 3,
    /// Shale with thin hard beds: soft lowlands, wide valleys, badlands where dry.
    Shale = 4,
    /// Thick limestone beds: cliffs, gorges; karst where wet.
    Limestone = 5,
}

impl Rock {
    pub const ALL: [Rock; 6] =
        [Rock::Granite, Rock::Metamorphic, Rock::Basalt, Rock::Sandstone, Rock::Shale, Rock::Limestone];

    /// The rock of an id (see [`Rock`] as `u8`).
    pub fn from_id(id: u8) -> Rock {
        Rock::ALL.get(id as usize).copied().unwrap_or(Rock::Granite)
    }

    fn properties(self) -> &'static Properties {
        &PROPERTIES[self as usize]
    }

    /// Bed thickness (erosion units; 0: massive rock) and share of the hard beds.
    pub fn bedding(self) -> (f32, f32) {
        let p = self.properties();
        (p.layer, p.hard_share)
    }

    /// Erodibility and diffusion multipliers of this rock at strata height `level` (the
    /// height plus the strata offset, erosion units).
    pub fn hardness(self, level: f32) -> (f32, f32) {
        let p = self.properties();
        let hard = hard_bed(level, p.layer, p.hard_share);
        (
            p.erodibility.1 + (p.erodibility.0 - p.erodibility.1) * hard,
            p.diffusion.1 + (p.diffusion.0 - p.diffusion.1) * hard,
        )
    }

    /// The largest diffusion multiplier (for the erosion's stability limit).
    pub fn max_diffusion() -> f32 {
        PROPERTIES.iter().map(|p| p.diffusion.0.max(p.diffusion.1)).fold(0., f32::max)
    }
}

/// How hard the bed at strata height `level` is (1: hard, 0: soft), with soft edges (sharp
/// steps in erodibility make the erosion ring).
pub fn hard_bed(level: f32, layer: f32, hard_share: f32) -> f32 {
    if layer <= 0. {
        return 1.;
    }
    let phase = (level / layer).rem_euclid(1.);
    let edge = BED_EDGE.min(hard_share / 2.).min((1. - hard_share) / 2.);
    smooth(0., edge, phase) * smooth(hard_share + edge, hard_share - edge, phase)
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Share of a bed over which it turns from hard to soft.
const BED_EDGE: f32 = 0.08;

/// A rock's behaviour under erosion.
struct Properties {
    /// Erodibility multipliers of the hard and soft beds (massive rocks: the first).
    erodibility: (f32, f32),
    /// Hillslope diffusion multipliers of the hard and soft beds.
    diffusion: (f32, f32),
    /// Bed thickness (erosion units; 0: massive) and share of the hard beds.
    layer: f32,
    hard_share: f32,
}

/// By [`Rock`] id. Contrasts matter more than values: soft beds under hard caprock retreat
/// (wide canyons, mesas), massive hard rock keeps narrow gorges.
const PROPERTIES: [Properties; 6] = [
    // Granite.
    Properties { erodibility: (0.35, 0.35), diffusion: (0.7, 0.7), layer: 0., hard_share: 1. },
    // Metamorphic.
    Properties { erodibility: (0.55, 0.55), diffusion: (0.6, 0.6), layer: 0., hard_share: 1. },
    // Basalt.
    Properties { erodibility: (0.45, 0.45), diffusion: (0.45, 0.45), layer: 0., hard_share: 1. },
    // Sandstone.
    Properties { erodibility: (0.3, 1.6), diffusion: (0.12, 1.8), layer: 6., hard_share: 0.4 },
    // Shale.
    Properties { erodibility: (0.6, 1.8), diffusion: (0.5, 2.), layer: 4., hard_share: 0.18 },
    // Limestone.
    Properties { erodibility: (0.5, 1.2), diffusion: (0.25, 1.), layer: 9., hard_share: 0.7 },
];
