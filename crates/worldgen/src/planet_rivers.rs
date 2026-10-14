//! Rivers on the planet: as on the flat map (`rivers.rs`), cells draining a large enough area
//! are rivers, traced from their mouths upstream (main stem = largest tributary), smoothed,
//! given a width and depth from their discharge, and meandering where the land is flat. Paths
//! live on the sphere (positions in cells from the centre, kept on the sphere).

use std::f32::consts::TAU;

use bevy_math::Vec3;

use crate::{RiverEnd, flow::FlowGraph, noise::smoothstep, sphere::SphereGrid};

/// Spacing of the output points, in cells.
const SPACING: f32 = 0.5;
/// Drainage area (mean cells) from which a cell is a river, width and depth there (world
/// units, as on the flat map), meander amplitude (channel widths) and the valley slope above
/// which rivers don't meander.
pub const RIVER_MIN_AREA: f32 = 400.;
const RIVER_WIDTH: f32 = 0.5;
const RIVER_DEPTH: f32 = 0.4;
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

/// A river, from its source to its end (the sea, a lake, or its parent river).
#[derive(Clone, Debug)]
pub struct PlanetRiver {
    pub points: Vec<PlanetRiverPoint>,
    pub end: RiverEnd,
}

/// Extract the rivers from the heights, the depression-filled surface they drain on, the sea
/// and the lakes (water depth over each cell).
pub fn extract(
    grid: &SphereGrid,
    radius: f32,
    heights: &[f32],
    filled: &[f32],
    sea: &[bool],
    lake_depth: &[f32],
) -> Vec<PlanetRiver> {
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
    let mut output_index: Vec<Option<usize>> = Vec::with_capacity(traced.len());
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
        let widths: Vec<f32> = path.iter().map(|p| RIVER_WIDTH * (p.2 / RIVER_MIN_AREA).sqrt()).collect();
        let depths: Vec<f32> =
            path.iter().map(|p| RIVER_DEPTH * (p.2 / RIVER_MIN_AREA).powf(0.4)).collect();
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
        let points = (0..length)
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
                PlanetRiverPoint {
                    direction: (position + normal * offset).normalize(),
                    level,
                    width: widths[i],
                    depth: depths[i],
                }
            })
            .collect();
        output_index.push(Some(rivers.len()));
        rivers.push(PlanetRiver { points, end: river.end });
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
    rivers
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
