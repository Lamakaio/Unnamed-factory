//! River springs: where a river starts (not out of a lake, the sea or another river), it
//! comes out of a little pond on flat ground, or out of a notch in the hillside on a slope.
//! Both are carved into the terrain at sampling time (like river channels); `water` draws the
//! pond surfaces and the spouts.

use bevy::prelude::*;
use worldgen::{Continent, RiverEnd};

use super::{
    MIN_CELL,
    spatial::{SpatialGrid, rects_overlap},
};

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

pub enum SpringKind {
    /// A pond whose outline (world xz, closed) the river leaves from.
    Pond { center: Vec2, outline: Vec<Vec2> },
    /// Water spouting out of a notch in the slope.
    Hillside,
}

pub struct Spring {
    /// Index of the river in `Continent::rivers`.
    pub river: usize,
    /// The river's first point (water surface).
    pub position: Vec3,
    /// Flow direction at the source.
    pub direction: Vec2,
    pub width: f32,
    pub kind: SpringKind,
}

impl Spring {
    fn reach(&self) -> f32 {
        match &self.kind {
            SpringKind::Pond { center, outline } => {
                let radius = outline
                    .iter()
                    .map(|p| p.distance(*center))
                    .fold(0., f32::max);
                radius + BANK
            }
            SpringKind::Hillside => self.notch_radius() + NOTCH_BANK,
        }
    }

    fn center(&self) -> Vec2 {
        match &self.kind {
            SpringKind::Pond { center, .. } => *center,
            SpringKind::Hillside => self.position.xz(),
        }
    }

    pub fn notch_radius(&self) -> f32 {
        NOTCH_RADIUS.0 + NOTCH_RADIUS.1 * self.width
    }

    /// Pond radius towards `p` (the outline, interpolated by angle).
    fn pond_radius(center: Vec2, outline: &[Vec2], p: Vec2) -> f32 {
        let angle = (p - center).to_angle().rem_euclid(std::f32::consts::TAU);
        let step = std::f32::consts::TAU / outline.len() as f32;
        let k = (angle / step).floor() as usize % outline.len();
        let t = angle / step - (angle / step).floor();
        let r0 = outline[k].distance(center);
        let r1 = outline[(k + 1) % outline.len()].distance(center);
        r0 + (r1 - r0) * t
    }
}

/// The springs of a continent's rivers.
pub fn find_springs(continent: &Continent) -> Vec<Spring> {
    let rivers = &continent.rivers;
    let size = continent.size() as i64;
    let near_lake = |p: Vec2| {
        continent.world_to_cell(p).is_some_and(|(x, z)| {
            (-2..=2).any(|dz: i64| {
                (-2..=2).any(|dx: i64| {
                    let (x, z) = (x as i64 + dx, z as i64 + dz);
                    x >= 0
                        && z >= 0
                        && x < size
                        && z < size
                        && continent.lake_of[(z * size + x) as usize].is_some()
                })
            })
        })
    };
    // Sources on another river (delta distributaries, lake outlets joining) are not springs.
    let on_river = |river: usize, p: Vec2| {
        rivers.iter().enumerate().any(|(other, r)| {
            other != river
                && r.points
                    .iter()
                    .any(|q| q.position.xz().distance(p) < q.width / 2. + 1.)
        })
    };
    let mut springs = Vec::new();
    for (index, river) in rivers.iter().enumerate() {
        if river.points.len() < 6 || river.end == RiverEnd::Sea && river.points[0].position.y < 0.5
        {
            continue;
        }
        let source = river.points[0];
        let p = source.position.xz();
        if source.position.y <= 0.3 || near_lake(p) || on_river(index, p) {
            continue;
        }
        let direction = (river.points[3].position - source.position)
            .xz()
            .normalize_or_zero();
        // Terrain slope around the source.
        let at = |q: Vec2| continent.surface.bilinear(q);
        let d = 1.5;
        let slope = Vec2::new(
            at(p + Vec2::X * d) - at(p - Vec2::X * d),
            at(p + Vec2::Y * d) - at(p - Vec2::Y * d),
        )
        .length()
            / (2. * d);
        let kind = if slope < POND_MAX_SLOPE {
            let radius =
                (POND_RADIUS.0 + POND_RADIUS.1 * source.width) * (0.8 + 0.4 * jitter(p, 1));
            // The pond lies upstream of the source, which is on its rim.
            let center = p - direction * radius * 0.85;
            let outline = (0..POND_SIDES)
                .map(|k| {
                    let angle = k as f32 / POND_SIDES as f32 * std::f32::consts::TAU;
                    // A lumpy outline: a few smooth bumps.
                    let bumps = 0.12 * (angle * 2. + jitter(p, 2) * 6.).sin()
                        + 0.08 * (angle * 3. + jitter(p, 3) * 6.).sin();
                    center + Vec2::from_angle(angle) * radius * (1. + bumps)
                })
                .collect();
            SpringKind::Pond { center, outline }
        } else {
            SpringKind::Hillside
        };
        springs.push(Spring {
            river: index,
            position: source.position,
            direction,
            width: source.width,
            kind,
        });
    }
    let ponds = springs
        .iter()
        .filter(|s| matches!(s.kind, SpringKind::Pond { .. }))
        .count();
    info!("{} springs: {ponds} ponds, {} hillside", springs.len(), springs.len() - ponds);
    for spring in &springs {
        debug!(
            "spring at {:.1} {:.1}: {}",
            spring.position.x,
            spring.position.z,
            if matches!(spring.kind, SpringKind::Pond { .. }) { "pond" } else { "hillside" }
        );
    }
    springs
}

