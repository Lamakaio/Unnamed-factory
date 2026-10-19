//! The planet's water, drawn with `PlanetWaterMaterial` (`planet_water.wesl`, the flat map's
//! toon water on the sphere), as on the former flat map:
//! - rivers: ribbons along the water line of the flat map's river profile (pools and falls of
//!   varied slant, sloping chutes, domed steep stretches), with the flow speed and whitewater
//!   for the flow map. They fade out where they reach the sea or go under their lake (carrying
//!   on straight if needed), fade in where they leave a lake, and tributaries end on their
//!   parent's ribbon edge (a junction cap sharing its vertices), whitening the parent below;
//! - lakes: per face, outlined by marching squares where the ground rises above the water (a
//!   lake covers its extent, the lower ground connected to it and a little of the shore, not
//!   ground beyond its rim), reaching just into the river channels at its edge;
//! - springs (`worldgen::planet_springs`): pond surfaces the river fades in out of, and
//!   hillside spouts (a row up in the notch, whitewater below);
//! - the sea: a sphere at sea level, built face by face, each face with its wave field
//!   (`worldgen::planet_sea`, a texture by cell: swell crests along the travel time to the
//!   shore, bigger where they come with a strong wind).
//!
//! Lakes and the sea carry the local wind (UV0, see `PlanetSurface::wind_at`): ripples drift
//! with it, and are stronger the stronger it blows.
//!
//! Patterns use 2D coordinates in vertex colour red/green: along / across a river (so its
//! flow pattern follows it across cube faces; a junction cap uses its tributary's), the face's
//! cell coordinates elsewhere.

use std::collections::{HashMap, VecDeque};

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use half::f16;
use worldgen::{
    RiverEnd, RiverPoint,
    planet::Planet,
    planet_rivers::PlanetRiver,
    planet_sea::sea_field,
    planet_springs::{PlanetSpringKind, SPOUT_HEIGHT},
    sphere::FACES,
    surface::PlanetSurface,
};

use super::river_profile::{
    DOME_DROP, DOME_SEGMENTS, DOME_SHAPE, DOME_SLOPE, RIVER_OFFSET, RIVER_OVERLAP, fall_split,
    river_profile,
};
use crate::shaders::{PlanetWaterMaterial, PlanetWaterShader};

// Lake outlines, river fades and confluences, as on the flat map.
/// Lake surfaces reach under the shore up to where the ground is this much above them.
const SHORE_MARGIN: f32 = 0.03;

/// Contour value in river channels outside a lake: just outside, so the outline reaches well
/// into the channel (under the ribbon, which is then cut out exactly).
const RIVER_EDGE: f32 = -0.01;

/// Lake surfaces are drawn this far under their level: where a river fades into or out of a
/// lake, its ribbon (at the lake's level, plus [`RIVER_OFFSET`]) stays clearly above it.
const LAKE_SINK: f32 = 0.01;

/// Lake surfaces cover the shore up to this many samples from the water.
const SHORE_STEPS: u8 = 3;

/// Contour value of the samples a lake must not cover (low ground beyond its rim, or another
/// lake's): strongly outside, so the outline keeps close to the last sample it covers.
const OUTSIDE: f32 = -1.;

/// Rivers fade in (outlets) or out (inflows) over this distance (world units), this far inside
/// their lake (they stay visible across the shore).
const OUTLET_FADE: f32 = 1.;

const LAKE_FADE_INSIDE: f32 = 1.;

/// Rivers also start fading out where the water under them is this deep.
const OPEN_WATER_DEPTH: f32 = 0.5;

/// Rivers fade out over this distance (a constant plus this many widths) into the water they
/// flow into.
const RIVER_FADE: (f32, f32) = (1., 2.5);

/// Whitewater where a tributary pours into its parent, fading downstream over this many
/// tributary widths (plus a constant).
const CONFLUENCE_FOAM: f32 = 0.55;

const CONFLUENCE_REACH: (f32, f32) = (1.5, 3.);

