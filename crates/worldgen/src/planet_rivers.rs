//! Rivers on the planet: as on the flat map (`rivers.rs`), cells draining a large enough area
//! are rivers, traced from their mouths upstream (main stem = largest tributary), smoothed,
//! given a width and depth from their discharge, and meandering where the land is flat. Paths
//! live on the sphere (positions in cells from the centre, kept on the sphere).

use std::f32::consts::TAU;

use bevy_math::Vec3;

use crate::{RiverEnd, flow::FlowGraph, grid::Grid, noise::smoothstep, sphere::SphereGrid};

/// Spacing of the output points, in cells.
const SPACING: f32 = 0.5;
/// Drainage area (mean cells) from which a cell is a river, width and depth there (world
/// units, as on the flat map), meander amplitude (channel widths) and the valley slope above
/// which rivers don't meander.
pub const RIVER_MIN_AREA: f32 = 400.;
/// Width and depth of the smallest rivers, and how they grow with the area drained (power of
/// the area: the largest rivers are some 20 times wider than the smallest).
const RIVER_WIDTH: (f32, f32) = (0.25, 0.64);
const RIVER_DEPTH: (f32, f32) = (0.25, 0.5);
const MEANDER_AMPLITUDE: f32 = 1.5;
const MEANDER_MAX_SLOPE: f32 = 0.05;
/// Lake depth from which a cell is under a lake (rivers end there).
const LAKE: f32 = 0.05;

/// A point of a planet river.
#[derive(Clone, Copy, Debug)]
pub struct PlanetRiverPoint {
    /// Unit direction from the planet's centre.
    pub direction: Vec3,
    /// Water surface height above sea level (world units).
    pub level: f32,
    /// Channel width and depth below the water surface (world units).
    pub width: f32,
    pub depth: f32,
}

/// A point of a river's un-meandered valley path, to shape its floodplain (as on the flat
/// map, `rivers::ValleyPoint`).
#[derive(Clone, Copy, Debug)]
pub struct ValleyPoint {
    pub direction: Vec3,
    pub level: f32,
    /// Half-width of the floodplain (world units; 0 where the river doesn't meander).
    pub floodplain: f32,
}

/// A river, from its source to its end (the sea, a lake, or its parent river).
#[derive(Clone, Debug)]
pub struct PlanetRiver {
    pub points: Vec<PlanetRiverPoint>,
    pub end: RiverEnd,
    /// A side channel of a river splitting on flat land: it leaves this river (index) and
    /// rejoins it (`end`).
    pub branch_of: Option<usize>,
}