/// A deterministic pseudo-random value in 0..1 for a position (and a salt).
fn jitter(p: Vec2, salt: u32) -> f32 {
    let mut h = p.x.to_bits() ^ p.y.to_bits().rotate_left(16) ^ salt.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 15)).wrapping_mul(0x85eb_ca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

/// Carves the springs into the terrain.
pub struct SpringLayer {
    springs: Vec<Spring>,
    lookup: SpatialGrid,
}

impl SpringLayer {
    pub fn new(springs: Vec<Spring>) -> Self {
        let mut lookup = SpatialGrid::new(4.);
        for (id, spring) in springs.iter().enumerate() {
            let reach = spring.reach();
            lookup.insert(id as u32, spring.center() - reach, spring.center() + reach);
        }
        Self { springs, lookup }
    }

    pub fn springs(&self) -> &[Spring] {
        &self.springs
    }

    /// Carve the springs at `p` into `height`, returning how wet the point is (0 to 1).
    pub fn carve(&self, p: Vec2, height: &mut f32) -> f32 {
        let base = *height;
        let mut wetness: f32 = 0.;
        for &id in self.lookup.query_point(p) {
            let spring = &self.springs[id as usize];
            let level = spring.position.y;
            match &spring.kind {
                SpringKind::Pond { center, outline } => {
                    // A bowl under the water, then a bank easing back into the terrain.
                    let radius = Spring::pond_radius(*center, outline, p);
                    let distance = p.distance(*center);
                    if distance >= radius + BANK {
                        continue;
                    }
                    let carved = if distance < radius {
                        level - POND_DEPTH * (1. - (distance / radius).powi(2))
                    } else {
                        let rise = (base - level).max(0.);
                        let t = (distance - radius) / BANK;
                        level + rise * (1. - (1. - t) * (1. - t))
                    };
                    *height = height.min(carved);
                    wetness = wetness.max(((radius + 0.3 - distance) / 0.3).clamp(0., 1.));
                }
                SpringKind::Hillside => {
                    // A notch in the slope, its floor at the spout, upstream of the source.
                    let center = spring.position.xz() - spring.direction * spring.notch_radius() * 0.5;
                    let radius = spring.notch_radius();
                    let distance = p.distance(center);
                    if distance >= radius + NOTCH_BANK {
                        continue;
                    }
                    let floor = level + SPOUT_HEIGHT;
                    let carved = if distance < radius {
                        floor + 0.15 * (distance / radius).powi(2)
                    } else {
                        let rise = (base - floor - 0.15).max(0.);
                        let t = (distance - radius) / NOTCH_BANK;
                        floor + 0.15 + rise * (1. - (1. - t) * (1. - t))
                    };
                    *height = height.min(carved);
                }
            }
        }
        wetness
    }

    /// How close `p` is to a pond's water or a spring's notch: 1 at the edge (and inside), 0
    /// from `margin` beyond it.
    pub fn near_water(&self, p: Vec2, margin: f32) -> f32 {
        let mut near: f32 = 0.;
        for &id in self.lookup.query_point(p) {
            let spring = &self.springs[id as usize];
            let beyond = match &spring.kind {
                SpringKind::Pond { center, outline } => {
                    p.distance(*center) - Spring::pond_radius(*center, outline, p)
                }
                SpringKind::Hillside => p.distance(spring.position.xz()) - spring.notch_radius(),
            };
            near = near.max(1. - (beyond / margin).clamp(0., 1.));
        }
        near
    }

    /// Mesh cell size wanted by the springs overlapping `min..max`, if any.
    pub fn detail_cell(&self, min: Vec2, max: Vec2) -> Option<f32> {
        self.lookup
            .query_rect(min, max)
            .map(|id| &self.springs[id as usize])
            .filter(|spring| {
                let reach = spring.reach();
                rects_overlap(min, max, spring.center() - reach, spring.center() + reach)
            })
            .map(|spring| (spring.width / 4.).max(MIN_CELL))
            .reduce(f32::min)
    }
}