/// Lake depth from which a point is under a lake.
const LAKE: f32 = 0.02;
/// Ground carved this much (by river channels) counts as a river's for the lakes.
const CARVED: f32 = 0.05;
/// A tributary's junction cap fails where its outline steps up or down more than this between
/// two points (across a fall): the tributary fades out instead.
const CAP_MAX_RISE: f32 = 0.9;
/// Largest sag of the sea's triangles below sea level (world units).
const SEA_SAG: f32 = 0.02;
/// Spacing of the rows added past a river's end while it fades.
const FADE_STEP: f32 = 0.5;
/// A junction cap looks for its parent's edge this far (world units) around the junction.
const CAP_SEARCH: f32 = 30.;
/// Rivers fade in out of their pond over this distance, inside it (opaque from its rim);
/// hillside springs foam on their first rows (as on the flat map).
const POND_FADE: f32 = 0.8;
const SPRING_FOAM: [f32; 4] = [1., 0.8, 0.55, 0.3];
/// Highest dome of a steep stretch (world units): the flat map's scale with the width, but
/// wide rivers' domes would be far too round and sink their edges deep underground.
const DOME_MAX: f32 = 0.5;

/// A mesh being built: positions, pattern coordinates (vertex colour), flow velocity or wind
/// (UV0) and (whitewater, opacity) (UV1, rivers only).
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
        // (UV0: the flow on rivers, the wind elsewhere; UV1 tells the shader it is a river.)
        let normals: Vec<[f32; 3]> =
            self.positions.iter().map(|p| Vec3::from(*p).normalize_or_zero().to_array()).collect();
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.coords)
            .with_inserted_indices(Indices::U32(self.indices));
        mesh = mesh.with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.flow);
        if rivers {
            mesh = mesh.with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, self.flow_b);
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
        rows[k].dome = rows[k].half.min(DOME_MAX) * smoothstep(DOME_SLOPE.0, DOME_SLOPE.1, slope) * ends;
    }
    rows
}

/// Fade a river out from row `start` on over `length`, carrying it on straight past its end
/// if needed, and drop the rows past the fade (as the flat map did).
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
    let in_end_water = |row: &Row| match river.end {
        RiverEnd::Lake(_) => in_lake(row),
        RiverEnd::Sea => {
            let at_sea = |p: Vec3| surface.ocean_at(p.normalize()) > 0.5;
            let bank = |side: f32| ground_at(surface, row.edge(side).normalize()) <= 0.02 && at_sea(row.edge(side));
            (bank(-1.) && bank(1.))
                || ground_at(surface, row.center.normalize()) < -OPEN_WATER_DEPTH && at_sea(row.center)
        }
        RiverEnd::Junction(_) => false,
    };
    // The water it ends in: the last stretch under it (searched back from the end: on its way
    // a river may brush other water, or the ragged shore of the lake it left).
    let tail = rows[leave..].iter().rev().take_while(|row| in_end_water(row)).count();
    let reached = (tail > 0).then(|| rows.len() - leave - tail);
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

