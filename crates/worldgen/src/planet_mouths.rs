//! River mouths on the planet's sea, as on the flat map (`mouths.rs`): large rivers reaching a
//! flat coast may build a delta (low new land crossed by distributaries), some open into an
//! estuary, and both spread a low sediment plain around the mouth. Each mouth is worked out in
//! the plane tangent to the planet there (a delta is small next to the radius): the flat
//! map's channel growth runs unchanged in it, and the cells around are walked on the sphere's
//! grid.

use std::collections::{HashSet, VecDeque};

use bevy_math::{Vec2, Vec3};
use rand::{RngExt, SeedableRng, rngs::StdRng};

use crate::{
    RiverEnd, RiverPoint,
    grid::Grid,
    mouths::{Channel, Mouth, grow_channels},
    noise::{Noise, hash, smoothstep},
    planet_rivers::{PlanetRiver, PlanetRiverPoint},
    sphere::SphereGrid,
};

/// River width (world units) from which mouths can build deltas (`WorldgenSettings`).
const DELTA_MIN_WIDTH: f32 = 2.;
/// Spacing of river points (world units, about cells on the planet).
const SPACING: f32 = crate::mouths::SPACING;

/// The plane tangent to the planet at a point: 2D coordinates in world units (gnomonic).
struct Frame {
    up: Vec3,
    east: Vec3,
    north: Vec3,
    radius: f32,
}

impl Frame {
    fn new(up: Vec3, radius: f32) -> Self {
        let east = up.any_orthonormal_vector();
        Self { up, east, north: up.cross(east), radius }
    }

    fn local(&self, direction: Vec3) -> Vec2 {
        let c = direction.dot(self.up).max(1e-3);
        Vec2::new(direction.dot(self.east), direction.dot(self.north)) * self.radius / c
    }

    fn direction(&self, p: Vec2) -> Vec3 {
        (self.up + (self.east * p.x + self.north * p.y) / self.radius).normalize()
    }
}

/// Call `f(cell, local position)` for every cell within `radius` of `center` (local
/// coordinates), walking the grid from the cell there.
fn for_cells_around(
    grid: &SphereGrid,
    frame: &Frame,
    center: Vec2,
    radius: f32,
    mut f: impl FnMut(usize, Vec2),
) {
    let start = grid.cell_at(frame.direction(center));
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(i) = queue.pop_front() {
        let p = frame.local(grid.direction(i));
        let distance = p.distance(center);
        if distance <= radius {
            f(i, p);
        }
        if distance > radius + 1.5 {
            continue;
        }
        for (j, _) in grid.neighbours(i) {
            if seen.insert(j) {
                queue.push_back(j);
            }
        }
    }
}

/// Choose and shape the mouth of every river ending in the sea. Deltas raise land in `heights`
/// (updating `sea`) and add their distributaries to `rivers`. `shore` is the signed distance to
/// the shore (cells, positive on land).
pub fn shape_mouths(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    noise: Noise,
    shore: &[f32],
    heights: &mut [f32],
    sea: &mut [bool],
    rivers: &mut Vec<PlanetRiver>,
) {
    let original = rivers.len();
    for index in 0..original {
        if rivers[index].end != RiverEnd::Sea || rivers[index].points.len() < 8 {
            continue;
        }
        let end = *rivers[index].points.last().unwrap();
        let frame = Frame::new(end.direction, radius);
        let mouth = choose(grid, &frame, seed, heights, end.width, index);
        if mouth != Mouth::Plain {
            spread_sediment(grid, &frame, shore, heights, end.width);
        }
        match mouth {
            Mouth::Plain => {}
            Mouth::Estuary => widen(&mut rivers[index]),
            Mouth::Delta => {
                // The delta starts no earlier than the river's last confluence: its tributaries
                // must still find it.
                let last_join = (0..original)
                    .filter(|&t| rivers[t].end == RiverEnd::Junction(index))
                    .filter_map(|t| rivers[t].points.last())
                    .filter_map(|join| {
                        let river = &rivers[index].points;
                        (0..river.len()).max_by(|&a, &b| {
                            river[a].direction.dot(join.direction).total_cmp(&river[b].direction.dot(join.direction))
                        })
                    })
                    .max()
                    .unwrap_or(0);
                let distributaries = build_delta(
                    grid,
                    &frame,
                    seed,
                    noise,
                    shore,
                    heights,
                    sea,
                    &mut rivers[index],
                    index,
                    last_join,
                );
                rivers.extend(distributaries);
            }
        }
    }
}

fn choose(grid: &SphereGrid, frame: &Frame, seed: u64, heights: &[f32], width: f32, index: usize) -> Mouth {
    let roll = hash(seed as u32, index as u32, 0x0D17A) as f32 / u32::MAX as f32;
    let large = width >= DELTA_MIN_WIDTH;
    let (mut sum, mut count) = (0., 0.);
    for_cells_around(grid, frame, Vec2::ZERO, 15., |i, _| {
        if heights[i] > 0. {
            sum += heights[i];
            count += 1.;
        }
    });
    let flat = count == 0. || sum / count < 3.;
    match (large, flat) {
        (true, true) if roll < 0.55 => Mouth::Delta,
        (true, true) if roll < 0.85 => Mouth::Estuary,
        (true, false) if roll < 0.4 => Mouth::Estuary,
        (false, _) if width >= DELTA_MIN_WIDTH * 0.6 && roll < 0.2 => Mouth::Estuary,
        _ => Mouth::Plain,
    }
}