/// Extract the rivers from the heights, the depression-filled surface they drain on, the sea
/// and the lakes (water depth over each cell), and their valley paths.
pub fn extract(
    grid: &SphereGrid,
    radius: f32,
    heights: &[f32],
    filled: &[f32],
    sea: &[bool],
    lake_depth: &[f32],
) -> (Vec<PlanetRiver>, Vec<ValleyPoint>) {
    let graph = FlowGraph::new(grid, filled, sea);
    let area = graph.accumulate(|i| grid.area(i));
    let n = heights.len();
    let is_river = |i: usize| area[i] >= RIVER_MIN_AREA && !sea[i] && lake_depth[i] < LAKE;
    let mut donors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        let r = graph.receivers[i];
        if r != i && is_river(i) && is_river(r) {
            donors[r].push(i);
        }
    }
    let mut mouths: Vec<usize> = (0..n).filter(|&i| is_river(i) && !is_river(graph.receivers[i])).collect();
    mouths.sort_by(|&a, &b| area[b].total_cmp(&area[a]));

    // Trace upstream; parents come before their tributaries.
    struct Traced {
        cells: Vec<usize>,
        end: RiverEnd,
        junction: Option<usize>,
    }
    let mut traced: Vec<Traced> = Vec::new();
    let mut queue: Vec<(usize, RiverEnd, Option<usize>)> = mouths
        .into_iter()
        .map(|mouth| {
            let r = graph.receivers[mouth];
            // (Planet lakes have no index yet: the end only says "a lake".)
            let end = if sea[r] { RiverEnd::Sea } else { RiverEnd::Lake(0) };
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
        traced.push(Traced { cells, end, junction });
    }

    let mut rivers = Vec::with_capacity(traced.len());
    let mut valley = Vec::new();
    let mut output_index: Vec<Option<usize>> = Vec::with_capacity(traced.len());
    // How flat the valley is at each point of each river (for the side channels).
    let mut flats: Vec<Vec<f32>> = Vec::with_capacity(traced.len());
    for river in &traced {
        let point = |i: usize| grid.direction(i) * radius;
        let mut path: Vec<(Vec3, f32, f32)> =
            river.cells.iter().map(|&i| (point(i), heights[i], area[i])).collect();
        let last = *river.cells.last().unwrap();
        let outlet = river.junction.unwrap_or(graph.receivers[last]);
        let outlet_level = match river.end {
            RiverEnd::Sea => 0f32.min(heights[last]),
            RiverEnd::Lake(_) => filled[outlet],
            RiverEnd::Junction(_) => heights[outlet],
        };
        path.push((point(outlet), outlet_level, area[last]));
        if path.len() < 3 {
            output_index.push(None);
            continue;
        }
        let mut path = resample(&chaikin(&path, 3, radius), SPACING);
        // Water only flows down.
        for i in 1..path.len() {
            path[i].1 = path[i].1.min(path[i - 1].1);
        }
        let length = path.len();
        let widths: Vec<f32> =
            path.iter().map(|p| RIVER_WIDTH.0 * (p.2 / RIVER_MIN_AREA).powf(RIVER_WIDTH.1)).collect();
        let depths: Vec<f32> =
            path.iter().map(|p| RIVER_DEPTH.0 * (p.2 / RIVER_MIN_AREA).powf(RIVER_DEPTH.1)).collect();
        // Meanders where the valley is flat, about 12 channel widths long.
        let slope: Vec<f32> = (0..length)
            .map(|i| {
                let (a, b) = (i.saturating_sub(10), (i + 10).min(length - 1));
                (path[a].1 - path[b].1) / ((b - a).max(1) as f32 * SPACING)
            })
            .collect();
        let flatness: Vec<f32> = (0..length)
            .map(|i| {
                let (a, b) = (i.saturating_sub(20), (i + 21).min(length));
                slope[a..b].iter().map(|&s| smoothstep(MEANDER_MAX_SLOPE, 0., s)).sum::<f32>()
                    / (b - a) as f32
            })
            .collect();
        let mut phase = 0f32;
        let mut points: Vec<PlanetRiverPoint> = (0..length)
            .map(|i| {
                let (position, level, _) = path[i];
                let wavelength = (12. * widths[i]).max(8.);
                phase += TAU * SPACING / wavelength;
                let from_start = i as f32 * SPACING;
                let from_end = (length - 1 - i) as f32 * SPACING;
                let taper = smoothstep(0., wavelength, from_start) * smoothstep(0., wavelength, from_end);
                let amplitude = MEANDER_AMPLITUDE * widths[i] * flatness[i] * taper;
                let tangent = path[(i + 2).min(length - 1)].0 - path[i.saturating_sub(2)].0;
                let up = position.normalize();
                let normal = tangent.cross(up).normalize_or_zero();
                let offset = amplitude * (phase.sin() + 0.2 * (3. * phase).sin());
                valley.push(ValleyPoint {
                    direction: up,
                    level,
                    floodplain: if flatness[i] > 0.05 {
                        amplitude.max(widths[i]) * 1.2 + widths[i]
                    } else {
                        0.
                    },
                });
                PlanetRiverPoint {
                    direction: (position + normal * offset).normalize(),
                    level,
                    width: widths[i],
                    depth: depths[i],
                }
            })
            .collect();
        // Tributaries end exactly on their parent's (final) path.
        if let RiverEnd::Junction(parent) = river.end
            && let Some(&Some(parent)) = output_index.get(parent)
        {
            join_parent(&mut points, &rivers[parent]);
        }
        output_index.push(Some(rivers.len()));
        flats.push(flatness);
        rivers.push(PlanetRiver { points, end: river.end, branch_of: None });
    }
    // Junctions point at output indices (short rivers were dropped).
    for river in &mut rivers {
        if let RiverEnd::Junction(parent) = river.end {
            river.end = match output_index[parent] {
                Some(parent) => RiverEnd::Junction(parent),
                None => RiverEnd::Sea,
            };
        }
    }
    split_on_flats(radius, &flats, &mut rivers);
    through_lakes(grid, radius, filled, lake_depth, &mut rivers);
    (rivers, valley)
}

