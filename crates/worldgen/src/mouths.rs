//! River mouths on the sea. Most rivers just end at the shore. Large rivers reaching a flat
//! coast may build a **delta** (a lobe of low new land crossed by distributary channels), and
//! some rivers open into a wide **estuary**. Both spread a sediment fan around the mouth: low,
//! flat land with a wide beach.

use bevy_math::{Vec2, Vec3};
use rand::{Rng, RngExt, SeedableRng, rngs::StdRng};

use crate::{
    River, RiverEnd, RiverPoint, WorldgenSettings,
    noise::{Noise, hash, smoothstep},
};

/// How a river meets the sea.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouth {
    Plain,
    Estuary,
    Delta,
}

/// Spacing of distributary polyline points, in world units (as for rivers).
const SPACING: f32 = 0.5;
/// Largest angle (radians) between a distributary and the delta's direction.
const MAX_TURN: f32 = 1.3;

/// Choose and shape the mouth of every river ending in the sea. Deltas raise land in `heights`
/// and add their distributaries to `rivers`. `shore` is the distance to the shore (cells).
///
/// Returns the mouth of each original river ending in the sea (by river index), and the
/// sediment fans (0..1 per cell, for beaches) and the delta wetlands (0..1 per cell).
pub fn shape_mouths(
    settings: &WorldgenSettings,
    noise: Noise,
    shore: &[f32],
    heights: &mut [f32],
    rivers: &mut Vec<River>,
) -> (Vec<(usize, Mouth)>, Vec<f32>, Vec<f32>) {
    let mut mouths = Vec::new();
    let mut sediment = vec![0f32; heights.len()];
    let mut wetland = vec![0f32; heights.len()];
    for index in 0..rivers.len() {
        if rivers[index].end != RiverEnd::Sea || rivers[index].points.len() < 8 {
            continue;
        }
        let mouth = choose(settings, heights, &rivers[index], index);
        if mouth != Mouth::Plain {
            let end = *rivers[index].points.last().unwrap();
            // Deltas are mostly vegetated, with a sandy rim; estuaries are lined with sand.
            let sandiness = if mouth == Mouth::Delta { 0. } else { 1. };
            spread_sediment(settings, shore, heights, &mut sediment, end, sandiness);
        }
        match mouth {
            Mouth::Plain => {}
            Mouth::Estuary => widen(&mut rivers[index]),
            Mouth::Delta => {
                // The delta starts no earlier than the river's last confluence: its tributaries
                // must still find it.
                let last_join = (0..rivers.len())
                    .filter(|&t| rivers[t].end == RiverEnd::Junction(index))
                    .filter_map(|t| rivers[t].points.last())
                    .filter_map(|end| {
                        let river = &rivers[index].points;
                        (0..river.len()).min_by(|&a, &b| {
                            xz(river[a].position)
                                .distance_squared(xz(end.position))
                                .total_cmp(&xz(river[b].position).distance_squared(xz(end.position)))
                        })
                    })
                    .max()
                    .unwrap_or(0);
                let distributaries = build_delta(
                    settings,
                    noise,
                    shore,
                    heights,
                    &mut wetland,
                    &mut rivers[index],
                    index,
                    last_join,
                );
                rivers.extend(distributaries);
            }
        }
        mouths.push((index, mouth));
    }
    (mouths, sediment, wetland)
}

/// Sediment fan around a mouth (radius 6 + 5·width cells): the land there is lowered to a
/// low, flat deposit plain. The sea floor is left alone.
fn spread_sediment(
    settings: &WorldgenSettings,
    shore: &[f32],
    heights: &mut [f32],
    sediment: &mut [f32],
    end: RiverPoint,
    sandiness: f32,
) {
    let radius = 6. + 5. * end.width / settings.cell_size;
    for_cells_around(
        settings.size,
        cell_of(settings, end.position),
        radius,
        |i, p| {
            let value = smoothstep(
                radius,
                radius * 0.3,
                p.distance(cell_of(settings, end.position)),
            );
            sediment[i] = sediment[i].max(value * sandiness);
            let h = heights[i];
            if h > 0. {
                let deposit = h.min(0.25 + 0.06 * shore[i].max(0.));
                heights[i] = h + (deposit - h) * value;
            }
        },
    );
}