/// Sediment fan around a mouth (radius 6 + 5·width): the land there is lowered to a low, flat
/// deposit plain. The sea floor is left alone.
fn spread_sediment(grid: &SphereGrid, frame: &Frame, shore: &[f32], heights: &mut [f32], width: f32) {
    let radius = 6. + 5. * width;
    for_cells_around(grid, frame, Vec2::ZERO, radius, |i, p| {
        let value = smoothstep(radius, radius * 0.3, p.length());
        let h = heights[i];
        if h > 0. {
            let deposit = h.min(0.25 + 0.06 * shore[i].max(0.));
            heights[i] = h + (deposit - h) * value;
        }
    });
}

/// Estuary: the channel widens and deepens a little over its last stretch.
fn widen(river: &mut PlanetRiver) {
    let end_width = river.points.last().unwrap().width;
    let length = 12. + 4. * end_width;
    let count = river.points.len();
    for (k, point) in river.points.iter_mut().enumerate() {
        let remaining = (count - 1 - k) as f32 * SPACING;
        if remaining < length {
            let t = 1. - remaining / length;
            point.width *= 1. + 1.5 * t * t;
            point.depth *= 1. + 0.5 * t;
        }
    }
}

/// Delta, as on the flat map (`mouths::build_delta`): the river now ends where it reaches the
/// shore and splits there into a tree of distributaries, built up with low land along them.
#[allow(clippy::too_many_arguments)]
fn build_delta(
    grid: &SphereGrid,
    frame: &Frame,
    seed: u64,
    noise: Noise,
    shore: &[f32],
    heights: &mut [f32],
    sea: &mut [bool],
    river: &mut PlanetRiver,
    index: usize,
    last_join: usize,
) -> Vec<PlanetRiver> {
    let points = &river.points;
    let count = points.len();
    let local = |k: usize| frame.local(points[k].direction);
    // The delta fans out toward the open sea (down the shore distance).
    let flow = (local(count - 1) - local(count - 7)).normalize_or_zero();
    let shore_at = |p: Vec2| shore[grid.cell_at(frame.direction(p))];
    let seaward = -Vec2::new(
        shore_at(Vec2::X * 2.) - shore_at(-Vec2::X * 2.),
        shore_at(Vec2::Y * 2.) - shore_at(-Vec2::Y * 2.),
    )
    .normalize_or_zero();
    let direction = (0.4 * flow + 0.6 * seaward).normalize_or_zero();
    if direction == Vec2::ZERO {
        return Vec::new();
    }
    // The apex: the last point over land.
    let apex_index = (0..count)
        .rev()
        .find(|&k| heights[grid.cell_at(points[k].direction)] > 0.)
        .unwrap_or(count - 1)
        .max(last_join)
        .clamp(1, count - 1);
    river.points.truncate(apex_index + 1);
    let apex = river.points[apex_index];
    let apex_local = frame.local(apex.direction);

    let mut rng = StdRng::seed_from_u64(seed ^ (index as u64).wrapping_mul(0x9E37_79B9));
    let length = (14. + 7. * apex.width) * rng.random_range(0.8..1.2f32);
    let is_sea = |p: Vec2| heights[grid.cell_at(frame.direction(p))] <= 0.;
    let channels = grow_channels(
        &mut rng,
        noise.layer(40 + index as u32),
        &is_sea,
        RiverPoint {
            position: Vec3::new(apex_local.x, apex.level, apex_local.y),
            width: apex.width,
            depth: apex.depth,
        },
        direction,
        length,
    );
    raise_delta_land(grid, frame, noise.layer(60 + index as u32), heights, sea, &channels, length);

    // Water falls from the apex level to just under the sea at the full delta length.
    let level = |travelled: f32| apex.level + (-0.1 - apex.level) * (travelled / length).min(1.);
    channels
        .into_iter()
        .map(|channel| PlanetRiver {
            points: channel
                .points
                .iter()
                .map(|point| PlanetRiverPoint {
                    direction: frame.direction(point.position),
                    level: level(point.travelled),
                    width: point.width,
                    depth: apex.depth * (point.width / apex.width).sqrt(),
                })
                .collect(),
            end: RiverEnd::Sea,
        })
        .collect()
}

/// The delta land around the channels (`mouths::raise_delta_land`): wide near the apex,
/// narrowing to levees along the outer channels, open at each tip.
fn raise_delta_land(
    grid: &SphereGrid,
    frame: &Frame,
    noise: Noise,
    heights: &mut [f32],
    sea: &mut [bool],
    channels: &[Channel],
    length: f32,
) {
    let points: Vec<_> = channels.iter().flat_map(|c| &c.points).collect();
    let Some(first) = points.first() else {
        return;
    };
    for_cells_around(grid, frame, first.position, length * 1.6, |i, p| {
        let mut best: f32 = 0.;
        for point in &points {
            let t = (point.travelled / length).min(1.);
            let reach = (0.45 * length * (1. - t).powf(1.5) + 1.5 * point.width + 1.)
                * smoothstep(0., 3. * point.width + 2., point.to_tip);
            if reach <= 0. {
                continue;
            }
            let d = p.distance(point.position) / reach;
            if d < 1. {
                best = best.max((1. - d).sqrt() * (0.15 + 0.3 * (1. - t)));
            }
        }
        if best > 0. {
            let wobble = 1. + 0.3 * noise.fbm(p / 5., 2);
            heights[i] = heights[i].max(0.08 + best * wobble);
            sea[i] = heights[i] <= 0.;
        }
    });
    // Open every tip to the sea.
    for channel in channels.iter().filter(|channel| channel.open) {
        let Some(tip) = channel.points.last() else {
            continue;
        };
        for_cells_around(grid, frame, tip.position, tip.width + 1.5, |i, _| {
            heights[i] = heights[i].min(-0.3);
            sea[i] = true;
        });
    }
}
