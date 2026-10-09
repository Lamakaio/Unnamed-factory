//! Terrain edits (building foundations, terraforming tools), stored as operations and evaluated
//! at sampling time: they stay smooth at any mesh resolution and are non-destructive.

use bevy::prelude::*;

use super::{GRID_SQUARE_SIZE, MIN_CELL, PatchOp, spatial::SpatialGrid};

/// Height change of one raise / lower tool use, in world units.
const RAISE_AMOUNT: f32 = 10.;
/// Cell size of the edit lookup grid, in world units.
const LOOKUP_CELL: f32 = 8.;
/// Fraction of a flatten radius that is fully flat; the rest blends into the terrain.
const FLATTEN_PLATEAU: f32 = 0.6;
/// Mesh cells across an edit's falloff band.
const CELLS_PER_FALLOFF: f32 = 8.;

/// One terrain modification, applied over a disc with a smooth falloff.
#[derive(Clone, Debug)]
pub enum TerrainEdit {
    /// Blend the terrain towards `height` (building foundations).
    Flatten {
        center: Vec2,
        radius: f32,
        height: f32,
    },
    /// Add `amount` at the center, fading out towards the radius.
    Raise {
        center: Vec2,
        radius: f32,
        amount: f32,
    },
}

impl TerrainEdit {
    /// The edit made by a placement operation centered on `center` (world position).
    pub fn from_op(op: PatchOp, center: Vec3, radius: f32) -> Option<Self> {
        match op {
            PatchOp::Flatten => Some(Self::Flatten {
                center: center.xz(),
                radius,
                height: center.y,
            }),
            PatchOp::Up | PatchOp::Down => Some(Self::Raise {
                center: center.xz(),
                radius,
                amount: if matches!(op, PatchOp::Down) {
                    -RAISE_AMOUNT
                } else {
                    RAISE_AMOUNT
                },
            }),
            PatchOp::Smooth => None,
        }
    }

    fn center_radius(&self) -> (Vec2, f32) {
        match *self {
            Self::Flatten { center, radius, .. } | Self::Raise { center, radius, .. } => {
                (center, radius)
            }
        }
    }

    /// Rectangle affected by the edit.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let (center, radius) = self.center_radius();
        (center - radius, center + radius)
    }

    /// Width of the band where the edit blends into the terrain.
    fn falloff_width(&self) -> f32 {
        match *self {
            Self::Flatten { radius, .. } => radius * (1. - FLATTEN_PLATEAU),
            Self::Raise { radius, .. } => radius,
        }
    }

    /// Mesh cell size needed to render the edit smoothly.
    pub fn detail_cell(&self) -> f32 {
        (self.falloff_width() / CELLS_PER_FALLOFF).clamp(MIN_CELL, GRID_SQUARE_SIZE)
    }

    fn apply(&self, p: Vec2, height: &mut f32) {
        let (center, radius) = self.center_radius();
        let distance = p.distance(center);
        if distance > radius {
            return;
        }
        // C2-smooth falloffs, so the edit blends into the terrain without creases.
        match *self {
            Self::Flatten { height: target, .. } => {
                let plateau = radius * FLATTEN_PLATEAU;
                let t = smootherstep((distance - plateau) / (radius - plateau));
                *height = target.lerp(*height, t);
            }
            Self::Raise { amount, .. } => {
                *height += amount * (1. - smootherstep(distance / radius));
            }
        }
    }
}

/// All edits, applied in order.
#[derive(Clone)]
pub struct EditLayer {
    edits: Vec<TerrainEdit>,
    lookup: SpatialGrid,
}

impl Default for EditLayer {
    fn default() -> Self {
        Self {
            edits: Vec::new(),
            lookup: SpatialGrid::new(LOOKUP_CELL),
        }
    }
}

impl EditLayer {
    pub fn push(&mut self, edit: TerrainEdit) {
        let (min, max) = edit.bounds();
        self.lookup.insert(self.edits.len() as u32, min, max);
        self.edits.push(edit);
    }

    pub fn apply(&self, p: Vec2, height: &mut f32) {
        // Ids are stored in insertion order, so edits apply in the order they were made.
        for &id in self.lookup.query_point(p) {
            self.edits[id as usize].apply(p, height);
        }
    }

    /// Mesh cell size wanted by the edits overlapping `min..max`, if any.
    pub fn detail_cell(&self, min: Vec2, max: Vec2) -> Option<f32> {
        self.lookup
            .query_rect(min, max)
            .map(|id| &self.edits[id as usize])
            .filter(|edit| {
                let (e_min, e_max) = edit.bounds();
                super::spatial::rects_overlap(min, max, e_min, e_max)
            })
            .map(TerrainEdit::detail_cell)
            .reduce(f32::min)
    }
}

/// Smooth 0 to 1 transition over `t` in 0..1, with zero first and second derivatives at both
/// ends.
fn smootherstep(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (t * (6. * t - 15.) + 10.)
}
