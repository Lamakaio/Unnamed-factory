//! Rivers from the drainage network: cells draining a large enough area are rivers. They are
//! traced from their mouths upstream (main stem = largest tributary), smoothed, given a width
//! and depth from their discharge, and meander where the land is flat.

use std::f32::consts::TAU;

use bevy_math::{Vec2, Vec3, Vec3Swizzles};

use crate::{River, RiverEnd, RiverPoint, WorldgenSettings, flow::FlowGraph, noise::smoothstep};

/// Spacing of the output polyline points, in cells.
const SPACING: f32 = 0.5;

/// A river point of the un-meandered valley path, used to shape floodplains.
pub struct ValleyPoint {
    pub position: Vec2,
    pub water_level: f32,
    /// Half-width of the floodplain (0 where the river doesn't meander).
    pub floodplain: f32,
}

struct Traced {
    cells: Vec<usize>,
    end: RiverEnd,
    /// Cell where this river ends, if it is a tributary.
    junction: Option<usize>,
}

/// Extract the river network. `routing` must be depression-free (Priority-Flood with epsilon);
/// `lake_of` gives each cell's lake.
///
/// Water levels follow the actual `terrain`, kept non-increasing downstream: rivers cut through
/// small barriers (breaching) instead of floating over filled depressions.
pub fn extract_rivers(
    settings: &WorldgenSettings,
    terrain: &[f32],
    routing: &[f32],
    base_level: &[bool],
    lake_of: &[Option<u16>],
    lake_levels: &[f32],
) -> (Vec<River>, Vec<ValleyPoint>) {
    let size = settings.size;
    let graph = FlowGraph::new(size, routing, base_level);
    let area = graph.accumulate(|_| 1.);
    let is_river =
        |i: usize| area[i] >= settings.river_min_area && !base_level[i] && lake_of[i].is_none();

    // River donors of each river cell.
    let mut donors: Vec<Vec<usize>> = vec![Vec::new(); routing.len()];
    for i in 0..routing.len() {
        let r = graph.receivers[i];
        if r != i && is_river(i) && is_river(r) {
            donors[r].push(i);
        }
    }

    // Mouths: river cells flowing into the sea or a lake.
    let mut mouths: Vec<usize> = (0..routing.len())
        .filter(|&i| is_river(i) && !is_river(graph.receivers[i]))
        .collect();
    mouths.sort_by(|&a, &b| area[b].total_cmp(&area[a]));

    // Trace upstream; parents are always traced before their tributaries.
    let mut traced: Vec<Traced> = Vec::new();
    let mut queue: Vec<(usize, RiverEnd, Option<usize>)> = mouths
        .into_iter()
        .map(|mouth| {
            let r = graph.receivers[mouth];
            let end = match lake_of[r] {
                Some(lake) => RiverEnd::Lake(lake as usize),
                None => RiverEnd::Sea,
            };
            (mouth, end, None)
        })
        .collect();
    let mut k = 0;
    while k < queue.len() {
        let (start, end, junction) = queue[k];
        k += 1;
        let index = traced.len();
        let mut cells = vec![start];
        let mut current = start;
        loop {
            let mut upstream = donors[current].clone();
            if upstream.is_empty() {
                break;
            }
            upstream.sort_by(|&a, &b| area[b].total_cmp(&area[a]));
            for &tributary in &upstream[1..] {
                queue.push((tributary, RiverEnd::Junction(index), Some(current)));
            }
            current = upstream[0];
            cells.push(current);
        }
        cells.reverse();
        traced.push(Traced {
            cells,
            end,
            junction,
        });
    }

    let mut rivers: Vec<River> = Vec::with_capacity(traced.len());
    // Index in `rivers` of each traced river (short ones are skipped).
    let mut output_index: Vec<Option<usize>> = Vec::with_capacity(traced.len());
    let mut valley = Vec::new();
    for river in &traced {
        // Polyline through the cells, ending in the receiver (sea, lake, or parent river).
        let mut path: Vec<(Vec3, f32)> = river
            .cells
            .iter()
            .map(|&i| (cell_point(size, i, terrain[i]), area[i]))
            .collect();
        let last = *river.cells.last().unwrap();
        let outlet = river.junction.unwrap_or(graph.receivers[last]);
        let outlet_level = match river.end {
            RiverEnd::Sea => 0f32.min(terrain[last]),
            RiverEnd::Lake(lake) => lake_levels[lake],
            RiverEnd::Junction(_) => terrain[outlet],
        };
        path.push((cell_point(size, outlet, outlet_level), area[last]));
        if path.len() < 3 {
            output_index.push(None);
            continue;
        }

        let mut path = resample(&chaikin(&path, 3), SPACING);
        // Water only flows down.
        for i in 1..path.len() {
            path[i].0.y = path[i].0.y.min(path[i - 1].0.y);
        }

        let widths: Vec<f32> = path
            .iter()
            .map(|(_, a)| settings.river_width * (a / settings.river_min_area).sqrt())
            .collect();
        let depths: Vec<f32> = path
            .iter()
            .map(|(_, a)| settings.river_depth * (a / settings.river_min_area).powf(0.4))
            .collect();

        // Meanders where the valley is flat, with a wavelength of ~12 channel widths.
        let length = path.len();
        let slope: Vec<f32> = (0..length)
            .map(|i| {
                let (a, b) = (i.saturating_sub(10), (i + 10).min(length - 1));
                (path[a].0.y - path[b].0.y) / ((b - a).max(1) as f32 * SPACING)
            })
            .collect();
        let flatness = moving_average(
            &slope
                .iter()
                .map(|&s| smoothstep(settings.meander_max_slope, 0., s))
                .collect::<Vec<_>>(),
            20,
        );
        let mut phase = 0f32;
        let mut meandered = Vec::with_capacity(length);
        for i in 0..length {
            let (position, _) = path[i];
            let wavelength = (12. * widths[i]).max(8.);
            phase += TAU * SPACING / wavelength;
            // No meanders near the ends, so junctions and mouths stay put.
            let from_start = i as f32 * SPACING;
            let from_end = (length - 1 - i) as f32 * SPACING;
            let taper =
                smoothstep(0., wavelength, from_start) * smoothstep(0., wavelength, from_end);
            let amplitude = settings.meander_amplitude * widths[i] * flatness[i] * taper;
            let tangent = path[(i + 2).min(length - 1)].0.xz() - path[i.saturating_sub(2)].0.xz();
            let normal = tangent.normalize_or_zero().perp();
            let offset = amplitude * (phase.sin() + 0.2 * (3. * phase).sin());
            meandered.push((position + (normal * offset).extend(0.).xzy(), i));
            valley.push(ValleyPoint {
                position: position.xz(),
                water_level: position.y,
                floodplain: if flatness[i] > 0.05 {
                    amplitude.max(widths[i]) * 1.2 + widths[i]
                } else {
                    0.
                },
            });
        }

        // Meanders offset along the valley's normal fold over on bends tighter than their
        // amplitude: relax every bend tighter than the channel can carry.
        relax_bends(&mut meandered, &widths);

        let mut points: Vec<RiverPoint> = meandered
            .iter()
            .map(|&(position, i)| RiverPoint {
                position,
                width: widths[i],
                depth: depths[i],
            })
            .collect();

        // Tributaries end exactly on their parent's (final) polyline.
        let end = match river.end {
            RiverEnd::Junction(parent) => match output_index[parent] {
                Some(parent) => RiverEnd::Junction(parent),
                None => RiverEnd::Sea,
            },
            end => end,
        };
        if let RiverEnd::Junction(parent) = end
            && let Some(parent) = rivers.get(parent)
        {
            let end = points.last().unwrap().position;
            // Join a little downstream of the nearest point, so the tributary meets its parent
            // at an angle leaning downstream, as real confluences do.
            let distance = |k: usize| parent.points[k].position.xz().distance_squared(end.xz());
            let nearest = (0..parent.points.len())
                .min_by(|&a, &b| distance(a).total_cmp(&distance(b)))
                .map(|k| &parent.points[(k + JOIN_LEAD).min(parent.points.len() - 1)]);
            if let Some(nearest) = nearest {
                let last = points.last_mut().unwrap();
                last.position = nearest.position;
                // The snap can kink the end: relax it again.
                let mut path: Vec<(Vec3, usize)> =
                    points.iter().enumerate().map(|(i, p)| (p.position, i)).collect();
                let widths: Vec<f32> = points.iter().map(|p| p.width).collect();
                relax_bends(&mut path, &widths);
                for (point, (position, _)) in points.iter_mut().zip(path) {
                    point.position = position;
                }
            }
        }
        output_index.push(Some(rivers.len()));
        rivers.push(River { points, end });
    }

    // Convert to world units.
    for river in &mut rivers {
        for point in &mut river.points {
            let p = point.position;
            point.position = Vec3::new(
                (p.x - size as f32 / 2.) * settings.cell_size,
                p.y,
                (p.z - size as f32 / 2.) * settings.cell_size,
            );
            point.width *= settings.cell_size;
        }
    }
    (rivers, valley)
}

