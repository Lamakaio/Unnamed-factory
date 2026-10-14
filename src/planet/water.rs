//! The planet's water, drawn with `PlanetWaterMaterial` (`planet_water.wesl`, the flat map's
//! toon water on the sphere), as on the flat map (`map::water`):
//! - rivers: ribbons along the water line of the flat map's river profile (pools and falls of
//!   varied slant, sloping chutes, domed steep stretches), with the flow speed and whitewater
//!   for the flow map. They fade out where they reach the sea or go under their lake (carrying
//!   on straight if needed), fade in where they leave a lake, and tributaries end on their
//!   parent's ribbon edge (a junction cap sharing its vertices), whitening the parent below;
//! - lakes: per face, outlined by marching squares where the ground rises above the water (a
//!   lake covers its extent, the lower ground connected to it and a little of the shore, not
//!   ground beyond its rim), reaching just into the river channels at its edge;
//! - the sea: a sphere at sea level, built face by face.
//!
//! Patterns use 2D coordinates in vertex colour red/green: along / across a river (so its
//! flow pattern follows it across cube faces; a junction cap uses its tributary's), the face's
//! cell coordinates elsewhere.

use std::collections::{HashMap, VecDeque};

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use worldgen::{
    RiverEnd, RiverPoint, planet_rivers::PlanetRiver, sphere::FACES, surface::PlanetSurface,
};

use crate::{
    map::water::{
        CONFLUENCE_FOAM, CONFLUENCE_REACH, DOME_DROP, DOME_SEGMENTS, DOME_SHAPE, DOME_SLOPE,
        LAKE_FADE_INSIDE, LAKE_SINK, OPEN_WATER_DEPTH, OUTLET_FADE, OUTSIDE, RIVER_EDGE,
        RIVER_FADE, RIVER_OFFSET, RIVER_OVERLAP, SHORE_MARGIN, SHORE_STEPS, clip_half_plane,
        contour_cell, fall_split, river_profile,
    },
    shaders::{PlanetWaterMaterial, PlanetWaterShader},
};

/// Lake depth from which a point is under a lake.
const LAKE: f32 = 0.02;
/// Largest sag of the sea's triangles below sea level (world units).
const SEA_SAG: f32 = 0.02;
/// Spacing of the rows added past a river's end while it fades.
const FADE_STEP: f32 = 0.5;
/// A junction cap looks for its parent's edge this far (world units) around the junction.
const CAP_SEARCH: f32 = 30.;

/// A mesh being built: positions, pattern coordinates (vertex colour), flow velocity (UV0)
/// and (whitewater, opacity) (UV1).
#[derive(Default)]
struct Water {
    positions: Vec<[f32; 3]>,
    coords: Vec<[f32; 4]>,
    flow: Vec<[f32; 2]>,
    flow_b: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Water {
    fn vertex(&mut self, position: Vec3, coords: Vec2, flow: Vec2, foam: f32, fade: f32) -> u32 {
        self.positions.push(position.to_array());
        self.coords.push([coords.x, coords.y, 0., 1.]);
        self.flow.push(flow.to_array());
        self.flow_b.push([foam, fade]);
        (self.positions.len() - 1) as u32
    }

    fn build(self, rivers: bool) -> Mesh {
        let normals: Vec<[f32; 3]> =
            self.positions.iter().map(|p| Vec3::from(*p).normalize_or_zero().to_array()).collect();
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.coords)
            .with_inserted_indices(Indices::U32(self.indices));
        // (UV1 tells the shader it is a river.)
        if rivers {
            mesh = mesh
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.flow)
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, self.flow_b);
        }
        mesh
    }
}

/// A cross-section of a river ribbon: its centre on the water line, the directions along and
/// across, its half-width, distance along the river, flow speed, whitewater, opacity and dome.
#[derive(Clone, Copy)]
struct Row {
    center: Vec3,
    forward: Vec3,
    across: Vec3,
    half: f32,
    along: f32,
    speed: f32,
    foam: f32,
    fade: f32,
    dome: f32,
}

impl Row {
    /// An edge vertex (`side` -1: left, 1: right), sunk with the dome's edges.
    fn edge(&self, side: f32) -> Vec3 {
        let sink = self.dome * (-DOME_SHAPE.1 - DOME_DROP);
        self.center + self.across * side * self.half + self.center.normalize() * sink
    }
}