/// Rivers split on flat land (anabranches): wide enough rivers on flat valley stretches get a
/// side channel leaving them and rejoining them downstream round a low island; not where a
/// tributary joins (it would cross the side channel), and only some stretches.
const SPLIT_MIN_WIDTH: f32 = 0.8;
const SPLIT_FLATNESS: f32 = 0.7;
/// Length of a split (channel widths, and at least world units), and how far apart they are
/// at least (widths).
const SPLIT_LENGTH: (f32, f32, f32) = (14., 24., 10.);
const SPLIT_SPACING: f32 = 30.;
/// Share of splittable stretches that split.
const SPLIT_CHANCE: f32 = 0.5;
/// Width of the side channel (share of the river's) and of the island between the two
/// (river widths, at its widest).
const SPLIT_SHARE: f32 = 0.6;
const ISLAND: (f32, f32) = (0.8, 1.6);

fn split_on_flats(radius: f32, flats: &[Vec<f32>], rivers: &mut Vec<PlanetRiver>) {
    let count = rivers.len();
    // Where tributaries join each river (point indices).
    let mut joins: Vec<Vec<usize>> = vec![Vec::new(); count];
    for river in rivers.iter() {
        if let RiverEnd::Junction(parent) = river.end
            && let Some(end) = river.points.last()
        {
            let main = &rivers[parent].points;
            if let Some(k) = (0..main.len())
                .max_by(|&a, &b| main[a].direction.dot(end.direction).total_cmp(&main[b].direction.dot(end.direction)))
            {
                joins[parent].push(k);
            }
        }
    }
    let random = |river: usize, k: usize, salt: u32| {
        crate::noise::hash(0x5_9117 ^ salt, river as u32, k as u32) as f32 / u32::MAX as f32
    };
    for index in 0..count {
        let main = rivers[index].points.clone();
        let flat = &flats[index];
        let mut k = 0;
        while k < main.len() {
            let width = main[k].width;
            let length = ((SPLIT_LENGTH.0 + (SPLIT_LENGTH.1 - SPLIT_LENGTH.0) * random(index, k, 1)) * width)
                .max(SPLIT_LENGTH.2);
            let steps = (length / SPACING) as usize;
            let end = k + steps;
            let fits = end + 4 < main.len()
                && k > 4
                && width >= SPLIT_MIN_WIDTH
                && (k..=end).all(|j| flat[j] >= SPLIT_FLATNESS)
                && !joins[index].iter().any(|&j| j + 6 >= k && j <= end + 6);
            if !fits {
                k += 2;
                continue;
            }
            if random(index, k, 2) < SPLIT_CHANCE {
                let side = if random(index, k, 3) < 0.5 { -1. } else { 1. };
                let island = (ISLAND.0 + (ISLAND.1 - ISLAND.0) * random(index, k, 4)) * width;
                let branch_width = width * SPLIT_SHARE;
                // Off to the side at most the half widths and the island, easing in and out.
                let reach = width / 2. + branch_width / 2. + island;
                let points = (k..=end)
                    .map(|j| {
                        let t = (j - k) as f32 / steps as f32;
                        let p = main[j];
                        let (before, after) = (main[j - 1].direction, main[j + 1].direction);
                        let across = (after - before).cross(p.direction).normalize_or_zero();
                        let offset = reach * (std::f32::consts::PI * t).sin().max(0.).powf(0.7) * side;
                        PlanetRiverPoint {
                            direction: (p.direction * radius + across * offset).normalize(),
                            width: branch_width * (0.6 + 0.4 * (std::f32::consts::PI * t).sin().max(0.)),
                            depth: p.depth * SPLIT_SHARE.sqrt(),
                            ..p
                        }
                    })
                    .collect();
                rivers.push(PlanetRiver { points, end: RiverEnd::Junction(index), branch_of: Some(index) });
            }
            k = end + (SPLIT_SPACING * width / SPACING) as usize;
        }
    }
}