/// Tributaries join their parent this many points (of 0.5 cells) downstream of the nearest.
const JOIN_LEAD: usize = 2;

/// Smallest radius of a river bend, relative to the channel width (and at least this many
/// cells), so the channel and its banks never fold over themselves.
const MIN_BEND_RADIUS: f32 = 1.2;
const MIN_BEND_RADIUS_CELLS: f32 = 1.5;

/// Move points of a polyline (`(position, index into widths)`, in cells) towards their
/// neighbours' midpoint wherever it turns more sharply than [`MIN_BEND_RADIUS`] allows (a
/// curvature-limited Laplacian smoothing). The ends stay put; heights are unchanged.
fn relax_bends(path: &mut [(Vec3, usize)], widths: &[f32]) {
    let n = path.len();
    if n < 3 {
        return;
    }
    for _ in 0..200 {
        let mut moved = false;
        for i in 1..n - 1 {
            let (a, b, c) = (path[i - 1].0.xz(), path[i].0.xz(), path[i + 1].0.xz());
            let (ab, bc) = (b - a, c - b);
            let (la, lc) = (ab.length(), bc.length());
            if la < 1e-4 || lc < 1e-4 {
                path[i].0 = ((a + c) / 2.).extend(path[i].0.y).xzy();
                moved = true;
                continue;
            }
            let turn = (ab / la).dot(bc / lc).clamp(-1., 1.).acos();
            // Radius of the circle through the three points: chord / (2 sin(turn / 2)) ~
            // mean segment length / turn.
            let radius = (la + lc) / 2. / turn.max(1e-4);
            let min_radius = (MIN_BEND_RADIUS * widths[path[i].1]).max(MIN_BEND_RADIUS_CELLS);
            if radius < min_radius {
                let target = (a + c) / 2.;
                path[i].0 = b.lerp(target, 0.5).extend(path[i].0.y).xzy();
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

/// Cell center as (x, height, z), in cells.
fn cell_point(size: usize, i: usize, height: f32) -> Vec3 {
    Vec3::new((i % size) as f32, height, (i / size) as f32)
}

/// Chaikin corner cutting, keeping both ends.
fn chaikin(path: &[(Vec3, f32)], iterations: usize) -> Vec<(Vec3, f32)> {
    let mut path = path.to_vec();
    for _ in 0..iterations {
        let mut next = Vec::with_capacity(path.len() * 2);
        next.push(path[0]);
        for pair in path.windows(2) {
            let ((a, wa), (b, wb)) = (pair[0], pair[1]);
            next.push((a.lerp(b, 0.25), wa + (wb - wa) * 0.25));
            next.push((a.lerp(b, 0.75), wa + (wb - wa) * 0.75));
        }
        next.push(*path.last().unwrap());
        path = next;
    }
    path
}

/// Resample a polyline at a constant horizontal spacing.
fn resample(path: &[(Vec3, f32)], spacing: f32) -> Vec<(Vec3, f32)> {
    let mut result = vec![path[0]];
    let mut carry = 0.;
    for pair in path.windows(2) {
        let ((a, wa), (b, wb)) = (pair[0], pair[1]);
        let length = a.xz().distance(b.xz());
        let mut t = spacing - carry;
        while t <= length {
            let f = t / length;
            result.push((a.lerp(b, f), wa + (wb - wa) * f));
            t += spacing;
        }
        carry = length - (t - spacing);
    }
    if result.last().unwrap().0 != path.last().unwrap().0 {
        result.push(*path.last().unwrap());
    }
    result
}

fn moving_average(values: &[f32], radius: usize) -> Vec<f32> {
    (0..values.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(radius), (i + radius + 1).min(values.len()));
            values[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect()
}