/// Uncarved height of the cells' surface in a direction (no detail, no channels).
fn ground_at(surface: &PlanetSurface, direction: Vec3) -> f32 {
    let (face, p) = surface.face_coords(direction);
    surface.heights.bilinear(face, p)
}

/// The rows of a river along its water line (falls split in two rows, domes on steep
/// stretches), all opaque.
fn river_rows(surface: &PlanetSurface, river: &PlanetRiver) -> Vec<Row> {
    let points = &river.points;
    let count = points.len();
    // The river unrolled along its length, for the flat map's profile (which only uses
    // distances along it and levels). Rivers into the sea stay just under its level.
    let floor = if river.end == RiverEnd::Sea { -0.08 } else { f32::MIN };
    let mut along = 0.;
    let unrolled: Vec<RiverPoint> = points
        .iter()
        .enumerate()
        .map(|(k, p)| {
            if k > 0 {
                along += (p.direction - points[k - 1].direction).length() * surface.radius;
            }
            RiverPoint {
                position: Vec3::new(along, p.level.max(floor), 0.),
                width: p.width,
                depth: p.depth,
            }
        })
        .collect();
    let profile = river_profile(&unrolled);
    let mut rows: Vec<Row> = Vec::with_capacity(count + 8);
    let frame_at = |k: usize| {
        let (before, after) = (points[k.saturating_sub(1)], points[(k + 1).min(count - 1)]);
        let up = points[k].direction;
        let along = after.direction - before.direction;
        let across = along.cross(up).normalize_or_zero();
        (up.cross(across).normalize_or_zero(), across)
    };
    for k in 0..count {
        let up = points[k].direction;
        let here = &profile[k];
        let (forward, across) = frame_at(k);
        let row = Row {
            center: up * (surface.radius + here.level + RIVER_OFFSET),
            forward,
            across,
            half: points[k].width / 2. + RIVER_OVERLAP,
            along: unrolled[k].position.x,
            speed: here.speed,
            foam: here.foam,
            fade: 1.,
            dome: 0.,
        };
        rows.push(row);
        // A fall: a row a little downstream, at the next pool's level.
        if here.drop > 0. && k + 1 < count {
            let t = fall_split(&unrolled, &profile, k);
            let up = up.lerp(points[k + 1].direction, t).normalize();
            let along = unrolled[k].position.x
                + (unrolled[k + 1].position.x - unrolled[k].position.x) * t;
            rows.push(Row {
                center: up * (surface.radius + here.level - here.drop + RIVER_OFFSET),
                along,
                speed: here.speed.max(2.),
                foam: 1.,
                ..row
            });
        }
    }
    // Volume on steep stretches (slope measured over a few rows), flat at both ends.
    let n = rows.len();
    let levels: Vec<f32> = rows.iter().map(|r| r.center.length()).collect();
    for k in 0..n {
        let (a, b) = (k.saturating_sub(2), (k + 2).min(n - 1));
        let run = (rows[b].along - rows[a].along).max(1e-3);
        let slope = (levels[a] - levels[b]).max(0.) / run;
        let ends = (k.min(n - 1 - k) as f32 / 3.).min(1.);
        rows[k].dome = rows[k].half * smoothstep(DOME_SLOPE.0, DOME_SLOPE.1, slope) * ends;
    }
    rows
}

/// Fade a river out from row `start` on over `length`, carrying it on straight past its end
/// if needed, and drop the rows past the fade (`map::water::fade_out`).
fn fade_out(rows: &mut Vec<Row>, start: usize, length: f32) {
    let remaining = rows[rows.len() - 1].along - rows[start].along;
    if remaining < length && rows.len() >= 2 {
        let last = rows[rows.len() - 1];
        let radius = last.center.length();
        let mut center = last.center;
        for k in 1..=((length - remaining) / FADE_STEP).ceil() as usize {
            let up = center.normalize();
            let forward = (last.forward - up * last.forward.dot(up)).normalize_or_zero();
            center = (center + forward * FADE_STEP).normalize() * radius;
            rows.push(Row {
                center,
                forward,
                across: forward.cross(center.normalize()).normalize_or_zero(),
                along: last.along + FADE_STEP * k as f32,
                dome: 0.,
                ..last
            });
        }
    }
    let mut keep = rows.len();
    for k in start..rows.len() {
        let fade = 1. - (rows[k].along - rows[start].along) / length;
        rows[k].fade *= fade.max(0.);
        if fade <= 0. {
            keep = k + 1;
            break;
        }
    }
    rows.truncate(keep);
}