fn choose(settings: &WorldgenSettings, heights: &[f32], river: &River, index: usize) -> Mouth {
    let end = river.points.last().unwrap();
    let width = end.width;
    let roll = hash(settings.seed as u32, index as u32, 0x0D17A) as f32 / u32::MAX as f32;
    let large = width >= settings.delta_min_width;
    let flat = mean_land_height(settings, heights, end.position, 15.) < 3.;
    match (large, flat) {
        (true, true) if roll < 0.55 => Mouth::Delta,
        (true, true) if roll < 0.85 => Mouth::Estuary,
        (true, false) if roll < 0.4 => Mouth::Estuary,
        (false, _) if width >= settings.delta_min_width * 0.6 && roll < 0.2 => Mouth::Estuary,
        _ => Mouth::Plain,
    }
}

/// Mean height of the land cells within `radius` cells of `position`.
fn mean_land_height(
    settings: &WorldgenSettings,
    heights: &[f32],
    position: Vec3,
    radius: f32,
) -> f32 {
    let size = settings.size;
    let center = cell_of(settings, position);
    let (mut sum, mut count) = (0., 0.);
    for_cells_around(size, center, radius, |i, _| {
        if heights[i] > 0. {
            sum += heights[i];
            count += 1.;
        }
    });
    if count > 0. { sum / count } else { 0. }
}