/// Tributaries join their parent this many points downstream of the nearest (so they meet at
/// an angle leaning downstream, as real confluences do), the move spread over this many
/// points of their end (as on the flat map, `rivers::JOIN_LEAD`).
const JOIN_LEAD: usize = 2;
const JOIN_BLEND: usize = 8;

/// Move a tributary's end onto its parent's path, easing the points before it along.
fn join_parent(points: &mut [PlanetRiverPoint], parent: &PlanetRiver) {
    let n = points.len();
    let end = points[n - 1].direction;
    let Some(nearest) = (0..parent.points.len()).max_by(|&a, &b| {
        parent.points[a].direction.dot(end).total_cmp(&parent.points[b].direction.dot(end))
    }) else {
        return;
    };
    let target = parent.points[(nearest + JOIN_LEAD).min(parent.points.len() - 1)];
    let shift = target.direction - end;
    let blend = JOIN_BLEND.min(n - 1);
    for j in 0..=blend {
        let t = 1. - j as f32 / blend.max(1) as f32;
        let k = n - 1 - j;
        points[k].direction = (points[k].direction + shift * t * t * (3. - 2. * t)).normalize();
    }
    points[n - 1].level = target.level.min(points[n.saturating_sub(2)].level);
}

/// Lead rivers through the lakes they flow into or out of, to or from the lake's deepest
/// point, at its level (as on the flat map, `lakes::extend_through_lakes`): their channels are
/// carved through the bed, and they clearly join the lake rather than stop at its edge.
fn through_lakes(
    grid: &SphereGrid,
    radius: f32,
    filled: &[f32],
    lake_depth: &[f32],
    rivers: &mut [PlanetRiver],
) {
    let in_lake = |i: usize| lake_depth[i] >= LAKE;
    // Each lake's cells (their lake's id), and its deepest cell.
    let mut lake_of = vec![u32::MAX; lake_depth.len()];
    let mut deepest = Vec::new();
    for start in 0..lake_depth.len() {
        if !in_lake(start) || lake_of[start] != u32::MAX {
            continue;
        }
        let id = deepest.len() as u32;
        let mut queue = std::collections::VecDeque::from([start]);
        lake_of[start] = id;
        let mut best = start;
        while let Some(i) = queue.pop_front() {
            if lake_depth[i] > lake_depth[best] {
                best = i;
            }
            for (j, _) in grid.neighbours(i) {
                if in_lake(j) && lake_of[j] == u32::MAX {
                    lake_of[j] = id;
                    queue.push_back(j);
                }
            }
        }
        deepest.push(best);
    }
    // The lake cell at or next to a direction.
    let lake_cell_near = |direction: Vec3| {
        let cell = grid.cell_at(direction);
        std::iter::once(cell)
            .chain(grid.neighbours(cell).map(|(j, _)| j))
            .find(|&j| in_lake(j))
    };
    // Cells from `entry` to its lake's deepest, through the lake.
    let path_to_deepest = |entry: usize| {
        let lake = lake_of[entry];
        let target = deepest[lake as usize];
        let mut previous = std::collections::HashMap::from([(entry, entry)]);
        let mut queue = std::collections::VecDeque::from([entry]);
        while let Some(i) = queue.pop_front() {
            if i == target {
                break;
            }
            for (j, _) in grid.neighbours(i) {
                if lake_of[j] == lake && !previous.contains_key(&j) {
                    previous.insert(j, i);
                    queue.push_back(j);
                }
            }
        }
        previous.contains_key(&target).then(|| {
            let mut path = vec![target];
            while *path.last().unwrap() != entry {
                path.push(previous[path.last().unwrap()]);
            }
            path.reverse();
            path
        })
    };
    // River points along directions (smoothed), every `SPACING`, at a level.
    let points_along = |mut route: Vec<Vec3>, like: PlanetRiverPoint, level: f32| {
        for _ in 0..3 {
            route = (0..route.len())
                .map(|k| match k {
                    0 => route[0],
                    k if k + 1 == route.len() => route[k],
                    k => (route[k - 1] + route[k] * 2. + route[k + 1]).normalize(),
                })
                .collect();
        }
        let mut points = Vec::new();
        for pair in route.windows(2) {
            let steps = ((pair[0].distance(pair[1]) * radius / SPACING).ceil() as usize).max(1);
            for k in 0..steps {
                let direction = pair[0].lerp(pair[1], k as f32 / steps as f32).normalize();
                points.push(PlanetRiverPoint { direction, level, ..like });
            }
        }
        points.push(PlanetRiverPoint { direction: *route.last().unwrap(), level, ..like });
        points
    };
    for river in rivers.iter_mut() {
        let (first, last) = (river.points[0], *river.points.last().unwrap());
        // Into a lake: on to its deepest point.
        let into = match river.end {
            RiverEnd::Lake(_) => lake_cell_near(last.direction),
            _ => None,
        };
        if let Some(entry) = into
            && let Some(path) = path_to_deepest(entry)
        {
            let level = filled[entry].min(last.level);
            let mut route = vec![last.direction];
            route.extend(path.iter().map(|&i| grid.direction(i)));
            river.points.extend(points_along(route, last, level).into_iter().skip(1));
        }
        // Out of a lake (next to the source, not the one it flows into): from its deepest.
        if let Some(entry) = lake_cell_near(first.direction)
            && into.is_none_or(|into| lake_of[into] != lake_of[entry])
            && let Some(path) = path_to_deepest(entry)
        {
            let mut route: Vec<Vec3> = path.iter().rev().map(|&i| grid.direction(i)).collect();
            route.push(first.direction);
            let mut points = points_along(route, first, filled[entry].max(first.level));
            points.pop();
            points.extend(river.points.iter().copied());
            river.points = points;
        }
    }
}