/// Fades at a river's ends: out of the lake it leaves, into the sea or lake it reaches (a
/// tributary's end is capped on its parent instead).
fn apply_fades(surface: &PlanetSurface, river: &PlanetRiver, rows: &mut Vec<Row>) {
    let under_lake = |row: &Row, side: f32| surface.lake_depth_at(row.edge(side).normalize()) > LAKE;
    let in_lake = |row: &Row| under_lake(row, -1.) && under_lake(row, 1.);
    // Out of a lake: visible a little way into it, then fading.
    let leave = if in_lake(&rows[0]) {
        rows.iter().position(|row| !in_lake(row)).unwrap_or(0)
    } else {
        0
    };
    if leave > 0 {
        for k in (0..leave).rev() {
            let distance = rows[leave].along - rows[k].along;
            rows[k].fade *= (1. - (distance - LAKE_FADE_INSIDE) / OUTLET_FADE).clamp(0., 1.);
        }
    }
    let first = (0..leave).rev().find(|&k| rows[k].fade <= 0.).unwrap_or(0);
    rows.drain(..first);
    let leave = (leave - first).min(rows.len() - 1);
    // Into the water it reaches (searched from where it leaves its own lake).
    let reached = rows[leave..].iter().position(|row| match river.end {
        RiverEnd::Lake(_) => in_lake(row),
        RiverEnd::Sea => {
            let bank = |side: f32| ground_at(surface, row.edge(side).normalize()) <= 0.02;
            (bank(-1.) && bank(1.)) || ground_at(surface, row.center.normalize()) < -OPEN_WATER_DEPTH
        }
        RiverEnd::Junction(_) => false,
    });
    match (reached, river.end) {
        (Some(start), RiverEnd::Lake(_)) => {
            let mut start = start + leave;
            let inside = rows[start].along + LAKE_FADE_INSIDE;
            while start + 1 < rows.len() && rows[start].along < inside {
                start += 1;
            }
            fade_out(rows, start, OUTLET_FADE);
        }
        (Some(start), _) => {
            let start = start + leave;
            fade_out(rows, start, RIVER_FADE.0 + RIVER_FADE.1 * 2. * rows[start].half);
        }
        // (Lake and sea ends always reach their water; just in case.)
        (None, RiverEnd::Sea | RiverEnd::Lake(_)) => {
            let start = rows.len() - 1;
            fade_out(rows, start, RIVER_FADE.0);
        }
        (None, RiverEnd::Junction(_)) => {}
    }
}

/// A vertex of a junction cap's outline: position, flow velocity (3D), whitewater.
#[derive(Clone, Copy)]
struct CapVertex {
    position: Vec3,
    flow: Vec3,
    foam: f32,
}

impl CapVertex {
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            position: self.position.lerp(other.position, t),
            flow: self.flow.lerp(other.flow, t),
            foam: self.foam + (other.foam - self.foam) * t,
        }
    }
}