/// The end of a tributary on its parent's edge (as the flat map did, in the plane
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
        match surface.springs().iter().find(|spring| spring.river == index).map(|s| &s.kind) {
            // Out of a pond: fading in from its rim.
            Some(PlanetSpringKind::Pond { .. }) => {
                let first = rows[0];
                rows.insert(
                    0,
                    Row {
                        center: first.center - first.forward * POND_FADE,
                        along: first.along - POND_FADE,
                        fade: 0.,
                        dome: 0.,
                        ..first
                    },
                );
            }
            // Out of the hillside: a spout from the notch, foaming.
            Some(PlanetSpringKind::Hillside) => {
                for (row, foam) in rows.iter_mut().zip(SPRING_FOAM) {
                    row.foam = row.foam.max(foam);
                }
                let first = rows[0];
                rows.insert(
                    0,
                    Row {
                        center: first.center - first.forward * 0.4 + first.center.normalize() * SPOUT_HEIGHT,
                        half: first.half * 0.45,
                        along: first.along - 0.4,
                        speed: 2.,
                        foam: 1.,
                        dome: 0.,
                        ..first
                    },
                );
            }
            None => {}
        }
        // A side channel (a river splitting on flat land) leaves its river on its ribbon edge
        // too: the cap of its rows taken backwards.
        if let Some(main) = river.branch_of.and_then(|main| all_rows.get(main)).filter(|r| r.len() >= 2) {
            let backwards: Vec<Row> = rows.iter().rev().copied().collect();
            let up = backwards[backwards.len() - 1].center.normalize();
            let (east, north) = up.any_orthonormal_pair();
            let project = |p: Vec3| Vec2::new(p.dot(east), p.dot(north));
            let cap = junction_cap(&backwards, &parent_edge(main, &backwards, true), project)
                .or_else(|| junction_cap(&backwards, &parent_edge(main, &backwards, false), project));
            if let Some((full_rows, outline)) = cap {
                fan(&mut water, &backwards[full_rows - 1], &outline);
                rows.drain(..rows.len() - full_rows);
            }
            if rows.len() < 2 {
                continue;
            }
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
        // (Not across a fall: the outline would fold over itself.)
        let cap = cap.filter(|(_, outline)| {
            (0..outline.len()).all(|k| {
                let next = &outline[(k + 1) % outline.len()];
                (outline[k].position.length() - next.position.length()).abs() < CAP_MAX_RISE
            })
        });
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
/// sunk under the banks, the whole lowered a little: see `river_profile`).
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

/// Lake surfaces of one face, as on the flat map: on the cells'
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
            // (The ground before the channels are carved: carved banks round an outlet or an
            // inlet are lower than the lake, which would spill over them.)
            // The ground as drawn; the channels' carving (a river's bed and banks: lower than the
            // lake round an outlet or an inlet) stops the lake like a river channel does.
            let direction = surface.direction(face, p);
            let ground = surface.height(face, p);
            let carved = surface.uncarved_height(direction) - ground > CARVED;
            let sample = if depth >= LAKE {
                Sample::Lake(surface.heights.bilinear(face, p) + depth)
            } else if depth > 0. {
                Sample::Low
            } else if carved || surface.wetness(face, p) > 0. {
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
                water.vertex(surface.direction(face, p) * height, p, wind(surface, face, p), 0., 1.);
            }
            for k in 1..polygon.len() as u32 - 1 {
                water.indices.extend([first, first + k, first + k + 1]);
            }
        }
    }
    (!water.indices.is_empty()).then(|| water.build(false))
}

