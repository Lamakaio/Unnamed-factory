//! Analytic river channels: carved at sampling time from the generated river polylines, so
//! banks stay crisp at any mesh resolution. The valleys themselves are baked by erosion.

use bevy::prelude::*;
use worldgen::River;

use super::{
    MIN_CELL,
    spatial::{SpatialGrid, rects_overlap},
};

/// Bank width relative to the channel width (with a minimum of [`MIN_BANK`]).
const BANK_PER_WIDTH: f32 = 0.6;
const MIN_BANK: f32 = 0.8;
/// Banks widen so their mean slope stays below this, up to [`MAX_BANK`] wide: steep walls
/// alias into a sawtooth on the mesh grid.
const MAX_BANK_SLOPE: f32 = 0.8;
const MAX_BANK: f32 = 3.;
/// Cell size of the segment lookup grid, in world units.
const LOOKUP_CELL: f32 = 4.;

/// A straight piece of river between two polyline points.
struct RiverSegment {
    /// Water surface positions at both ends.
    a: Vec3,
    b: Vec3,
    width_a: f32,
    width_b: f32,
    depth_a: f32,
    depth_b: f32,
}

impl RiverSegment {
    /// Distance from the river axis where the carving can end (the widest bank).
    fn reach(width: f32) -> f32 {
        width / 2. + MAX_BANK.max(width * BANK_PER_WIDTH)
    }

    fn bounds(&self) -> (Vec2, Vec2) {
        let reach = Self::reach(self.width_a.max(self.width_b));
        (
            self.a.xz().min(self.b.xz()) - reach,
            self.a.xz().max(self.b.xz()) + reach,
        )
    }
}

pub struct RiverLayer {
    segments: Vec<RiverSegment>,
    lookup: SpatialGrid,
}

impl RiverLayer {
    pub fn new(rivers: &[River]) -> Self {
        let segments: Vec<RiverSegment> = rivers
            .iter()
            .flat_map(|river| river.points.windows(2))
            .map(|pair| RiverSegment {
                a: pair[0].position,
                b: pair[1].position,
                width_a: pair[0].width,
                width_b: pair[1].width,
                depth_a: pair[0].depth,
                depth_b: pair[1].depth,
            })
            .collect();
        let mut lookup = SpatialGrid::new(LOOKUP_CELL);
        for (id, segment) in segments.iter().enumerate() {
            let (min, max) = segment.bounds();
            lookup.insert(id as u32, min, max);
        }
        Self { segments, lookup }
    }

    /// Carve the river channels at `p` into `height`, returning how wet the point is (0 to 1).
    ///
    /// Cross-section: a parabolic channel `depth` deep and `width` wide, then banks rising
    /// from the water line back to the terrain, steep at first (so the water ribbon's overlap
    /// goes under them) and wide enough to keep a moderate slope. The profile is continuous.
    pub fn carve(&self, p: Vec2, height: &mut f32) -> f32 {
        let base = *height;
        let mut wetness: f32 = 0.;
        for &id in self.lookup.query_point(p) {
            let segment = &self.segments[id as usize];
            let (a, b) = (segment.a.xz(), segment.b.xz());
            let ab = b - a;
            let u = if ab.length_squared() > 0. {
                ((p - a).dot(ab) / ab.length_squared()).clamp(0., 1.)
            } else {
                0.
            };
            let distance = p.distance(a + ab * u);
            let width = segment.width_a.lerp(segment.width_b, u);
            if distance >= RiverSegment::reach(width) {
                continue;
            }
            let water = segment.a.y.lerp(segment.b.y, u);
            let half = width / 2.;
            let carved = if distance < half {
                let depth = segment.depth_a.lerp(segment.depth_b, u);
                water - depth * (1. - (distance / half).powi(2))
            } else {
                let rise = (base - water).max(0.);
                let bank = (width * BANK_PER_WIDTH)
                    .max(MIN_BANK)
                    .max(rise / MAX_BANK_SLOPE)
                    .min(MAX_BANK.max(width * BANK_PER_WIDTH));
                // Ease-out: steepest at the water line, flattening into the terrain.
                let t = ((distance - half) / bank).clamp(0., 1.);
                water + rise * (1. - (1. - t) * (1. - t))
            };
            *height = height.min(carved);
            wetness = wetness.max(((half + 0.3 - distance) / 0.3).clamp(0., 1.));
        }
        wetness
    }

    /// Mesh cell size wanted by the rivers overlapping `min..max`, if any.
    pub fn detail_cell(&self, min: Vec2, max: Vec2) -> Option<f32> {
        self.lookup
            .query_rect(min, max)
            .map(|id| &self.segments[id as usize])
            .filter(|segment| {
                let (s_min, s_max) = segment.bounds();
                rects_overlap(min, max, s_min, s_max)
            })
            .map(|segment| segment.width_a.min(segment.width_b) / 6.)
            .reduce(f32::min)
            .map(|cell| cell.max(MIN_CELL))
    }
}

