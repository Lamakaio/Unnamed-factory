//! Analytic river layer: rivers are carved into the terrain at sampling time from the river
//! curves computed by map generation, so their banks stay crisp at any mesh resolution.

use std::f32::consts::SQRT_2;

use bevy::{
    curve::{Curve, cubic_splines::CubicGenerator},
    prelude::*,
};

use super::{
    MIN_CELL,
    spatial::{SpatialGrid, rects_overlap},
};
use crate::mapgen::RiverPath;

/// Polyline samples per curve segment (a curve segment spans ~15 grid tiles).
const SAMPLES_PER_SEGMENT: usize = 45;
/// Width of the wet margin around the river bed, in world units.
const BANK: f32 = 0.25;
/// Depth of the river bed below the water line, in world units.
const BED_DEPTH: f32 = 0.2;
/// Cell size of the segment lookup grid, in world units.
const LOOKUP_CELL: f32 = 4.;

/// A straight piece of river between two polyline samples.
struct RiverSegment {
    /// Water line positions at both ends.
    a: Vec3,
    b: Vec3,
    /// River radius at both ends.
    radius_a: f32,
    radius_b: f32,
}

impl RiverSegment {
    /// Distance from the river axis at which the carve fades out.
    fn reach(radius: f32) -> f32 {
        radius * SQRT_2 + BANK
    }

    fn bounds(&self) -> (Vec2, Vec2) {
        let reach = Self::reach(self.radius_a.max(self.radius_b));
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
    pub fn new(paths: &[RiverPath]) -> Self {
        let mut segments = Vec::new();
        for path in paths {
            let (Ok(position), Ok(amount)) = (path.position.to_curve(), path.amount.to_curve())
            else {
                continue;
            };
            let samples = position.segments().len() * SAMPLES_PER_SEGMENT;
            let mut previous: Option<(Vec3, f32)> = None;
            for i in 0..=samples {
                let t = i as f32 / SAMPLES_PER_SEGMENT as f32;
                // The amount spline has twice fewer segments (as in the original carving code).
                let (Some(pos), Some(amount)) = (position.sample(t), amount.sample(t / 2.)) else {
                    continue;
                };
                let radius = (amount + 50.).log10() - 50f32.log10();
                if let Some((a, radius_a)) = previous {
                    segments.push(RiverSegment {
                        a,
                        b: pos,
                        radius_a,
                        radius_b: radius,
                    });
                }
                previous = Some((pos, radius));
            }
        }

        let mut lookup = SpatialGrid::new(LOOKUP_CELL);
        for (id, segment) in segments.iter().enumerate() {
            let (min, max) = segment.bounds();
            lookup.insert(id as u32, min, max);
        }
        Self { segments, lookup }
    }

    /// Carve the rivers at `p` into `height`, returning how wet the point is (0 to 1).
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
            let radius = segment.radius_a.lerp(segment.radius_b, u);
            let reach = RiverSegment::reach(radius);
            if distance >= reach {
                continue;
            }
            let water_line = segment.a.y.lerp(segment.b.y, u);
            let bed = water_line - BED_DEPTH;
            let t = distance / reach;
            *height = height.min(bed.lerp(base, t * t));
            wetness = wetness.max(((radius + BANK - distance) / BANK).clamp(0., 1.));
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
            .map(|segment| segment.radius_a.min(segment.radius_b) / 4.)
            .reduce(f32::min)
            .map(|cell| cell.max(MIN_CELL))
    }
}