/// Where segment `a` -> `b` crosses `c` -> `d` (2D): the parameters along each.
fn crossing(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(f32, f32)> {
    let (ab, cd, ac) = (b - a, d - c, c - a);
    let denominator = ab.perp_dot(cd);
    if denominator.abs() < 1e-9 {
        return None;
    }
    let t = ac.perp_dot(cd) / denominator;
    let u = ac.perp_dot(ab) / denominator;
    ((0.0..=1.).contains(&t) && (0.0..=1.).contains(&u)).then_some((t, u))
}

/// The end of a tributary on its parent's edge (`map::water::junction_cap`, in the plane
/// tangent at the junction): the number of rows drawn as strips, then the outline to fill
/// (from the last full row's left vertex along the tributary's left edge to the parent's
/// edge, along that edge, and back along the right edge).
fn junction_cap(
    rows: &[Row],
    edge: &[CapVertex],
    project: impl Fn(Vec3) -> Vec2,
) -> Option<(usize, Vec<CapVertex>)> {
    let tributary = |row: &Row, side: f32| CapVertex {
        position: row.edge(side),
        flow: row.forward * row.speed,
        foam: row.foam,
    };
    // Last crossing of each tributary edge with the parent's edge: (row, vertex, position
    // along the parent's edge).
    let hit = |side: f32| -> Option<(usize, CapVertex, f32)> {
        for k in (1..rows.len()).rev() {
            let (a, b) = (project(rows[k - 1].edge(side)), project(rows[k].edge(side)));
            for j in 1..edge.len() {
                let (c, d) = (edge[j - 1], edge[j]);
                if let Some((_, u)) = crossing(a, b, project(c.position), project(d.position)) {
                    return Some((k, c.lerp(d, u), (j - 1) as f32 + u));
                }
            }
        }
        None
    };
    // An edge still outside the parent at the end (joining at a shallow angle): its last
    // vertex moves onto the parent's edge.
    let snap = |side: f32| -> Option<(usize, CapVertex, f32)> {
        let p = project(rows.last()?.edge(side));
        let mut best: Option<(f32, CapVertex, f32)> = None;
        for j in 1..edge.len() {
            let (c, d) = (edge[j - 1], edge[j]);
            let (pc, pd) = (project(c.position), project(d.position));
            let cd = pd - pc;
            let u = ((p - pc).dot(cd) / cd.length_squared().max(1e-9)).clamp(0., 1.);
            let distance = p.distance(pc + cd * u);
            if best.is_none_or(|b| distance < b.0) {
                best = Some((distance, c.lerp(d, u), (j - 1) as f32 + u));
            }
        }
        best.map(|(_, vertex, at)| (rows.len() - 1, vertex, at))
    };
    let (left, right) = match (hit(-1.), hit(1.)) {
        (Some(left), Some(right)) => (left, right),
        (Some(left), None) => (left, snap(1.)?),
        (None, Some(right)) => (snap(-1.)?, right),
        (None, None) => return None,
    };
    let ((left_row, left_hit, left_at), (right_row, right_hit, right_at)) = (left, right);
    let full_rows = left_row.min(right_row);
    let mut outline: Vec<CapVertex> =
        rows[full_rows - 1..left_row].iter().map(|row| tributary(row, -1.)).collect();
    outline.push(left_hit);
    // Parent edge vertices strictly between the two crossings.
    if left_at < right_at {
        for k in left_at.floor() as usize + 1..=right_at.floor() as usize {
            outline.push(edge[k]);
        }
    } else {
        let mut k = left_at.floor() as usize;
        while k > right_at.floor() as usize {
            outline.push(edge[k]);
            k -= 1;
        }
    }
    outline.push(right_hit);
    outline.extend(rows[full_rows - 1..right_row].iter().rev().map(|row| tributary(row, 1.)));
    Some((full_rows, outline))
}

/// The parent's ribbon edge on the tributary's side (`near`) or the other, around the
/// junction.
fn parent_edge(parent: &[Row], rows: &[Row], near: bool) -> Vec<CapVertex> {
    let (end, approach) = (rows[rows.len() - 1].center, rows[rows.len().saturating_sub(6)].center);
    let Some(nearest) = (0..parent.len())
        .min_by(|&a, &b| parent[a].center.distance(end).total_cmp(&parent[b].center.distance(end)))
    else {
        return Vec::new();
    };
    let row = &parent[nearest];
    let side = if (row.edge(-1.).distance(approach) < row.edge(1.).distance(approach)) == near {
        -1.
    } else {
        1.
    };
    parent
        .iter()
        .filter(|row| row.center.distance(end) < CAP_SEARCH)
        .map(|row| CapVertex {
            position: row.edge(side),
            flow: row.forward * row.speed,
            foam: row.foam,
        })
        .collect()
}

/// Whether a point lies in a river's ribbon.
fn inside_ribbon(rows: &[Row], p: Vec3) -> bool {
    rows.iter().any(|row| {
        let offset = p - row.center;
        offset.dot(row.across).abs() < row.half && offset.dot(row.forward).abs() < 0.5
    })
}

/// River ribbons of the whole planet.
fn rivers_mesh(surface: &PlanetSurface) -> Mesh {
    let rivers = &surface.rivers;
    let mut all_rows: Vec<Vec<Row>> = rivers
        .iter()
        .map(|river| if river.points.len() >= 3 { river_rows(surface, river) } else { Vec::new() })
        .collect();
    // Whitewater on the parent below each confluence.
    for river in rivers {
        let (RiverEnd::Junction(parent), Some(join)) = (river.end, river.points.last()) else {
            continue;
        };
        let rows = &mut all_rows[parent];
        let join_at = join.direction * surface.radius;
        let Some(nearest) = (0..rows.len()).min_by(|&a, &b| {
            rows[a].center.distance(join_at).total_cmp(&rows[b].center.distance(join_at))
        }) else {
            continue;
        };
        let reach = CONFLUENCE_REACH.0 + CONFLUENCE_REACH.1 * join.width;
        for k in nearest..rows.len() {
            let mixing = CONFLUENCE_FOAM * (-(rows[k].along - rows[nearest].along) / reach).exp();
            if mixing < 0.02 {
                break;
            }
            rows[k].foam = rows[k].foam.max(mixing);
        }
    }
    let mut water = Water::default();
    let mut uncapped = 0;
    for (index, river) in rivers.iter().enumerate() {
        let mut rows = all_rows[index].clone();
        if rows.len() < 2 {
            continue;
        }
        apply_fades(surface, river, &mut rows);
        if rows.len() < 2 {
            continue;
        }
        let parent = match river.end {
            RiverEnd::Junction(parent) => Some(&all_rows[parent]).filter(|rows| rows.len() >= 2),
            _ => None,
        };
        let Some(parent) = parent else {
            for pair in rows.windows(2) {
                strip(&mut water, &pair[0], &pair[1]);
            }
            continue;
        };
        // The tributary ends on its parent's ribbon edge, in the plane tangent there.
        let up = rows[rows.len() - 1].center.normalize();
        let (east, north) = up.any_orthonormal_pair();
        let project = |p: Vec3| Vec2::new(p.dot(east), p.dot(north));
        let cap = junction_cap(&rows, &parent_edge(parent, &rows, true), project)
            .or_else(|| junction_cap(&rows, &parent_edge(parent, &rows, false), project));
        match cap {
            Some((full_rows, outline)) => {
                for pair in rows[..full_rows].windows(2) {
                    strip(&mut water, &pair[0], &pair[1]);
                }
                fan(&mut water, &rows[full_rows - 1], &outline);
            }
            // Lying entirely in its parent's channel: nothing to draw.
            None if rows.iter().all(|row| inside_ribbon(parent, row.center)) => {}
            None => {
                uncapped += 1;
                let last = rows.len() - 1;
                let length = 2. * rows[last].half;
                let start = rows.iter().position(|row| row.along > rows[last].along - length);
                fade_out(&mut rows, start.unwrap_or(last), length);
                for pair in rows.windows(2) {
                    strip(&mut water, &pair[0], &pair[1]);
                }
            }
        }
    }
    if uncapped > 0 {
        info!("{uncapped} planet tributaries never cross their parent's edge (faded instead)");
    }
    water.build(true)
}

/// Fill a junction cap's outline with a fan from its middle, in the pattern coordinates of
/// `row` (its tributary's last full row, whose vertices it shares).
fn fan(water: &mut Water, row: &Row, outline: &[CapVertex]) {
    let local = |v: &CapVertex| {
        let offset = v.position - row.center;
        let coords = Vec2::new(row.along + offset.dot(row.forward), offset.dot(row.across));
        let flow = Vec2::new(v.flow.dot(row.forward), v.flow.dot(row.across));
        (coords, flow)
    };
    let n = outline.len() as f32;
    let middle = CapVertex {
        position: outline.iter().map(|v| v.position).sum::<Vec3>() / n,
        flow: outline.iter().map(|v| v.flow).sum::<Vec3>() / n,
        foam: outline.iter().map(|v| v.foam).sum::<f32>() / n,
    };
    let (coords, flow) = local(&middle);
    let center = water.vertex(middle.position, coords, flow, middle.foam, 1.);
    let vertices: Vec<u32> = outline
        .iter()
        .map(|v| {
            let (coords, flow) = local(v);
            water.vertex(v.position, coords, flow, v.foam, 1.)
        })
        .collect();
    for (k, &vertex) in vertices.iter().enumerate() {
        water.indices.extend([center, vertex, vertices[(k + 1) % vertices.len()]]);
    }
}

/// A ribbon strip between two rows: flat across, or the top of an oval where domed (the edges
/// sunk under the banks, the whole lowered a little: see `map::water`).
fn strip(water: &mut Water, a: &Row, b: &Row) {
    let segments = if a.dome > 0. || b.dome > 0. { DOME_SEGMENTS } else { 1 };
    let first = water.positions.len() as u32;
    for row in [a, b] {
        let up = row.center.normalize();
        for k in 0..=segments {
            let (t, rise) = if segments == 1 {
                (k as f32, 0.)
            } else {
                let angle = std::f32::consts::PI * k as f32 / segments as f32;
                let rise = row.dome
                    * ((DOME_SHAPE.0 + DOME_SHAPE.1) * angle.sin() - DOME_SHAPE.1 - DOME_DROP);
                ((1. - angle.cos()) / 2., rise)
            };
            let offset = (t * 2. - 1.) * row.half;
            let position = row.center + row.across * offset + up * rise;
            water.vertex(
                position,
                Vec2::new(row.along, offset),
                Vec2::new(row.speed, 0.),
                row.foam,
                row.fade,
            );
        }
    }
    let columns = segments as u32 + 1;
    for k in 0..segments as u32 {
        let (a0, a1) = (first + k, first + k + 1);
        let (b0, b1) = (a0 + columns, a1 + columns);
        water.indices.extend([a0, b0, a1, a1, b0, b1]);
    }
}

/// What a sample of a face is to the lakes (see [`lakes_mesh`]).
#[derive(Clone, Copy, PartialEq)]
enum Sample {
    /// Under a lake (its water level).
    Lake(f32),
    /// Lower ground, not deep enough to count as lake.
    Low,
    /// Ground at or above the water around: the shore.
    Shore,
    /// In a river's channel outside the lakes.
    River,
}

/// Lake surfaces of one face, as on the flat map (`map::water::lakes_mesh`): on the cells'
/// samples, a lake covers its extent and the lower ground connected to it (never across the
/// shore: that is its rim), and up to `SHORE_STEPS` samples onto the shore; the outline is the
/// contour where the ground rises [`SHORE_MARGIN`] above the water, reaching just into river
/// channels. Samples just past the face's edges are used, and the outline clipped to it.
fn lakes_mesh(surface: &PlanetSurface, face: usize) -> Option<Mesh> {
    let n = surface.n as i32;
    let at = |x: i32, z: i32| Vec2::new(x as f32, z as f32);
    let inside = |x: i32, z: i32| (-1..=n).contains(&x) && (-1..=n).contains(&z);
    let mut kinds: HashMap<(i32, i32), (Sample, f32)> = HashMap::new();
    let mut kind = |x: i32, z: i32| -> (Sample, f32) {
        *kinds.entry((x, z)).or_insert_with(|| {
            let p = at(x, z);
            let depth = surface.lake_depth.bilinear(face, p);
            let ground = surface.height(face, p);
            let sample = if depth >= LAKE {
                Sample::Lake(surface.heights.bilinear(face, p) + depth)
            } else if depth > 0. {
                Sample::Low
            } else if surface.wetness(face, p) > 0. {
                Sample::River
            } else {
                Sample::Shore
            };
            (sample, ground)
        })
    };
    // Covered samples: (steps onto the shore, the level of the lake reaching them).
    let mut covered: HashMap<(i32, i32), (u8, f32)> = HashMap::new();
    let mut queue = VecDeque::new();
    for z in -1..=n {
        for x in -1..=n {
            if surface.lake_depth.bilinear(face, at(x, z)) < LAKE {
                continue;
            }
            if let (Sample::Lake(level), _) = kind(x, z) {
                covered.insert((x, z), (0, level));
                queue.push_back((x, z));
            }
        }
    }
    if covered.is_empty() {
        return None;
    }
    while let Some((x, z)) = queue.pop_front() {
        let (steps, level) = covered[&(x, z)];
        let (from, _) = kind(x, z);
        for (nx, nz) in [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)] {
            if !inside(nx, nz) || covered.contains_key(&(nx, nz)) {
                continue;
            }
            let (to, ground) = kind(nx, nz);
            let step = match (from, to) {
                (Sample::Lake(_) | Sample::Low, Sample::Low) if ground < level => 0,
                (_, Sample::Shore | Sample::Low | Sample::River) if steps < SHORE_STEPS => {
                    steps + 1
                }
                _ => continue,
            };
            covered.insert((nx, nz), (step, level));
            if to != Sample::River {
                queue.push_back((nx, nz));
            }
        }
    }
    // Contour field (positive under the water) and level of each covered sample.
    let field = |x: i32, z: i32, kinds: &HashMap<(i32, i32), (Sample, f32)>| match covered.get(&(x, z)) {
        None => (OUTSIDE, None),
        Some(&(_, level)) => {
            let (sample, ground) = kinds[&(x, z)];
            let water = level + SHORE_MARGIN - ground;
            let value = match sample {
                Sample::River => RIVER_EDGE,
                Sample::Lake(_) | Sample::Low => water.max(1e-3),
                Sample::Shore => water,
            };
            (value, Some(level))
        }
    };
    let mut squares: Vec<(i32, i32)> = covered
        .keys()
        .flat_map(|&(x, z)| [(x - 1, z - 1), (x, z - 1), (x - 1, z), (x, z)])
        .filter(|&(x, z)| inside(x, z) && inside(x + 1, z + 1))
        .collect();
    squares.sort_unstable();
    squares.dedup();
    let face_min = Vec2::splat(-0.5);
    let face_max = Vec2::splat(n as f32 - 0.5);
    let mut water = Water::default();
    for (x, z) in squares {
        let corners = [(x, z), (x + 1, z), (x + 1, z + 1), (x, z + 1)];
        let samples = corners.map(|(x, z)| field(x, z, &kinds));
        let values = samples.map(|(value, _)| value);
        if values.iter().all(|&v| v <= 0.) {
            continue;
        }
        let level = samples.iter().filter_map(|(_, level)| *level).fold(f32::MIN, f32::max);
        for mut polygon in contour_cell(corners.map(|(x, z)| at(x, z)), values) {
            // Clipped to the face.
            for (a, b) in [
                (face_min, Vec2::new(face_max.x, face_min.y)),
                (Vec2::new(face_max.x, face_min.y), face_max),
                (face_max, Vec2::new(face_min.x, face_max.y)),
                (Vec2::new(face_min.x, face_max.y), face_min),
            ] {
                polygon = clip_half_plane(&polygon, a, b, 1.);
                if polygon.len() < 3 {
                    break;
                }
            }
            if polygon.len() < 3 {
                continue;
            }
            let height = surface.radius + level - LAKE_SINK;
            let first = water.positions.len() as u32;
            for &p in &polygon {
                water.vertex(surface.direction(face, p) * height, p, Vec2::ZERO, 0., 1.);
            }
            for k in 1..polygon.len() as u32 - 1 {
                water.indices.extend([first, first + k, first + k + 1]);
            }
        }
    }
    (!water.indices.is_empty()).then(|| water.build(false))
}

