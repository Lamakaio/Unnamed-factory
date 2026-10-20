//! River channels carved into the surface along the rivers' points: a channel and its banks
//! across each segment, found through buckets of space.

use std::collections::HashMap;

use bevy_math::{IVec3, Vec3};

use crate::rivers::PlanetRiver;

/// Size of the lookup buckets of river segments (world units).
const BUCKET: f32 = 4.;

/// Bank width relative to the channel width (at least [`MIN_BANK`]), widened to keep the
/// bank's mean slope under [`MAX_BANK_SLOPE`], up to [`MAX_BANK`].
const BANK_PER_WIDTH: f32 = 1.;
const MIN_BANK: f32 = 0.8;
const MAX_BANK_SLOPE: f32 = 0.5;
const MAX_BANK: f32 = 6.;

/// A straight piece of river between two points (world positions at sea level radius).
#[derive(Clone)]
struct Segment {
    a: Vec3,
    b: Vec3,
    level: (f32, f32),
    width: (f32, f32),
    depth: (f32, f32),
}

/// The river channels, carved into the surface: segments bucketed by position.
#[derive(Clone, Default)]
pub(super) struct Channels {
    segments: Vec<Segment>,
    buckets: HashMap<IVec3, Vec<u32>>,
}

impl Channels {
    pub(super) fn new(rivers: &[PlanetRiver], radius: f32) -> Self {
        let mut channels = Self::default();
        for river in rivers {
            for pair in river.points.windows(2) {
                let (p, q) = (pair[0], pair[1]);
                let segment = Segment {
                    a: p.direction * radius,
                    b: q.direction * radius,
                    level: (p.level, q.level),
                    // (Channels narrower than the terrain mesh shows come out as holes.)
                    width: (p.width.max(MIN_CARVED_WIDTH), q.width.max(MIN_CARVED_WIDTH)),
                    depth: (p.depth, q.depth),
                };
                let reach = reach(p.width.max(q.width));
                let (lo, hi) = (
                    ((segment.a.min(segment.b) - reach) / BUCKET)
                        .floor()
                        .as_ivec3(),
                    ((segment.a.max(segment.b) + reach) / BUCKET)
                        .floor()
                        .as_ivec3(),
                );
                let id = channels.segments.len() as u32;
                for x in lo.x..=hi.x {
                    for y in lo.y..=hi.y {
                        for z in lo.z..=hi.z {
                            channels
                                .buckets
                                .entry(IVec3::new(x, y, z))
                                .or_default()
                                .push(id);
                        }
                    }
                }
                channels.segments.push(segment);
            }
        }
        channels
    }

    /// Carve the channels at `p` (a point at sea level radius) into `height`: a parabolic
    /// channel under the water line, then banks rising back to the terrain.
    pub(super) fn carve(&self, p: Vec3, height: f32) -> f32 {
        let Some(ids) = self.buckets.get(&(p / BUCKET).floor().as_ivec3()) else {
            return height;
        };
        let base = height;
        let mut height = height;
        for &id in ids {
            let s = &self.segments[id as usize];
            let ab = s.b - s.a;
            let u = ((p - s.a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0., 1.);
            let distance = p.distance(s.a + ab * u);
            let width = s.width.0 + (s.width.1 - s.width.0) * u;
            if distance >= reach(width) {
                continue;
            }
            let water = s.level.0 + (s.level.1 - s.level.0) * u;
            let half = width / 2.;
            let carved = if distance < half {
                let depth = s.depth.0 + (s.depth.1 - s.depth.0) * u;
                water - depth * (1. - (distance / half).powi(2))
            } else {
                let rise = (base - water).max(0.);
                let bank = (width * BANK_PER_WIDTH)
                    .max(MIN_BANK)
                    .max(rise / MAX_BANK_SLOPE)
                    .min(MAX_BANK.max(width * BANK_PER_WIDTH));
                let t = ((distance - half) / bank).clamp(0., 1.);
                let ease_out = 1. - (1. - t) * (1. - t);
                let smooth = t * t * (3. - 2. * t);
                water + rise * 0.5 * (ease_out + smooth)
            };
            height = height.min(carved);
        }
        height
    }
}

impl Channels {
    /// 1 in a channel, fading out just past its edge.
    pub(super) fn wetness(&self, p: Vec3) -> f32 {
        let Some(ids) = self.buckets.get(&(p / BUCKET).floor().as_ivec3()) else {
            return 0.;
        };
        let mut wet: f32 = 0.;
        for &id in ids {
            let s = &self.segments[id as usize];
            let ab = s.b - s.a;
            let u = ((p - s.a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0., 1.);
            let half = (s.width.0 + (s.width.1 - s.width.0) * u) / 2.;
            let distance = p.distance(s.a + ab * u);
            wet = wet.max(((half + 0.3 - distance) / 0.3).clamp(0., 1.));
        }
        wet
    }
}

/// Channels are carved at least this wide (world units).
const MIN_CARVED_WIDTH: f32 = 0.6;

/// Distance from a river's axis where its banks end.
fn reach(width: f32) -> f32 {
    width / 2. + MAX_BANK.max(width * BANK_PER_WIDTH)
}