/// Estuary: the channel widens and deepens a little over its last stretch.
fn widen(river: &mut River) {
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

/// Delta: the river now ends where it reaches the shore, and splits there into a tree of
/// distributaries. Channels fork again and again, unevenly; each wanders out to sea and ends
/// at its own distance. The delta land is built along the channels (natural levees), widest
/// near the apex: the shoreline juts out at every channel mouth, with bays between them.
/// The apex is at least at point `last_join` (the river's last confluence). Returns the
/// distributaries.
fn build_delta(
    settings: &WorldgenSettings,
    noise: Noise,
    shore: &[f32],
    heights: &mut [f32],
    wetland: &mut [f32],
    river: &mut River,
    index: usize,
    last_join: usize,
) -> Vec<River> {
    let points = &river.points;
    let count = points.len();
    // The delta fans out toward the open sea (down the shore distance), not necessarily
    // along the river's last stretch, which may run along the coast.
    let flow =
        (xz(points[count - 1].position) - xz(points[count - 7].position)).normalize_or_zero();
    let seaward = {
        let size = settings.size as i32;
        let c = cell_of(settings, points[count - 1].position).round().as_ivec2();
        let at =
            |x: i32, z: i32| shore[(z.clamp(0, size - 1) * size + x.clamp(0, size - 1)) as usize];
        -Vec2::new(
            at(c.x + 2, c.y) - at(c.x - 2, c.y),
            at(c.x, c.y + 2) - at(c.x, c.y - 2),
        )
        .normalize_or_zero()
    };
    let direction = (0.4 * flow + 0.6 * seaward).normalize_or_zero();
    if direction == Vec2::ZERO {
        return Vec::new();
    }
    // The apex: the last point over land.
    let shore = (0..count)
        .rev()
        .find(|&k| heights[index_of(settings, points[k].position)] > 0.)
        .unwrap_or(count - 1)
        .max(last_join)
        .clamp(1, count - 1);
    river.points.truncate(shore + 1);
    let apex = river.points[shore];

    let mut rng = StdRng::seed_from_u64(settings.seed ^ (index as u64).wrapping_mul(0x9E37_79B9));
    let length = (14. + 7. * apex.width) * rng.random_range(0.8..1.2f32);
    let is_sea = |p: Vec2| heights[index_of(settings, Vec3::new(p.x, 0., p.y))] <= 0.;
    let channels = grow_channels(
        &mut rng,
        noise.layer(40 + index as u32),
        &is_sea,
        apex,
        direction,
        length,
    );
    raise_delta_land(
        settings,
        noise.layer(60 + index as u32),
        heights,
        wetland,
        &channels,
        length,
    );

    // Water falls from the apex level to just under the sea at the full delta length.
    let level =
        |travelled: f32| apex.position.y + (-0.1 - apex.position.y) * (travelled / length).min(1.);
    channels
        .into_iter()
        .map(|channel| River {
            points: channel
                .points
                .iter()
                .map(|point| RiverPoint {
                    position: Vec3::new(point.position.x, level(point.travelled), point.position.y),
                    width: point.width,
                    depth: apex.depth * (point.width / apex.width).sqrt(),
                })
                .collect(),
            end: RiverEnd::Sea,
        })
        .collect()
}

struct Channel {
    points: Vec<ChannelPoint>,
    /// Ends in the sea (as opposed to forking, or giving up over land).
    open: bool,
}

#[derive(Clone, Copy)]
struct ChannelPoint {
    position: Vec2,
    width: f32,
    /// Distance from the apex along the channels.
    travelled: f32,
    /// Distance to the channel's tip (infinite where it forks).
    to_tip: f32,
}

/// Grow the distributary tree from the apex.
fn grow_channels(
    rng: &mut impl Rng,
    noise: Noise,
    is_sea: &dyn Fn(Vec2) -> bool,
    apex: RiverPoint,
    direction: Vec2,
    length: f32,
) -> Vec<Channel> {
    // (start, heading, width, travelled, forks left)
    let forks = if apex.width > 2.5 { 4 } else { 3 };
    let mut stack = vec![(xz(apex.position), direction, apex.width * 0.85, 0f32, forks)];
    let mut channels = Vec::new();
    let mut seed = 0u32;
    while let Some((start, mut heading, width, travelled, forks_left)) = stack.pop() {
        seed += 1;
        let forks = forks_left > 0 && width > 0.7;
        // Where this channel forks, or where it reaches the sea (each tip at its own reach).
        let end = if forks {
            travelled + length * rng.random_range(0.15..0.35f32)
        } else {
            length * rng.random_range(0.75..1.25f32)
        }
        .max(travelled + 2.);
        let wavelength = 10. * width.max(0.6);
        let initial = heading;
        let mut points = Vec::new();
        let mut position = start;
        let mut s = travelled;
        // Tips keep going until they reach the sea (within reason).
        let limit = end + length * 0.6;
        while s <= end || (!forks && !is_sea(position) && s <= limit) {
            points.push(ChannelPoint {
                position,
                width,
                travelled: s,
                to_tip: f32::INFINITY,
            });
            // Wander (meander-like curvature from noise), pulled back toward the channel's
            // initial heading so branches keep fanning out instead of curling back.
            let curvature =
                3. * noise.get(Vec2::new(s / wavelength, seed as f32 * 7.31)) / wavelength;
            let pull = heading.angle_to(initial) * 0.5 / wavelength;
            heading = Vec2::from_angle((curvature + pull) * SPACING)
                .rotate(heading)
                .normalize();
            // Never turn back inland.
            let off = direction.angle_to(heading);
            if off.abs() > MAX_TURN {
                heading = Vec2::from_angle(MAX_TURN * off.signum()).rotate(direction);
            }
            position += heading * SPACING;
            s += SPACING;
        }
        if !forks {
            for point in &mut points {
                point.to_tip = s - point.travelled;
            }
        }
        if forks {
            // Uneven split: one branch takes most of the flow and turns less.
            let share = rng.random_range(0.3..0.5f32);
            let (small, large) = (width * share.sqrt(), width * (1. - share).sqrt());
            let side = if rng.random_bool(0.5) { 1. } else { -1. };
            let small_turn = side * rng.random_range(0.45..0.8f32);
            let large_turn = -side * rng.random_range(0.1..0.35f32);
            let fork_travelled = s;
            stack.push((
                position,
                Vec2::from_angle(small_turn).rotate(heading),
                small,
                fork_travelled,
                forks_left - 1,
            ));
            stack.push((
                position,
                Vec2::from_angle(large_turn).rotate(heading),
                large,
                fork_travelled,
                forks_left - 1,
            ));
        }
        // Tips that never found the sea are dropped.
        let open = !forks && is_sea(position);
        if forks || open {
            channels.push(Channel { points, open });
        }
    }
    channels
}

/// Build the delta land around the channels: wide near the apex, narrowing to levees along
/// the outer channels, and open at each tip so every channel reaches the sea.
fn raise_delta_land(
    settings: &WorldgenSettings,
    noise: Noise,
    heights: &mut [f32],
    wetland: &mut [f32],
    channels: &[Channel],
    length: f32,
) {
    let points: Vec<&ChannelPoint> = channels.iter().flat_map(|c| &c.points).collect();
    let Some(first) = points.first() else {
        return;
    };
    let half = (settings.size / 2) as f32;
    let center = first.position / settings.cell_size + half;
    for_cells_around(
        settings.size,
        center,
        length * 1.6 / settings.cell_size,
        |i, cell| {
            let p = (cell - half) * settings.cell_size;
            let mut best: f32 = 0.;
            // Distance to the nearest channel, in levee widths.
            let mut near = f32::MAX;
            for point in &points {
                near = near.min(p.distance(point.position) / (2. * point.width + 1.));
                let t = (point.travelled / length).min(1.);
                // Land reach around this channel point: wide near the apex, levees further out,
                // and nothing at the very tip.
                let reach = (0.45 * length * (1. - t).powf(1.5) + 1.5 * point.width + 1.)
                    * smoothstep(0., 3. * point.width + 2., point.to_tip);
                if reach <= 0. {
                    continue;
                }
                let d = p.distance(point.position) / reach;
                if d < 1. {
                    let rise = (1. - d).sqrt() * (0.15 + 0.3 * (1. - t));
                    best = best.max(rise);
                }
            }
            if best > 0. {
                let wobble = 1. + 0.3 * noise.fbm(p / 5., 2);
                let target = 0.08 + best * wobble;
                // Wetlands on the new land, and along the channels.
                let wet = if heights[i] <= 0. { 1. } else { (1. - near).max(0.) };
                wetland[i] = wetland[i].max(wet);
                heights[i] = heights[i].max(target);
            }
        },
    );
    // Open every tip to the sea.
    for channel in channels.iter().filter(|channel| channel.open) {
        let Some(tip) = channel.points.last() else {
            continue;
        };
        let radius = (tip.width + 1.5) / settings.cell_size;
        for_cells_around(
            settings.size,
            tip.position / settings.cell_size + half,
            radius,
            |i, _| {
                heights[i] = heights[i].min(-0.3);
            },
        );
    }
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// Continuous cell coordinates of a world position.
fn cell_of(settings: &WorldgenSettings, p: Vec3) -> Vec2 {
    xz(p) / settings.cell_size + (settings.size / 2) as f32
}

fn index_of(settings: &WorldgenSettings, p: Vec3) -> usize {
    let c = cell_of(settings, p).round();
    let max = settings.size as f32 - 1.;
    c.y.clamp(0., max) as usize * settings.size + c.x.clamp(0., max) as usize
}

/// Call `f(index, cell)` for every cell within `radius` of `center` (cell coordinates).
fn for_cells_around(size: usize, center: Vec2, radius: f32, mut f: impl FnMut(usize, Vec2)) {
    let (min, max) = ((center - radius).floor(), (center + radius).ceil());
    for z in (min.y.max(0.) as usize)..=(max.y.min(size as f32 - 1.) as usize) {
        for x in (min.x.max(0.) as usize)..=(max.x.min(size as f32 - 1.) as usize) {
            let p = Vec2::new(x as f32, z as f32);
            if p.distance(center) <= radius {
                f(z * size + x, p);
            }
        }
    }
}
