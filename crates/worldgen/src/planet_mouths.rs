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

/// River width (world units) from which mouths can build deltas (the flat map's
/// `WorldgenSettings::delta_min_width`, for the planet's narrower rivers).
const DELTA_MIN_WIDTH: f32 = 1.5;
/// Delta land sinks under the sea toward the channels' tips, over this distance (a constant
/// plus this many channel widths), to a bar this deep: no islands at the tips.
const TIP_TRIM: (f32, f32) = (4., 4.);
const TIP_BAR: f32 = -0.12;
/// A fork's branches start a step past the end of the channel they fork from (`grow_channels`:
/// `SPACING`): how far apart a branch's start and its parent's end may be.
const FORK_GAP: f32 = SPACING * 1.2;
/// Largest angle (radians) between a distributary and the delta's direction: the planet's
/// deltas fan less than the flat map's (sideways channels cut across to the next bay).
const MAX_TURN: f32 = 0.8;
/// Distributaries run over the sea (the delta's new land) for the most part: those with more
/// than this share of their points over the old land are dropped.
const MAX_OVERLAND: f32 = 0.5;
/// Distributaries never cross land higher than this (world units): they would cut a canyon
/// through it (the channel and its forks are dropped).
const DELTA_MAX_GROUND: f32 = 1.2;
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
/// (updating `sea`, wet: `wetland`) and add their distributaries to `rivers`. `shore` is the signed distance to
/// the shore (cells, positive on land).
pub fn shape_mouths(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    noise: Noise,
    shore: &[f32],
    heights: &mut [f32],
    sea: &mut [bool],
    wetland: &mut [f32],
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
                    wetland,
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
        // (Estuaries are more common than deltas on Earth.)
        (true, true) if roll < 0.25 => Mouth::Delta,
        (true, true) if roll < 0.85 => Mouth::Estuary,
        (true, false) if roll < 0.5 => Mouth::Estuary,
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
    wetland: &mut [f32],
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
        .find(|&k| !sea[grid.cell_at(points[k].direction)])
        .unwrap_or(count - 1)
        .max(last_join)
        .clamp(1, count - 1);
    let original = river.points.clone();
    river.points.truncate(apex_index + 1);
    let apex = river.points[apex_index];
    let apex_local = frame.local(apex.direction);

    let mut rng = StdRng::seed_from_u64(seed ^ (index as u64).wrapping_mul(0x9E37_79B9));
    let length = (14. + 7. * apex.width) * rng.random_range(0.8..1.2f32);
    // (The sea itself: land below sea level is no way out.)
    let is_sea = |p: Vec2| sea[grid.cell_at(frame.direction(p))];
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
        MAX_TURN,
    );
    // Channels over high ground (headlands, hills) are dropped, with their forks.
    let high = |channel: &Channel| {
        channel.points.iter().any(|p| heights[grid.cell_at(frame.direction(p.position))] > DELTA_MAX_GROUND)
    };
    // ...and so are channels running mostly over the land (not the delta's new land: across a
    // headland, from one sea to another).
    let overland = |channel: &Channel| {
        let on_land = channel
            .points
            .iter()
            .filter(|p| p.travelled > 3. * p.width && !sea[grid.cell_at(frame.direction(p.position))])
            .count();
        on_land as f32 > MAX_OVERLAND * channel.points.len() as f32
    };
    let mut dropped: Vec<bool> = channels.iter().map(|c| high(c) || overland(c)).collect();
    loop {
        let mut changed = false;
        for (k, channel) in channels.iter().enumerate() {
            let start = channel.points[0].position;
            let from_dropped = channels.iter().enumerate().any(|(j, parent)| {
                dropped[j] && j != k && parent.points.last().is_some_and(|end| end.position.distance(start) < FORK_GAP)
            });
            if from_dropped && !dropped[k] {
                dropped[k] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Forks whose branches were all dropped would end on land: dropped too.
    loop {
        let mut changed = false;
        for (k, channel) in channels.iter().enumerate() {
            let end = channel.points.last().map(|p| p.position);
            let continued = channels.iter().enumerate().any(|(j, child)| {
                !dropped[j] && j != k && end.is_some_and(|end| child.points[0].position.distance(end) < FORK_GAP)
            });
            if !dropped[k] && !channel.open && !continued {
                dropped[k] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let channels: Vec<Channel> =
        channels.into_iter().zip(dropped).filter(|(_, dropped)| !dropped).map(|(c, _)| c).collect();
    if !channels.iter().any(|c| c.open) {
        // No way to the sea left: an ordinary mouth.
        river.points = original;
        return Vec::new();
    }
    raise_delta_land(grid, frame, noise.layer(60 + index as u32), heights, sea, wetland, &channels, length);

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
            branch_of: None,
            distributary: true,
        })
        .collect()
}

/// The delta land around the channels (`mouths::raise_delta_land`): wide near the apex,
/// narrowing to levees along the outer channels, open at each tip. The new land is wetland,
/// and so is the old along the channels.
fn raise_delta_land(
    grid: &SphereGrid,
    frame: &Frame,
    noise: Noise,
    heights: &mut [f32],
    sea: &mut [bool],
    wetland: &mut [f32],
    channels: &[Channel],
    length: f32,
) {
    let points: Vec<_> = channels.iter().flat_map(|c| &c.points).collect();
    let Some(first) = points.first() else {
        return;
    };
    // The sea before, and the cells the delta reshapes.
    let mut region = Vec::new();
    let mut was_sea = std::collections::HashMap::new();
    for_cells_around(grid, frame, first.position, length * 1.6 + FRONT_REACH, |i, p| {
        region.push((i, p));
        was_sea.insert(i, sea[i]);
    });
    // The new land (core of the levees, away from the tips), kept above the sea by smoothing.
    let mut new_land = Vec::new();
    for &(i, p) in &region {
        let mut best: f32 = 0.;
        // How far beyond the land the cell is (the delta front).
        let mut beyond = f32::MAX;
        // How far from the tips the land of the best point is (1: away from them).
        let mut emerged: f32 = 0.;
        // Distance to the nearest channel, in levee widths.
        let mut near = f32::MAX;
        for point in &points {
            near = near.min(p.distance(point.position) / (2. * point.width + 1.));
            let t = (point.travelled / length).min(1.);
            let reach = (0.45 * length * (1. - t).powf(1.5) + 1.5 * point.width + 1.)
                * smoothstep(0., 3. * point.width + 2., point.to_tip);
            if reach <= 0. {
                continue;
            }
            beyond = beyond.min(p.distance(point.position) - reach);
            let d = p.distance(point.position) / reach;
            if d < 1. {
                let rise = (1. - d).sqrt() * (0.15 + 0.3 * (1. - t));
                if rise > best {
                    best = rise;
                    emerged = smoothstep(0., TIP_TRIM.0 + TIP_TRIM.1 * point.width, point.to_tip);
                }
            }
        }
        if best > 0. {
            let wobble = 1. + 0.3 * noise.fbm(p / 5., 2);
            let wet = if sea[i] { 1. } else { (1. - near).max(0.) };
            wetland[i] = wetland[i].max(wet);
            let land = 0.08 + best * wobble;
            heights[i] = heights[i].max(TIP_BAR + (land - TIP_BAR) * emerged);
            sea[i] = heights[i] <= 0.;
            if emerged > 0.5 && heights[i] > 0. {
                new_land.push(i);
            }
        } else if sea[i] && beyond < FRONT_REACH {
            // The delta front: sediment raising the sea floor round the delta, sloping down.
            heights[i] = heights[i].max(FRONT_DEPTH.0 - FRONT_DEPTH.1 * beyond.max(0.));
        }
    }
    // Smoothed, so the new land and the front ease into each other and into the old ground
    // (cell by cell, they meet in steps).
    for _ in 0..FRONT_SMOOTHING {
        let smoothed: Vec<f32> = region
            .iter()
            .map(|&(i, _)| {
                let (sum, count) = grid
                    .neighbours(i)
                    .filter(|&(_, d)| d < 1.2)
                    .fold((0., 0.), |(sum, count), (j, _)| (sum + heights[j], count + 1.));
                0.5 * heights[i] + 0.5 * sum / count
            })
            .collect();
        for (&(i, _), h) in region.iter().zip(smoothed) {
            // (Hills the delta abuts stay as they were.)
            if heights[i] < DELTA_MAX_GROUND {
                heights[i] = h;
                sea[i] = h <= 0. && was_sea[&i];
            }
        }
    }
    for &i in &new_land {
        heights[i] = heights[i].max(NEW_LAND_MIN);
        sea[i] = false;
    }
    // A corridor of land along every channel but at its tip: a channel crossing open water
    // would be cut there (rivers end where they reach the sea), its downstream part left alone.
    for channel in channels {
        for point in &channel.points {
            if point.to_tip < TIP_TRIM.0 + TIP_TRIM.1 * point.width {
                continue;
            }
            for_cells_around(grid, frame, point.position, point.width / 2. + CORRIDOR, |i, _| {
                heights[i] = heights[i].max(NEW_LAND_MIN);
                sea[i] = false;
            });
        }
    }
    // Open every tip to the sea, by a path to the open sea if the land closes it in.
    for channel in channels.iter().filter(|channel| channel.open) {
        let Some(tip) = channel.points.last() else {
            continue;
        };
        let mut opened = Vec::new();
        for_cells_around(grid, frame, tip.position, tip.width + 1.5, |i, _| {
            heights[i] = heights[i].min(-0.3);
            sea[i] = true;
            opened.push(i);
        });
        let open_sea = |i: usize| sea[i] && heights[i] < -0.05 && !opened.contains(&i) && was_sea.get(&i).is_none_or(|&s| s);
        let start = grid.cell_at(frame.direction(tip.position));
        let mut previous = std::collections::HashMap::from([(start, start)]);
        let mut queue = std::collections::VecDeque::from([(start, 0)]);
        let mut reached = None;
        while let Some((i, steps)) = queue.pop_front() {
            if open_sea(i) && grid.neighbours(i).all(|(j, _)| sea[j] || heights[j] < 0.) {
                reached = Some(i);
                break;
            }
            if steps >= TIP_PATH {
                continue;
            }
            for (j, _) in grid.neighbours(i) {
                if !previous.contains_key(&j) {
                    previous.insert(j, i);
                    queue.push_back((j, steps + 1));
                }
            }
        }
        let mut i = reached.unwrap_or(start);
        while i != start {
            heights[i] = heights[i].min(-0.3);
            sea[i] = true;
            i = previous[&i];
        }
    }
}

/// The delta front: the sea floor this far beyond the delta land (world units) is raised to
/// this depth, sloping down this much per unit; passes of smoothing over the delta; and how
/// far a tip's path to the open sea may go (cells).
const FRONT_REACH: f32 = 8.;
const FRONT_DEPTH: (f32, f32) = (-0.2, 0.15);
const FRONT_SMOOTHING: usize = 3;
/// Lowest the delta's new land goes once smoothed (thin levees would sink: their channels
/// would be cut into pieces where they reach the sea).
const NEW_LAND_MIN: f32 = 0.06;
/// Land on both sides of a channel, beyond its half width (world units; the cells' heights are
/// interpolated: a strip of a cell or two would dip under the water between them).
const CORRIDOR: f32 = 2.;
const TIP_PATH: usize = 25;
