//! River springs on the planet: where a river
//! starts (not out of a lake or on another river), it comes out of a little pond on flat
//! ground, or out of a notch in the hillside on a slope. Both are carved into the surface (like
//! the river channels, `PlanetSurface::height`); the game draws the pond surfaces and the
//! spouts. Shapes live in the plane tangent at the source: x downstream, y across.

use std::collections::HashMap;

use bevy_math::{IVec3, Vec2, Vec3};

use crate::rivers::PlanetRiver;

/// Ground flatter than this (slope around the source) gets a pond, steeper a hillside spring.
const POND_MAX_SLOPE: f32 = 0.18;
/// Pond radius: a constant plus this many channel widths (varied per pond by ±20%).
const POND_RADIUS: (f32, f32) = (0.8, 1.2);
const POND_DEPTH: f32 = 0.45;
/// Banks around ponds and spring notches, as for rivers.
const BANK: f32 = 1.2;
/// Height of a hillside spring's spout above the stream, and the notch it comes out of
/// (radius: a constant plus this many channel widths).
pub const SPOUT_HEIGHT: f32 = 0.45;
const NOTCH_RADIUS: (f32, f32) = (0.45, 0.9);
/// Width of the notch's steep walls.
const NOTCH_BANK: f32 = 0.5;
/// Number of points of a pond outline.
pub const POND_SIDES: usize = 28;
/// Bucket size (world units) of the springs' lookup.
const BUCKET: f32 = 4.;

#[derive(Clone, Debug)]
pub enum PlanetSpringKind {
    /// A pond: its centre (local coordinates) and the radius of its outline at `POND_SIDES`
    /// angles round it.
    Pond { center: Vec2, radii: Vec<f32> },
    /// Water spouting out of a notch in the slope.
    Hillside,
}

#[derive(Clone, Debug)]
pub struct PlanetSpring {
    /// Index of the river in `PlanetSurface::rivers`.
    pub river: usize,
    /// The river's first point (unit direction) and its water level.
    pub direction: Vec3,
    pub level: f32,
    /// Flow direction at the source (unit tangent), and across it.
    pub downstream: Vec3,
    pub across: Vec3,
    pub width: f32,
    pub kind: PlanetSpringKind,
}

impl PlanetSpring {
    pub fn notch_radius(&self) -> f32 {
        NOTCH_RADIUS.0 + NOTCH_RADIUS.1 * self.width
    }

    /// Local coordinates (x downstream, y across, world units) of a world position, from the
    /// source on the sphere of radius `radius`.
    fn local(&self, p: Vec3, radius: f32) -> Vec2 {
        let offset = p - self.direction * radius;
        Vec2::new(offset.dot(self.downstream), offset.dot(self.across))
    }

    /// The world direction of local coordinates.
    pub fn direction_of(&self, local: Vec2, radius: f32) -> Vec3 {
        (self.direction * radius + self.downstream * local.x + self.across * local.y).normalize()
    }

    /// The pond's outline (local coordinates, `POND_SIDES` points round it), if a pond.
    pub fn outline(&self) -> Option<Vec<Vec2>> {
        let PlanetSpringKind::Pond { center, radii } = &self.kind else {
            return None;
        };
        Some(
            radii
                .iter()
                .enumerate()
                .map(|(k, r)| *center + Vec2::from_angle(k as f32 * std::f32::consts::TAU / POND_SIDES as f32) * *r)
                .collect(),
        )
    }

    /// Centre of what is carved (local coordinates) and its reach.
    fn extent(&self) -> (Vec2, f32) {
        match &self.kind {
            PlanetSpringKind::Pond { center, radii } => (*center, radii.iter().fold(0f32, |a, &b| a.max(b)) + BANK),
            PlanetSpringKind::Hillside => (-Vec2::X * self.notch_radius() * 0.5, self.notch_radius() + NOTCH_BANK),
        }
    }

    /// The pond's radius toward local point `q` (its outline, interpolated by angle).
    fn pond_radius(center: Vec2, radii: &[f32], q: Vec2) -> f32 {
        let angle = (q - center).to_angle().rem_euclid(std::f32::consts::TAU);
        let step = std::f32::consts::TAU / radii.len() as f32;
        let k = (angle / step).floor() as usize % radii.len();
        let t = angle / step - (angle / step).floor();
        radii[k] + (radii[(k + 1) % radii.len()] - radii[k]) * t
    }
}