/// The sea over one face: a grid fine enough that its triangles sag less than [`SEA_SAG`].
fn sea_mesh(surface: &PlanetSurface, face: usize) -> Mesh {
    let n = surface.n as f32;
    // A chord of angle θ sags by about R θ² / 8; a face spans about π/2.
    let angle = (8. * SEA_SAG / surface.radius).sqrt();
    let segments = ((std::f32::consts::FRAC_PI_2 / angle).ceil() as usize).max(8);
    let mut water = Water::default();
    for z in 0..=segments {
        for x in 0..=segments {
            let p = Vec2::new(x as f32, z as f32) / segments as f32 * n - 0.5;
            water.vertex(surface.direction(face, p) * surface.radius, p, Vec2::ZERO, 0., 1.);
        }
    }
    let row = segments as u32 + 1;
    for z in 0..segments as u32 {
        for x in 0..segments as u32 {
            let (a, b, c, d) = (z * row + x, z * row + x + 1, (z + 1) * row + x, (z + 1) * row + x + 1);
            water.indices.extend([a, b, c, b, d, c]);
        }
    }
    water.build(false)
}

/// Spawn the sea, rivers and lakes.
pub fn spawn(
    commands: &mut Commands,
    surface: &PlanetSurface,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<PlanetWaterMaterial>,
) {
    let material = materials.add(PlanetWaterMaterial {
        base: StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: PlanetWaterShader {
            tint: LinearRgba::WHITE,
            planet: Vec4::new(surface.radius, 0., 0., 0.),
            wind: Vec4::new(1., 0., 0., 0.),
        },
    });
    let mut spawn = |name: String, mesh: Mesh| {
        commands.spawn((
            Name::new(name),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            NotShadowCaster,
            Pickable::IGNORE,
        ));
    };
    let start = std::time::Instant::now();
    spawn("Rivers".into(), rivers_mesh(surface));
    for face in 0..FACES {
        if let Some(mesh) = lakes_mesh(surface, face) {
            spawn(format!("Lakes {face}"), mesh);
        }
        spawn(format!("Sea {face}"), sea_mesh(surface, face));
    }
    info!("Planet water meshed in {:.1?}", start.elapsed());
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