/// The springs' ponds: a fan from each pond's centre to its outline, just over its level.
fn ponds_mesh(surface: &PlanetSurface) -> Option<Mesh> {
    let mut water = Water::default();
    for spring in surface.springs() {
        let (PlanetSpringKind::Pond { center, .. }, Some(outline)) = (&spring.kind, spring.outline())
        else {
            continue;
        };
        let height = surface.radius + spring.level + RIVER_OFFSET;
        let mut vertex = |local: Vec2| {
            let direction = spring.direction_of(local, surface.radius);
            let (face, p) = surface.face_coords(direction);
            water.vertex(direction * height, p, wind(surface, face, p), 0., 1.)
        };
        let middle = vertex(*center);
        let ring: Vec<u32> = outline.iter().map(|&q| vertex(q)).collect();
        for k in 0..ring.len() {
            water.indices.extend([middle, ring[k], ring[(k + 1) % ring.len()]]);
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
            water.vertex(surface.direction(face, p) * surface.radius, p, wind(surface, face, p), 0., 1.);
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

/// The local wind in a face's cell coordinates (direction × speed in m/s).
fn wind(surface: &PlanetSurface, face: usize, p: Vec2) -> Vec2 {
    let (wind, strength) = surface.wind_at(face, p);
    let wind = wind.normalize_or_zero() * strength;
    // World units per cell along each face axis.
    let along = |axis: Vec2| {
        (surface.direction(face, p + axis) - surface.direction(face, p - axis)) * surface.radius / 2.
    };
    let (ex, ey) = (along(Vec2::X), along(Vec2::Y));
    Vec2::new(wind.dot(ex) / ex.length_squared(), wind.dot(ey) / ey.length_squared())
}

/// The sea wave field of each face as texture data (`Rgba16Float`, a texel per cell).
pub fn sea_textures(planet: &Planet, surface: &PlanetSurface) -> Vec<Vec<u8>> {
    let field = sea_field(planet, surface);
    let per_face = surface.n * surface.n;
    field
        .chunks(per_face)
        .map(|face| {
            let mut data = Vec::with_capacity(per_face * 8);
            for texel in face {
                for value in texel {
                    data.extend(f16::from_f32(*value).to_le_bytes());
                }
            }
            data
        })
        .collect()
}

/// Spawn the sea (with each face's wave field), rivers and lakes.
pub fn spawn(
    commands: &mut Commands,
    surface: &PlanetSurface,
    sea: Vec<Vec<u8>>,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<PlanetWaterMaterial>,
) {
    let n = surface.n as u32;
    let mut material = |sea: Option<Handle<Image>>| {
        materials.add(PlanetWaterMaterial {
            base: StandardMaterial {
                alpha_mode: AlphaMode::Blend,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
            extension: PlanetWaterShader {
                tint: LinearRgba::WHITE,
                planet: Vec4::new(surface.radius, n as f32, sea.is_some() as u32 as f32, 0.),
                sea,
                wind: Vec4::new(1., 0., 0., 0.),
            },
        })
    };
    let still = material(None);
    let sea_materials: Vec<_> = sea
        .into_iter()
        .map(|data| {
            let mut image = Image::new(
                Extent3d { width: n, height: n, depth_or_array_layers: 1 },
                TextureDimension::D2,
                data,
                TextureFormat::Rgba16Float,
                RenderAssetUsages::RENDER_WORLD,
            );
            image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::ClampToEdge,
                address_mode_v: ImageAddressMode::ClampToEdge,
                ..ImageSamplerDescriptor::linear()
            });
            material(Some(images.add(image)))
        })
        .collect();
    let mut spawn = |name: String, mesh: Mesh, material: &Handle<PlanetWaterMaterial>| {
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
    spawn("Rivers".into(), rivers_mesh(surface), &still);
    let ponds = surface
        .springs()
        .iter()
        .filter(|spring| matches!(spring.kind, PlanetSpringKind::Pond { .. }))
        .count();
    info!("{} springs: {ponds} ponds, {} hillside", surface.springs().len(), surface.springs().len() - ponds);
    info!(
        "{} rivers, {} side channels (splitting on flat land)",
        surface.rivers.len(),
        surface.rivers.iter().filter(|river| river.branch_of.is_some()).count()
    );
    if let Some(mesh) = ponds_mesh(surface) {
        spawn("Ponds".into(), mesh, &still);
    }
    for face in 0..FACES {
        if let Some(mesh) = lakes_mesh(surface, face) {
            spawn(format!("Lakes {face}"), mesh, &still);
        }
        spawn(format!("Sea {face}"), sea_mesh(surface, face), sea_materials.get(face).unwrap_or(&still));
    }
    info!("Planet water meshed in {:.1?}", start.elapsed());
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// The part of a grid cell where `values` (at its corners, in order round it) are positive,
/// cut along their linear interpolation (marching squares): one polygon, or two for a saddle
/// whose center is outside.
fn contour_cell(corners: [Vec2; 4], values: [f32; 4]) -> Vec<Vec<Vec2>> {
    let cut = |a: usize, b: usize| {
        let t = values[a] / (values[a] - values[b]);
        corners[a].lerp(corners[b], t)
    };
    let inside = values.map(|v| v > 0.);
    let saddle = inside[0] == inside[2] && inside[1] == inside[3] && inside[0] != inside[1];
    if saddle && values.iter().sum::<f32>() <= 0. {
        // Two separate corners.
        return (0..4)
            .filter(|&k| inside[k])
            .map(|k| vec![cut(k, (k + 3) % 4), corners[k], cut(k, (k + 1) % 4)])
            .collect();
    }
    let mut polygon = Vec::with_capacity(6);
    for k in 0..4 {
        let next = (k + 1) % 4;
        if inside[k] {
            polygon.push(corners[k]);
        }
        if inside[k] != inside[next] {
            polygon.push(cut(k, next));
        }
    }
    vec![polygon]
}

/// The part of a convex polygon where `sign * (b - a).perp_dot(p - a) > 0`, on one side of the
/// line through `a` and `b` (Sutherland-Hodgman).
fn clip_half_plane(polygon: &[Vec2], a: Vec2, b: Vec2, sign: f32) -> Vec<Vec2> {
    let side = |p: Vec2| sign * (b - a).perp_dot(p - a);
    let mut out = Vec::with_capacity(polygon.len() + 2);
    for k in 0..polygon.len() {
        let (p, q) = (polygon[k], polygon[(k + 1) % polygon.len()]);
        let (sp, sq) = (side(p), side(q));
        if sp > 0. {
            out.push(p);
        }
        if (sp > 0.) != (sq > 0.) {
            out.push(p.lerp(q, sp / (sp - sq)));
        }
    }
    out
}