/// The springs of the planet's rivers. `ground(direction)` is the surface height (no channels),
/// `lake_depth(direction)` the lake water there, `ocean(direction)` the ocean (see
/// `PlanetSurface::ocean`): sources by the sea are no springs.
pub fn find_springs(
    rivers: &[PlanetRiver],
    radius: f32,
    ground: impl Fn(Vec3) -> f32,
    lake_depth: impl Fn(Vec3) -> f32,
    ocean: impl Fn(Vec3) -> f32,
) -> Vec<PlanetSpring> {
    // River points by bucket, to tell sources on another river (lake outlets joining, delta
    // distributaries) from springs.
    let mut points: HashMap<IVec3, Vec<(usize, Vec3, f32)>> = HashMap::new();
    for (index, river) in rivers.iter().enumerate() {
        for point in &river.points {
            let p = point.direction * radius;
            points.entry((p / BUCKET).floor().as_ivec3()).or_default().push((index, p, point.width));
        }
    }
    let on_river = |river: usize, p: Vec3| {
        let bucket = (p / BUCKET).floor().as_ivec3();
        (-1..=1).any(|x| {
            (-1..=1).any(|y| {
                (-1..=1).any(|z| {
                    points.get(&(bucket + IVec3::new(x, y, z))).is_some_and(|near| {
                        near.iter().any(|&(other, q, width)| other != river && q.distance(p) < width / 2. + 1.)
                    })
                })
            })
        })
    };
    let mut springs = Vec::new();
    for (index, river) in rivers.iter().enumerate() {
        let source = river.points[0];
        // (Low sources too, away from the sea: land below sea level has springs.)
        if river.points.len() < 6 || source.level <= 0.05 || ocean(source.direction) > 0.02 {
            continue;
        }
        let up = source.direction;
        let p = up * radius;
        let downstream = (river.points[3].direction - up).reject_from(up).normalize_or_zero();
        if downstream == Vec3::ZERO {
            continue;
        }
        let across = up.cross(downstream);
        let at = |offset: Vec3| ground((p + offset).normalize());
        // Near a lake, or on another river: not a spring.
        let near_lake = [Vec3::ZERO, downstream, -downstream, across, -across]
            .iter()
            .any(|&d| lake_depth((p + d * 2.).normalize()) > 0.05);
        if near_lake || on_river(index, p) {
            continue;
        }
        // Terrain slope around the source.
        let d = 1.5;
        let slope = Vec2::new(at(downstream * d) - at(-downstream * d), at(across * d) - at(-across * d)).length()
            / (2. * d);
        let kind = if slope < POND_MAX_SLOPE {
            let size = (POND_RADIUS.0 + POND_RADIUS.1 * source.width) * (0.8 + 0.4 * jitter(p, 1));
            // The pond lies upstream of the source, which is on its rim.
            let center = -Vec2::X * size * 0.85;
            let radii = (0..POND_SIDES)
                .map(|k| {
                    let angle = k as f32 / POND_SIDES as f32 * std::f32::consts::TAU;
                    // A lumpy outline: a few smooth bumps.
                    let bumps = 0.12 * (angle * 2. + jitter(p, 2) * 6.).sin()
                        + 0.08 * (angle * 3. + jitter(p, 3) * 6.).sin();
                    size * (1. + bumps)
                })
                .collect();
            PlanetSpringKind::Pond { center, radii }
        } else {
            PlanetSpringKind::Hillside
        };
        springs.push(PlanetSpring {
            river: index,
            direction: up,
            level: source.level,
            downstream,
            across,
            width: source.width,
            kind,
        });
    }
    springs
}

/// A deterministic pseudo-random value in 0..1 for a position (and a salt).
fn jitter(p: Vec3, salt: u32) -> f32 {
    let mut h = p.x.to_bits() ^ p.y.to_bits().rotate_left(11) ^ p.z.to_bits().rotate_left(22) ^ salt.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 15)).wrapping_mul(0x85eb_ca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

/// The springs, bucketed for carving the surface.
#[derive(Clone, Default)]
pub(crate) struct Springs {
    pub(crate) springs: Vec<PlanetSpring>,
    radius: f32,
    buckets: HashMap<IVec3, Vec<u32>>,
}

impl Springs {
    pub(crate) fn new(springs: Vec<PlanetSpring>, radius: f32) -> Self {
        let mut buckets: HashMap<IVec3, Vec<u32>> = HashMap::new();
        for (id, spring) in springs.iter().enumerate() {
            let (center, reach) = spring.extent();
            let c = spring.direction_of(center, radius) * radius;
            let (lo, hi) = (((c - reach) / BUCKET).floor().as_ivec3(), ((c + reach) / BUCKET).floor().as_ivec3());
            for x in lo.x..=hi.x {
                for y in lo.y..=hi.y {
                    for z in lo.z..=hi.z {
                        buckets.entry(IVec3::new(x, y, z)).or_default().push(id as u32);
                    }
                }
            }
        }
        Self { springs, radius, buckets }
    }

    /// Carve the springs at world position `p` into `height`, returning the carved height and
    /// how wet the point is (0..1: pond beds).
    pub(crate) fn carve(&self, p: Vec3, height: f32) -> (f32, f32) {
        let Some(ids) = self.buckets.get(&(p / BUCKET).floor().as_ivec3()) else {
            return (height, 0.);
        };
        let (base, mut height, mut wetness) = (height, height, 0f32);
        for &id in ids {
            let spring = &self.springs[id as usize];
            let q = spring.local(p, self.radius);
            let level = spring.level;
            match &spring.kind {
                PlanetSpringKind::Pond { center, radii } => {
                    // A bowl under the water, then a bank easing back into the terrain.
                    let r = PlanetSpring::pond_radius(*center, radii, q);
                    let distance = q.distance(*center);
                    if distance >= r + BANK {
                        continue;
                    }
                    let carved = if distance < r {
                        level - POND_DEPTH * (1. - (distance / r).powi(2))
                    } else {
                        let rise = (base - level).max(0.);
                        let t = (distance - r) / BANK;
                        level + rise * (1. - (1. - t) * (1. - t))
                    };
                    height = height.min(carved);
                    wetness = wetness.max(((r + 0.3 - distance) / 0.3).clamp(0., 1.));
                }
                PlanetSpringKind::Hillside => {
                    // A notch in the slope, its floor at the spout, upstream of the source.
                    let r = spring.notch_radius();
                    let distance = q.distance(-Vec2::X * r * 0.5);
                    if distance >= r + NOTCH_BANK {
                        continue;
                    }
                    let floor = level + SPOUT_HEIGHT;
                    let carved = if distance < r {
                        floor + 0.15 * (distance / r).powi(2)
                    } else {
                        let rise = (base - floor - 0.15).max(0.);
                        let t = (distance - r) / NOTCH_BANK;
                        floor + 0.15 + rise * (1. - (1. - t) * (1. - t))
                    };
                    height = height.min(carved);
                }
            }
        }
        (height, wetness)
    }
}