/// Chaikin corner cutting on the sphere (positions kept at `radius`).
fn chaikin(path: &[(Vec3, f32, f32)], iterations: usize, radius: f32) -> Vec<(Vec3, f32, f32)> {
    let mut path = path.to_vec();
    for _ in 0..iterations {
        let mut next = Vec::with_capacity(path.len() * 2);
        next.push(path[0]);
        for pair in path.windows(2) {
            let ((a, la, wa), (b, lb, wb)) = (pair[0], pair[1]);
            for t in [0.25, 0.75] {
                next.push(((a.lerp(b, t)).normalize() * radius, la + (lb - la) * t, wa + (wb - wa) * t));
            }
        }
        next.push(*path.last().unwrap());
        path = next;
    }
    path
}

/// Resample a path at a constant spacing (cells, along chords: short enough to equal arcs).
fn resample(path: &[(Vec3, f32, f32)], spacing: f32) -> Vec<(Vec3, f32, f32)> {
    let mut result = vec![path[0]];
    let mut carry = 0.;
    for pair in path.windows(2) {
        let ((a, la, wa), (b, lb, wb)) = (pair[0], pair[1]);
        let length = a.distance(b);
        let mut t = spacing - carry;
        while t <= length {
            let f = t / length;
            let radius = a.length();
            result.push((a.lerp(b, f).normalize() * radius, la + (lb - la) * f, wa + (wb - wa) * f));
            t += spacing;
        }
        carry = length - (t - spacing);
    }
    if result.last().unwrap().0 != path.last().unwrap().0 {
        result.push(*path.last().unwrap());
    }
    result
}

/// Call `f(cell, distance)` for every cell whose centre lies within `radius` (world units) of
/// `center` (a unit direction), walking the grid from the cell there.
pub(crate) fn for_cells_within(
    grid: &SphereGrid,
    radius_of_planet: f32,
    center: Vec3,
    radius: f32,
    mut f: impl FnMut(usize, f32),
) {
    let start = grid.cell_at(center);
    let mut seen = std::collections::HashSet::from([start]);
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some(i) = queue.pop_front() {
        let distance = grid.direction(i).distance(center) * radius_of_planet;
        if distance <= radius {
            f(i, distance);
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
