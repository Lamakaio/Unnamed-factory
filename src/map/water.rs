//! Water surfaces: the sea at height 0, lakes at their level, and river ribbons following the
//! water line of each river. They are translucent and ignored by picking, so clicks and
//! placement reach the terrain below.
//!
//! Steep river stretches run in steps (pools between small falls, as in mountain streams), and
//! the river mesh carries its flow (travel time, speed, whitewater) for the shader. The sea uses
//! a baked wave field (see `sea`).

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use worldgen::{Continent, LAKE_MIN_DEPTH, River, RiverEnd, RiverPoint};

use super::{
    Map,
    heights::TerrainHeights,
    springs::{SPOUT_HEIGHT, Spring, SpringKind},
};
use crate::{
    AppState,
    shaders::{WaterMaterial, WaterShader},
};

/// Size of the sea plane, in world units.
const SEA_SIZE: f32 = 8000.;
/// River ribbons extend this far past the channel edges, under the banks.
pub(crate) const RIVER_OVERLAP: f32 = 0.25;
/// Rivers float this much above their water line, to avoid fighting with lakes and the sea.
pub(crate) const RIVER_OFFSET: f32 = 0.02;
/// River ribbon directions are measured over this many points on each side.
const RIVER_TANGENT_SPAN: usize = 2;
/// Rivers steeper than this (water line slope) run in steps: pools between small falls.
const CASCADE_SLOPE: f32 = 0.18;
/// Step height of cascades, relative to the channel depth (the pools stay above the bed),
/// varied by up to ±45% per step.
const CASCADE_STEP: f32 = 0.45;
/// Pools keep this fraction of the natural slope (from their lowest point, so the water never
/// stands above the natural water line, which the banks hold), varied per pool; the falls take
/// the rest.
const POOL_TILT: (f32, f32) = (0.15, 0.6);
/// Share of cascade stretches left as sloping chutes (1 to 3 points) between the pools.
const CHUTE_SHARE: f32 = 0.35;
/// Horizontal extent of a fall, relative to its drop (from nearly vertical to a slanted
/// chute, varied per fall).
const FALL_RUN: (f32, f32) = (0.2, 1.4);
/// Steep stretches (water line slope from the first to the second value) get volume: an oval
/// cross-section (see [`Row::dome`]), the water bulging up in the middle and its edges sunk
/// into the banks, which the ribbon cannot follow down steep slopes.
pub(crate) const DOME_SLOPE: (f32, f32) = (0.2, 0.6);
/// Height of the oval's top above the water line, and its edges' depth below it, relative to
/// the ribbon's half-width.
pub(crate) const DOME_SHAPE: (f32, f32) = (0.3, 1.);
/// Domed stretches are also lowered by this much (relative to the half-width, like
/// [`DOME_SHAPE`]): on the steepest slopes the terrain mesh lags under the carved channel.
pub(crate) const DOME_DROP: f32 = 0.35;
/// Segments across a domed cross-section.
pub(crate) const DOME_SEGMENTS: usize = 8;

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_sea).add_systems(
            OnEnter(AppState::InGame),
            (spawn_lakes_and_rivers, bind_sea_field),
        );
    }
}

/// Shared material of all water surfaces.
#[derive(Resource)]
struct Water(Handle<WaterMaterial>);

fn spawn_sea(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
) {
    let material = materials.add(WaterMaterial {
        base: StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            // River ribbons wind either way depending on the flow direction.
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: WaterShader {
            tint: LinearRgba::WHITE,
            sea: None,
            sea_map: Vec4::ZERO,
            wind: Vec4::new(1., 0., 0., 0.),
        },
    });
    commands.insert_resource(Water(material.clone()));
    commands.spawn((
        Name::new("Sea"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(SEA_SIZE, SEA_SIZE))),
        MeshMaterial3d(material),
        Transform::default(),
        NotShadowCaster,
        Pickable::IGNORE,
    ));
}

/// Give the water material the sea wave field and the prevailing wind.
fn bind_sea_field(map: Res<Map>, water: Res<Water>, mut materials: ResMut<Assets<WaterMaterial>>) {
    if let Some(mut material) = materials.get_mut(&water.0) {
        let wind = Vec2::from_angle(map.continent.wind_angle);
        material.extension.sea = Some(map.textures.sea.clone());
        material.extension.sea_map = map.textures.sea_map;
        material.extension.wind = Vec4::new(wind.x, wind.y, 0., 0.);
    }
}

fn spawn_lakes_and_rivers(
    mut commands: Commands,
    map: Res<Map>,
    material: Res<Water>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let continent = &map.continent;
    let springs = map.heights.springs();
    let (rivers, opaque) = rivers_mesh(continent, springs);
    for (name, mesh) in [
        ("Lakes", lakes_mesh(continent, &map.heights, &opaque)),
        ("Ponds", ponds_mesh(springs)),
        ("Rivers", rivers),
    ] {
        commands.spawn((
            Name::new(name),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.0.clone()),
            Transform::default(),
            NotShadowCaster,
            Pickable::IGNORE,
        ));
    }
}

#[derive(Default)]
struct SurfaceBuilder {
    positions: Vec<[f32; 3]>,
    /// Rivers only: flow velocity (world x, z) and (whitewater, opacity), see [`Row`].
    flow: Vec<[f32; 2]>,
    flow_b: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl SurfaceBuilder {
    /// A flat convex polygon (a fan), wound like the ribbons' triangles.
    fn polygon(&mut self, vertices: &[Vec3]) {
        if vertices.len() < 3 {
            return;
        }
        // (Clockwise in xz: negative area.)
        let area: f32 = (0..vertices.len())
            .map(|k| vertices[k].xz().perp_dot(vertices[(k + 1) % vertices.len()].xz()))
            .sum();
        let first = self.positions.len() as u32;
        self.positions.extend(vertices.iter().map(|v| v.to_array()));
        let count = vertices.len() as u32;
        for k in 1..count - 1 {
            if area < 0. {
                self.indices.extend([first, first + k, first + k + 1]);
            } else {
                self.indices.extend([first, first + k + 1, first + k]);
            }
        }
    }

    /// A strip between two domed river rows: across each, the top of an oval (see
    /// [`Row::dome`]).
    fn dome_strip(&mut self, a: &Row, b: &Row) {
        let first = self.positions.len() as u32;
        for row in [a, b] {
            for k in 0..=DOME_SEGMENTS {
                let angle = std::f32::consts::PI * k as f32 / DOME_SEGMENTS as f32;
                let t = (1. - angle.cos()) / 2.;
                let rise =
                    row.dome * ((DOME_SHAPE.0 + DOME_SHAPE.1) * angle.sin() - DOME_SHAPE.1 - DOME_DROP);
                let position = row.left.lerp(row.right, t) + Vec3::Y * rise;
                self.positions.push(position.to_array());
                self.flow.push(row.left_flow.lerp(row.right_flow, t).to_array());
                self.flow_b.push([row.foam, row.fade]);
            }
        }
        let columns = DOME_SEGMENTS as u32 + 1;
        for k in 0..DOME_SEGMENTS as u32 {
            let (a0, a1) = (first + k, first + k + 1);
            let (b0, b1) = (a0 + columns, a1 + columns);
            self.indices.extend([a0, b0, a1, a1, b0, b1]);
        }
    }

    /// A ribbon quad between two river rows, split along the diagonal giving the better
    /// triangles: at a pinched bend, the usual diagonal can fall outside the quad and flip a
    /// triangle.
    fn ribbon_quad(&mut self, a: &Row, b: &Row) {
        // Fading out: split at the center line, which keeps the opacity while the sides fade
        // to nothing (no hard edge where there are no banks to hide it).
        if a.fade < 1. || b.fade < 1. {
            let half = |row: &Row, left: bool| {
                let center = row.center();
                let flow = row.flow();
                let (edge, edge_flow) = if left {
                    (row.left, row.left_flow)
                } else {
                    (row.right, row.right_flow)
                };
                let edge_fade = (row.fade * 2. - 1.).max(0.);
                [(edge, edge_flow, edge_fade), (center, flow, row.fade)]
            };
            for left in [true, false] {
                let ([ea, ca], [eb, cb]) = (half(a, left), half(b, left));
                let foams = [a.foam, a.foam, b.foam, b.foam];
                let vertices = [ea, ca, eb, cb].into_iter().zip(foams).map(|((position, flow, fade), foam)| {
                    let index = self.vertex(position, flow, foam);
                    self.flow_b[index as usize][1] = fade;
                    index
                }).collect::<Vec<u32>>();
                self.indices.extend([
                    vertices[0],
                    vertices[2],
                    vertices[1],
                    vertices[1],
                    vertices[2],
                    vertices[3],
                ]);
            }
            return;
        }
        if a.dome > 0. || b.dome > 0. {
            self.dome_strip(a, b);
            return;
        }
        let corners = [a.left, a.right, b.left, b.right];
        let area = |i: usize, j: usize, k: usize| {
            (corners[j] - corners[i])
                .xz()
                .perp_dot((corners[k] - corners[i]).xz())
        };
        let usual = area(0, 2, 1).min(area(1, 2, 3));
        let other = area(0, 3, 1).min(area(0, 2, 3));
        let first = self.positions.len() as u32;
        self.positions
            .extend(corners.map(|corner| corner.to_array()));
        self.flow.extend(
            [a.left_flow, a.right_flow, b.left_flow, b.right_flow].map(|flow| flow.to_array()),
        );
        self.flow_b.extend([
            [a.foam, a.fade],
            [a.foam, a.fade],
            [b.foam, b.fade],
            [b.foam, b.fade],
        ]);
        if other <= usual {
            self.indices
                .extend([first, first + 2, first + 1, first + 1, first + 2, first + 3]);
        } else {
            self.indices
                .extend([first, first + 3, first + 1, first, first + 2, first + 3]);
        }
    }

    /// A river vertex (position, flow velocity, whitewater); returns its index.
    fn vertex(&mut self, position: Vec3, flow: Vec2, foam: f32) -> u32 {
        self.positions.push(position.to_array());
        self.flow.push(flow.to_array());
        self.flow_b.push([foam, 1.]);
        self.positions.len() as u32 - 1
    }

    fn build(self) -> Mesh {
        let normals = vec![[0., 1., 0.]; self.positions.len()];
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(self.indices));
        if !self.flow.is_empty() {
            mesh = mesh
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.flow)
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, self.flow_b);
        }
        mesh
    }
}

/// Lake surfaces reach under the shore up to where the ground is this much above them.
pub(crate) const SHORE_MARGIN: f32 = 0.03;
/// Contour value in river channels outside a lake: just outside, so the outline reaches well
/// into the channel (under the ribbon, which is then cut out exactly).
pub(crate) const RIVER_EDGE: f32 = -0.01;
/// Cell size of the lookup grid of river ribbon triangles cut out of the lakes.
const CLIP_CELL: f32 = 2.;
/// Lake surfaces are drawn this far under their level: where a river fades into or out of a
/// lake, its ribbon (at the lake's level, plus [`RIVER_OFFSET`]) stays clearly above it.
pub(crate) const LAKE_SINK: f32 = 0.01;
/// Lake surfaces cover the shore up to this many samples from the water.
pub(crate) const SHORE_STEPS: u8 = 3;
/// Contour value of the samples a lake must not cover (low ground beyond its rim, or another
/// lake's): strongly outside, so the outline keeps close to the last sample it covers.
pub(crate) const OUTSIDE: f32 = -1.;

/// The lake surfaces, outlined by the contour where the ground (as drawn, with the river
/// channels) rises above them (marching squares on the lake mask's samples, interpolated: the
/// outline follows the shore, not the grid). A lake covers its extent
/// (`Continent::lake_mask`, the same one rivers use) and the ground around it below its level
/// (plus [`SHORE_MARGIN`], under the shore); not lower ground beyond, which its rim holds back
/// (where the outline crosses open ground, it hugs the extent).
fn lakes_mesh(continent: &Continent, heights: &TerrainHeights, opaque: &[[Vec2; 3]]) -> Mesh {
    let mask = &continent.lake_mask;
    // The opaque river ribbon triangles, by grid cell.
    let cells_of = |min: Vec2, max: Vec2| {
        let (min, max) = ((min / CLIP_CELL).floor().as_ivec2(), (max / CLIP_CELL).floor().as_ivec2());
        (min.y..=max.y).flat_map(move |z| (min.x..=max.x).map(move |x| (x, z)))
    };
    let mut ribbons: std::collections::HashMap<(i32, i32), Vec<usize>> = default();
    for (index, triangle) in opaque.iter().enumerate() {
        let (min, max) = bounds_of(triangle);
        for cell in cells_of(min, max) {
            ribbons.entry(cell).or_default().push(index);
        }
    }
    let n = mask.size;
    let at = |x: usize, z: usize| (Vec2::new(x as f32, z as f32) - (n / 2) as f32) * mask.cell;
    // Each lake's samples, and their bounds.
    let mut bounds = vec![(usize::MAX, usize::MAX, 0usize, 0usize); continent.lakes.len()];
    for (i, &lake) in mask.lakes.iter().enumerate() {
        if lake == u16::MAX {
            continue;
        }
        let b = &mut bounds[lake as usize];
        let (x, z) = (i % n, i / n);
        *b = (b.0.min(x), b.1.min(z), b.2.max(x), b.3.max(z));
    }
    let mut builder = SurfaceBuilder::default();
    for (index, lake) in continent.lakes.iter().enumerate() {
        let (x0, z0, x1, z1) = bounds[index];
        if x0 == usize::MAX {
            continue;
        }
        // (A margin of samples around: the shore lies beyond the extent.)
        let r = 12;
        let (x0, z0) = (x0.saturating_sub(r), z0.saturating_sub(r));
        let (x1, z1) = ((x1 + r).min(n - 1), (z1 + r).min(n - 1));
        let (w, h) = (x1 - x0 + 1, z1 - z0 + 1);
        let level = lake.level;
        // What each sample is to this lake.
        #[derive(Clone, Copy, PartialEq)]
        enum Sample {
            /// In its extent.
            Lake,
            /// Lower ground (left out of the extent as too thin, or beyond the rim).
            Low,
            /// Ground around its level: the shore.
            Shore,
            /// Never covered: another lake's.
            Never,
            /// In a river's channel outside the lake (the ribbon is the water there, at its
            /// own level): the lake reaches into it (under the ribbon, which then cuts it out
            /// exactly), but no further.
            River,
        }
        let ground: Vec<f32> = (0..w * h)
            .map(|k| heights.height(at(x0 + k % w, z0 + k / w)))
            .collect();
        let kind: Vec<Sample> = (0..w * h)
            .map(|k| {
                let (x, z) = (x0 + k % w, z0 + k / w);
                let owner = mask.lakes[z * n + x];
                if owner as usize == index {
                    Sample::Lake
                } else if owner != u16::MAX {
                    Sample::Never
                } else if heights.near_river(at(x, z), 1e-3, level) > 0. {
                    Sample::River
                } else if ground[k] < level - LAKE_MIN_DEPTH {
                    Sample::Low
                } else {
                    Sample::Shore
                }
            })
            .collect();
        // Covered: grown from the extent over the lower ground connected to it (never across
        // the shore: that is the rim), and up to `SHORE_STEPS` samples onto the shore.
        let mut steps = vec![u8::MAX; w * h];
        let mut queue = std::collections::VecDeque::new();
        for k in (0..w * h).filter(|&k| kind[k] == Sample::Lake) {
            steps[k] = 0;
            queue.push_back(k);
        }
        while let Some(k) = queue.pop_front() {
            let (x, z) = (k % w, k / w);
            for (nx, nz) in [(x.wrapping_sub(1), z), (x + 1, z), (x, z.wrapping_sub(1)), (x, z + 1)] {
                if nx >= w || nz >= h {
                    continue;
                }
                let m = nz * w + nx;
                let step = match (kind[k], kind[m]) {
                    (Sample::Lake | Sample::Low, Sample::Low) => 0,
                    (_, Sample::Shore) if steps[k] < SHORE_STEPS => steps[k] + 1,
                    _ => continue,
                };
                if step < steps[m] {
                    steps[m] = step;
                    queue.push_back(m);
                }
            }
        }
        // Contour field: positive under the lake.
        let field: Vec<f32> = (0..w * h)
            .map(|k| {
                let water = level + SHORE_MARGIN - ground[k];
                match kind[k] {
                    Sample::River => RIVER_EDGE,
                    _ if steps[k] == u8::MAX => OUTSIDE,
                    Sample::Lake | Sample::Low => water.max(1e-3),
                    _ => water,
                }
            })
            .collect();
        for z in 0..h - 1 {
            for x in 0..w - 1 {
                let corners = [(x, z), (x + 1, z), (x + 1, z + 1), (x, z + 1)];
                let values = corners.map(|(x, z)| field[z * w + x]);
                if values.iter().all(|&v| v <= 0.) {
                    continue;
                }
                let position = corners.map(|(x, z)| at(x + x0, z + z0));
                for polygon in contour_cell(position, values) {
                    // Less the opaque river ribbons over it.
                    let (min, max) = bounds_of(&polygon);
                    let mut near: Vec<usize> = cells_of(min, max)
                        .filter_map(|cell| ribbons.get(&cell))
                        .flatten()
                        .copied()
                        .collect();
                    near.sort_unstable();
                    near.dedup();
                    let mut pieces = vec![polygon];
                    for &triangle in &near {
                        pieces = pieces
                            .into_iter()
                            .flat_map(|piece| subtract_triangle(piece, &opaque[triangle]))
                            .collect();
                    }
                    for piece in pieces {
                        let piece: Vec<Vec3> =
                            piece.iter().map(|p| p.extend(level - LAKE_SINK).xzy()).collect();
                        builder.polygon(&piece);
                    }
                }
            }
        }
    }
    builder.build()
}

/// The part of a grid cell where `values` (at its corners, in order round it) are positive,
/// cut along their linear interpolation (marching squares): one polygon, or two for a saddle
/// whose center is outside.
pub(crate) fn contour_cell(corners: [Vec2; 4], values: [f32; 4]) -> Vec<Vec<Vec2>> {
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

/// The bounds (min, max) of a polygon's vertices.
fn bounds_of(polygon: &[Vec2]) -> (Vec2, Vec2) {
    polygon.iter().fold((Vec2::MAX, Vec2::MIN), |(min, max), &p| (min.min(p), max.max(p)))
}

/// The part of a convex polygon where `sign * (b - a).perp_dot(p - a) > 0`, on one side of the
/// line through `a` and `b` (Sutherland-Hodgman).
pub(crate) fn clip_half_plane(polygon: &[Vec2], a: Vec2, b: Vec2, sign: f32) -> Vec<Vec2> {
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

/// A convex polygon less a triangle: convex pieces (none if the triangle covers it).
fn subtract_triangle(polygon: Vec<Vec2>, triangle: &[Vec2; 3]) -> Vec<Vec<Vec2>> {
    let [a, b, c] = *triangle;
    let orientation = (b - a).perp_dot(c - a);
    if orientation.abs() < 1e-8 {
        return vec![polygon];
    }
    let (min, max) = bounds_of(&polygon);
    let (t_min, t_max) = bounds_of(triangle);
    if min.x >= t_max.x || min.y >= t_max.y || max.x <= t_min.x || max.y <= t_min.y {
        return vec![polygon];
    }
    // Outside each edge in turn, of what is inside the previous ones.
    let sign = orientation.signum();
    let mut pieces = Vec::new();
    let mut rest = polygon;
    for (p, q) in [(a, b), (b, c), (c, a)] {
        let outside = clip_half_plane(&rest, p, q, -sign);
        if outside.len() >= 3 {
            pieces.push(outside);
        }
        rest = clip_half_plane(&rest, p, q, sign);
        if rest.len() < 3 {
            break;
        }
    }
    pieces
}

/// Rivers fade in (outlets) or out (inflows) over this distance (world units), this far inside
/// their lake (they stay visible across the shore).
pub(crate) const OUTLET_FADE: f32 = 1.;
pub(crate) const LAKE_FADE_INSIDE: f32 = 1.;

/// Rivers also start fading out where the water under them is this deep.
pub(crate) const OPEN_WATER_DEPTH: f32 = 0.5;

/// Rivers fade out over this distance (a constant plus this many widths) into the water they
/// flow into.
pub(crate) const RIVER_FADE: (f32, f32) = (1., 2.5);

/// Fade a river out from row `start` on, extending it straight past its end if needed, and
/// drop the rows past the fade.
fn fade_out(rows: &mut Vec<Row>, start: usize, length: Option<f32>) {
    let width = (rows[start].left - rows[start].right).length();
    let length = length.unwrap_or(RIVER_FADE.0 + RIVER_FADE.1 * width);
    let step = 0.5;
    // Extend straight past the end while still visible.
    let mut distance_left = length;
    for pair in rows[start..].windows(2) {
        distance_left -= pair[0].center().xz().distance(pair[1].center().xz());
    }
    if distance_left > 0. && rows.len() >= 2 {
        let last = rows[rows.len() - 1].clone();
        let before = &rows[rows.len() - 2];
        let direction = (last.center() - before.center())
            .xz()
            .normalize_or_zero()
            .extend(0.)
            .xzy();
        let extra = (distance_left / step).ceil() as usize;
        for k in 1..=extra {
            let offset = direction * step * k as f32;
            rows.push(Row {
                left: last.left + offset,
                right: last.right + offset,
                ..last.clone()
            });
        }
    }
    let mut distance = 0.;
    let mut keep = rows.len();
    for k in start..rows.len() {
        if k > start {
            distance += rows[k].center().xz().distance(rows[k - 1].center().xz());
        }
        let fade = 1. - distance / length;
        rows[k].fade = fade.max(0.);
        if fade <= 0. {
            keep = k + 1;
            break;
        }
    }
    rows.truncate(keep);
}

/// The spring ponds: a fan over each outline, at the water line.
fn ponds_mesh(springs: &[Spring]) -> Mesh {
    let mut builder = SurfaceBuilder::default();
    for spring in springs {
        let SpringKind::Pond { center, outline } = &spring.kind else {
            continue;
        };
        let y = spring.position.y + RIVER_OFFSET;
        let first = builder.positions.len() as u32;
        builder.positions.push(center.extend(y).xzy().to_array());
        builder
            .positions
            .extend(outline.iter().map(|p| p.extend(y).xzy().to_array()));
        let n = outline.len() as u32;
        for k in 0..n {
            builder
                .indices
                .extend([first, first + 1 + k, first + 1 + (k + 1) % n]);
        }
    }
    builder.build()
}

/// Whitewater at a hillside spring, on its first rows.
const SPRING_FOAM: [f32; 4] = [1., 0.8, 0.55, 0.3];

/// A cross-section of a river ribbon.
#[derive(Clone)]
struct Row {
    left: Vec3,
    right: Vec3,
    /// Flow velocity at each side (world x, z, units per second): the shader pushes its
    /// world-space pattern along it (a flow map), so where rivers meet and their velocities
    /// are blended, the pattern bends from one into the other.
    left_flow: Vec2,
    right_flow: Vec2,
    /// Whitewater, 0..1 (rapids, below falls and confluences).
    foam: f32,
    /// Opacity: rivers fade out into the sea or lake they flow into.
    fade: f32,
    /// Steep stretches have volume: the cross-section is the top of an oval this high above
    /// the water line in the middle, sinking at the edges under the banks (see
    /// [`DOME_SHAPE`]); 0 for a flat ribbon.
    dome: f32,
}

impl Row {
    fn center(&self) -> Vec3 {
        (self.left + self.right) / 2.
    }

    fn flow(&self) -> Vec2 {
        (self.left_flow + self.right_flow) / 2.
    }
}

/// Turn a flow velocity towards another direction (keeping its speed), by `amount` 0..1.
fn steer(flow: Vec2, towards: Vec2, amount: f32) -> Vec2 {
    let direction = flow
        .normalize_or_zero()
        .lerp(towards.normalize_or_zero(), amount)
        .normalize_or_zero();
    direction * flow.length()
}

/// Water line and flow of a river, per point.
pub(crate) struct ProfilePoint {
    pub(crate) level: f32,
    pub(crate) speed: f32,
    pub(crate) foam: f32,
    /// Height of the fall right after this point (0 if none), and its horizontal extent
    /// relative to the drop.
    pub(crate) drop: f32,
    pub(crate) fall_run: f32,
}

/// A deterministic pseudo-random value in 0..1 for a river point (and a salt).
fn jitter(point: &RiverPoint, salt: u32) -> f32 {
    let mut h = point.position.x.to_bits()
        ^ point.position.z.to_bits().rotate_left(16)
        ^ salt.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 15)).wrapping_mul(0x85eb_ca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

/// The water line of a river and its flow. Steep stretches run in irregular steps: pools (see
/// [`POOL_TILT`]) between falls of varied height and slant, and sloping chutes.
pub(crate) fn river_profile(points: &[RiverPoint]) -> Vec<ProfilePoint> {
    let n = points.len();
    // Slope of the natural water line, smoothed over a few points.
    let slope: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(2), (i + 2).min(n - 1));
            let run: f32 = points[a..=b]
                .windows(2)
                .map(|w| (w[1].position - w[0].position).xz().length())
                .sum();
            ((points[a].position.y - points[b].position.y) / run.max(1e-3)).max(0.)
        })
        .collect();
    let mut levels: Vec<f32> = points.iter().map(|p| p.position.y).collect();
    let mut pool_ends = Vec::new();
    let mut i = 0;
    while i < n {
        if slope[i] < CASCADE_SLOPE {
            i += 1;
            continue;
        }
        if jitter(&points[i], 1) < CHUTE_SHARE {
            i += 1 + (jitter(&points[i], 2) * 3.) as usize;
            continue;
        }
        let step = CASCADE_STEP * points[i].depth * (0.55 + 0.9 * jitter(&points[i], 3));
        let tilt = POOL_TILT.0 + (POOL_TILT.1 - POOL_TILT.0) * jitter(&points[i], 4);
        let top = points[i].position.y;
        let mut end = i;
        while end + 1 < n
            && slope[end + 1] >= CASCADE_SLOPE
            && top - points[end + 1].position.y <= step
        {
            end += 1;
        }
        // Too steep for pools (a single point): a continuous cascade, left as a ramp.
        if end == i {
            i += 1;
            continue;
        }
        let bottom = points[end].position.y;
        for (level, point) in levels[i..=end].iter_mut().zip(&points[i..=end]) {
            *level = bottom + tilt * (point.position.y - bottom);
        }
        if end + 1 < n {
            pool_ends.push(end);
        }
        i = end + 1;
    }
    let mut drops = vec![0.; n];
    for end in pool_ends {
        drops[end] = (levels[end] - levels[end + 1]).max(0.);
    }
    // Speed from the slope (narrow streams faster); whitewater on rapids and below falls.
    let mut since_fall = f32::MAX;
    let mut plunge = 0f32;
    (0..n)
        .map(|i| {
            if i > 0 {
                since_fall += (points[i].position - points[i - 1].position).xz().length();
                if drops[i - 1] > 0. {
                    since_fall = 0.;
                    plunge = (drops[i - 1] / 0.25).min(0.9);
                }
            }
            let narrow = (1.3 - 0.1 * points[i].width).clamp(0.6, 1.2);
            let rapids = smoothstep(0.1, 0.6, slope[i]) * 0.35;
            let below_fall = plunge * (-since_fall / (0.12 + points[i].width * 0.2)).exp();
            ProfilePoint {
                level: levels[i],
                speed: (0.35 + 2.5 * slope[i].sqrt()).min(2.5) * narrow,
                foam: rapids.max(below_fall),
                drop: drops[i],
                fall_run: FALL_RUN.0 + (FALL_RUN.1 - FALL_RUN.0) * jitter(&points[i], 5),
            }
        })
        .collect()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Whitewater where a tributary pours into its parent, fading downstream over this many
/// tributary widths (plus a constant).
pub(crate) const CONFLUENCE_FOAM: f32 = 0.55;
pub(crate) const CONFLUENCE_REACH: (f32, f32) = (1.5, 3.);
/// At a confluence, the tributary's flow turns towards its parent's, and the parent's near
/// bank towards the tributary's, by up to this much, over this many tributary widths (plus a
/// constant).
const CONFLUENCE_TURN: f32 = 0.6;
const CONFLUENCE_TURN_REACH: (f32, f32) = (1., 1.5);

/// A river's points, never below the sea if it flows into it (delta branches run on over the
/// sea floor): no falls, nor foam below them, out at sea.
fn floored_points(river: &River) -> Vec<RiverPoint> {
    let floor = if river.end == RiverEnd::Sea { -0.08 } else { f32::MIN };
    river
        .points
        .iter()
        .map(|p| RiverPoint {
            position: p.position.with_y(p.position.y.max(floor)),
            ..*p
        })
        .collect()
}

/// Where the fall after point `i` reaches the next pool, as a fraction of the way to point
/// `i + 1`.
pub(crate) fn fall_split(points: &[RiverPoint], profile: &[ProfilePoint], i: usize) -> f32 {
    let run = (points[i + 1].position - points[i].position).xz().length();
    (profile[i].fall_run * profile[i].drop / run.max(1e-3)).min(0.6)
}

/// A river's water line as drawn (see [`river_profile`]): its points at the profile's levels,
/// with a point where each fall reaches the next pool. The channels are carved along it, so
/// the water never stands above its banks.
pub(super) fn water_line(river: &River) -> Vec<RiverPoint> {
    let points = floored_points(river);
    if points.len() < 2 {
        return points;
    }
    let profile = river_profile(&points);
    let mut line = Vec::with_capacity(points.len() + 8);
    for (i, (point, here)) in points.iter().zip(&profile).enumerate() {
        line.push(RiverPoint {
            position: point.position.with_y(here.level),
            ..*point
        });
        if here.drop > 0. && i + 1 < points.len() {
            let t = fall_split(&points, &profile, i);
            let next = &points[i + 1];
            line.push(RiverPoint {
                position: point.position.lerp(next.position, t).with_y(here.level - here.drop),
                width: point.width + (next.width - point.width) * t,
                depth: point.depth + (next.depth - point.depth) * t,
            });
        }
    }
    line
}

/// The cross-sections of a river ribbon, at its water line (see [`river_profile`]).
fn river_rows(river: &River) -> Vec<Row> {
    let points = &floored_points(river);
    let profile = river_profile(points);
    let tangents: Vec<Vec2> = (0..points.len())
        .map(|i| {
            let previous = points[i.saturating_sub(RIVER_TANGENT_SPAN)].position;
            let next = points[(i + RIVER_TANGENT_SPAN).min(points.len() - 1)].position;
            (next - previous).xz().normalize_or_zero()
        })
        .collect();
    // Offset each side, but never let an edge step backwards along the river: on bends
    // tighter than the river is wide, the inner edge pinches instead of folding over.
    let side = |sign: f32| -> Vec<Vec3> {
        let mut edge: Vec<Vec3> = Vec::with_capacity(points.len());
        for (i, (point, tangent)) in points.iter().zip(&tangents).enumerate() {
            let normal = tangent.perp() * sign;
            let half = point.width / 2. + RIVER_OVERLAP;
            let center = point.position.with_y(profile[i].level) + Vec3::Y * RIVER_OFFSET;
            let mut vertex = center + Vec3::new(normal.x, 0., normal.y) * half;
            if let Some(&last) = edge.last() {
                let step = (point.position - points[i - 1].position).xz();
                let advance = (vertex - last).xz();
                if advance.dot(*tangent) <= 0. || advance.dot(step) <= 0. {
                    vertex = last.with_y(vertex.y);
                }
            }
            edge.push(vertex);
        }
        edge
    };
    let (lefts, rights) = (side(-1.), side(1.));
    // Rows with their speed in `left_flow.x` for now.
    let mut rows: Vec<Row> = Vec::with_capacity(points.len() + 8);
    for (i, here) in profile.iter().enumerate() {
        rows.push(Row {
            left: lefts[i],
            right: rights[i],
            left_flow: Vec2::new(here.speed, 0.),
            right_flow: Vec2::ZERO,
            foam: here.foam,
            fade: 1.,
            dome: 0.,
        });
        // A fall: a steep row a little downstream, at the next pool's level.
        if here.drop > 0. && i + 1 < points.len() {
            let t = fall_split(points, &profile, i);
            let down = Vec3::Y * here.drop;
            rows.push(Row {
                left: lefts[i].lerp(lefts[i + 1], t).with_y(lefts[i].y) - down,
                right: rights[i].lerp(rights[i + 1], t).with_y(rights[i].y) - down,
                left_flow: Vec2::new(here.speed.max(2.), 0.),
                right_flow: Vec2::ZERO,
                foam: 1.,
                fade: 1.,
                dome: 0.,
            });
        }
    }
    // Velocities: along the ribbon, measured over a few rows.
    let centers: Vec<Vec2> = rows.iter().map(|row| row.center().xz()).collect();
    let n = rows.len();
    for (k, row) in rows.iter_mut().enumerate() {
        let direction = (centers[(k + 2).min(n - 1)] - centers[k.saturating_sub(2)])
            .normalize_or_zero();
        let velocity = direction * row.left_flow.x;
        row.left_flow = velocity;
        row.right_flow = velocity;
    }
    // Volume on steep stretches (slope measured over a few rows).
    let heights: Vec<f32> = rows.iter().map(|row| row.center().y).collect();
    for k in 0..n {
        let (a, b) = (k.saturating_sub(2), (k + 2).min(n - 1));
        let run: f32 = (a..b).map(|j| centers[j].distance(centers[j + 1])).sum();
        let slope = (heights[a] - heights[b]).max(0.) / run.max(1e-3);
        let half = rows[k].left.xz().distance(rows[k].right.xz()) / 2.;
        // (Flat at both ends, which join ponds, lakes and parent rivers along flat edges.)
        let ends = (k.min(n - 1 - k) as f32 / 3.).min(1.);
        rows[k].dome = half * smoothstep(DOME_SLOPE.0, DOME_SLOPE.1, slope) * ends;
    }
    rows
}

/// The flow direction of a river at its end (from its last points).
fn river_rows_end_flow(river: &River) -> Vec2 {
    let n = river.points.len();
    (river.points[n - 1].position - river.points[n.saturating_sub(4)].position)
        .xz()
        .normalize_or_zero()
}

/// How far `p` is inside a river's ribbon (positive inside): its half-width (with the overlap
/// under the banks) minus the distance to its center line, at the nearest center segment.
fn inside_ribbon(river: &River, p: Vec2) -> f32 {
    river
        .points
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0].position.xz(), w[1].position.xz());
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0., 1.);
            let half = (w[0].width + (w[1].width - w[0].width) * t) / 2. + RIVER_OVERLAP;
            half - p.distance(a + ab * t)
        })
        .fold(f32::MIN, f32::max)
}

/// A ribbon along each river, at its water line. Tributaries join their parent along its
/// ribbon edge (clipped there, so the two surfaces neither overlap nor leave a gap), and the
/// parent gets whitewater just below the confluence.
///
/// Also returns the triangles (xz) covered by opaque ribbon, which lake surfaces leave out.
fn rivers_mesh(continent: &Continent, springs: &[Spring]) -> (Mesh, Vec<[Vec2; 3]>) {
    let rivers = &continent.rivers;
    let mut opaque = Vec::new();
    let mut all_rows: Vec<Vec<Row>> = rivers.iter().map(river_rows).collect();
    for river in rivers {
        let (RiverEnd::Junction(parent), Some(join)) = (river.end, river.points.last()) else {
            continue;
        };
        let Some(rows) = all_rows.get_mut(parent) else {
            continue;
        };
        let center = |row: &Row| row.center().xz();
        let Some(nearest) = (0..rows.len()).min_by(|&a, &b| {
            center(&rows[a])
                .distance(join.position.xz())
                .total_cmp(&center(&rows[b]).distance(join.position.xz()))
        }) else {
            continue;
        };
        // The parent's bank on the tributary's side turns towards the incoming water.
        let incoming = river_rows_end_flow(river);
        let approach = river.points[river.points.len().saturating_sub(8)].position.xz();
        let turn_reach = CONFLUENCE_TURN_REACH.0 + CONFLUENCE_TURN_REACH.1 * join.width;
        for row in rows.iter_mut() {
            let distance = row.center().xz().distance(join.position.xz());
            if distance > 4. * turn_reach {
                continue;
            }
            let amount = CONFLUENCE_TURN * (-distance / turn_reach).exp();
            if row.left.xz().distance(approach) < row.right.xz().distance(approach) {
                row.left_flow = steer(row.left_flow, incoming, amount);
            } else {
                row.right_flow = steer(row.right_flow, incoming, amount);
            }
        }
        let reach = CONFLUENCE_REACH.0 + CONFLUENCE_REACH.1 * join.width;
        let mut distance = 0.;
        for k in nearest..rows.len() {
            if k > nearest {
                distance += center(&rows[k]).distance(center(&rows[k - 1]));
            }
            let mixing = CONFLUENCE_FOAM * (-distance / reach).exp();
            if mixing < 0.02 {
                break;
            }
            rows[k].foam = rows[k].foam.max(mixing);
        }
    }

    let mut builder = SurfaceBuilder::default();
    // Vertices each river starts at, to check every river shows some water.
    let mut starts = vec![0; rivers.len() + 1];
    let mut inside_parent = vec![false; rivers.len()];
    for (river_index, (river, rows)) in rivers.iter().zip(&all_rows).enumerate() {
        starts[river_index] = builder.positions.len();
        // Where the river reaches the water it flows into (both its banks under that water),
        // it fades out over a few widths, carrying on straight if needed: the overlapping
        // translucent surfaces would show a hard edge. (Not by its own water level: delta
        // branches are at sea level all along.)
        let end_level = match river.end {
            RiverEnd::Sea => 0.,
            RiverEnd::Lake(lake) => continent.lakes[lake].level,
            RiverEnd::Junction(_) => f32::MIN,
        };
        let mut rows = rows.to_vec();
        // Outlets start in their lake (at its deepest point): invisible under the lake, fading
        // in where they leave it.
        // Under a lake's water (as in world generation, which keeps the river at the lake's
        // level there).
        // Under the water of a given lake (by its level: lakes at the same level are one water).
        let under_lake = |p: Vec2, level: f32| {
            continent
                .lake_over(p)
                .is_some_and(|over| (over - level).abs() < 0.02)
        };
        // The lake the river starts in, if any.
        let source = rows.first().and_then(|row| continent.lake_over(row.center().xz()));
        let leave = source.map_or(0, |level| {
            rows.iter()
                .position(|row| !under_lake(row.center().xz(), level))
                .unwrap_or(0)
        });
        let mut rows_dropped = 0;
        if leave > 0 {
            // Visible a little way into the lake, then fading: the lake's surface is the
            // river's water further in.
            let mut distance = 0.;
            for k in (0..leave).rev() {
                distance += rows[k].center().xz().distance(rows[k + 1].center().xz());
                rows[k].fade = (1. - (distance - LAKE_FADE_INSIDE) / OUTLET_FADE).clamp(0., 1.);
            }
            // Drop the rows fully under the lake.
            let first = (0..leave).rev().find(|&k| rows[k].fade <= 0.).unwrap_or(0);
            rows_dropped = first;
            rows.drain(..first);
        }
        let leave = leave.saturating_sub(rows_dropped);
        // (Searched from where the river leaves its own lake: a lake's bed may lie below the
        // water it flows on into.)
        let reached = rows[leave..].iter().position(|row| {
            // Into a lake: where it goes under the lake's water (it carries on to the lake's
            // deepest point at its level, under the surface).
            if let RiverEnd::Lake(lake) = river.end {
                return under_lake(row.center().xz(), continent.lakes[lake].level);
            }
            let bank = |p: Vec3| continent.surface.bilinear(p.xz()) <= end_level + 0.02;
            let deep = continent.surface.bilinear(row.center().xz()) < end_level - OPEN_WATER_DEPTH;
            (bank(row.left) && bank(row.right)) || deep
        });
        // Never below the sea (delta branches carry on over the sea floor: no falls into it).
        if river.end == RiverEnd::Sea {
            let floor = end_level - 0.1 + RIVER_OFFSET;
            for row in &mut rows {
                row.left.y = row.left.y.max(floor);
                row.right.y = row.right.y.max(floor);
            }
        }
        if let Some(start) = reached {
            let mut start = start + leave;
            let length = if matches!(river.end, RiverEnd::Lake(_)) {
                // Visible a little way into the lake before fading.
                let mut distance = 0.;
                while start + 1 < rows.len() && distance < LAKE_FADE_INSIDE {
                    distance += rows[start].center().xz().distance(rows[start + 1].center().xz());
                    start += 1;
                }
                Some(OUTLET_FADE)
            } else {
                None
            };
            fade_out(&mut rows, start, length);
        }
        if rows.len() < 2 {
            continue;
        }
        for pair in rows.windows(2) {
            if pair[0].fade >= 1. && pair[1].fade >= 1. {
                let (a, b) = (&pair[0], &pair[1]);
                opaque.push([a.left.xz(), a.right.xz(), b.right.xz()]);
                opaque.push([a.left.xz(), b.right.xz(), b.left.xz()]);
            }
        }
        match springs.iter().find(|spring| spring.river == river_index).map(|s| (s, &s.kind)) {
            // Out of a pond: the river starts on the pond's outline (sharing its vertices).
            Some((spring, SpringKind::Pond { outline, .. })) => {
                let y = spring.position.y + RIVER_OFFSET;
                let outflow = spring.direction * 0.15;
                let mut edge: Vec<EdgeVertex> = outline
                    .iter()
                    .map(|p| (p.extend(y).xzy(), outflow, 0.1))
                    .collect();
                edge.push(edge[0]);
                let reversed: Vec<Row> = rows.iter().rev().cloned().collect();
                if let Some(cap) = junction_cap(&reversed, &edge, true) {
                    fan(&mut builder, &cap.outline);
                    rows = reversed[..cap.full_rows].iter().rev().cloned().collect();
                }
            }
            // Out of the hillside: a spout from the notch, foaming.
            Some((spring, SpringKind::Hillside)) => {
                for (row, foam) in rows.iter_mut().zip(SPRING_FOAM) {
                    row.foam = row.foam.max(foam);
                }
                let first = &rows[0];
                let center = first.center();
                let back = (spring.direction * 0.4).extend(0.).xzy();
                let spout = Row {
                    left: first.left.lerp(center, 0.55) - back + Vec3::Y * SPOUT_HEIGHT,
                    right: first.right.lerp(center, 0.55) - back + Vec3::Y * SPOUT_HEIGHT,
                    left_flow: first.left_flow.normalize_or_zero() * 2.,
                    right_flow: first.right_flow.normalize_or_zero() * 2.,
                    foam: 1.,
                    fade: 1.,
                    dome: 0.,
                };
                rows.insert(0, spout);
            }
            None => {}
        }
        let parent = match river.end {
            RiverEnd::Junction(parent) => rivers.get(parent).zip(all_rows.get(parent)),
            _ => None,
        };
        let Some((parent_river, parent_rows)) = parent else {
            for pair in rows.windows(2) {
                builder.ribbon_quad(&pair[0], &pair[1]);
            }
            continue;
        };
        // Near the confluence, the tributary turns towards its parent's flow.
        let turn_reach = CONFLUENCE_TURN_REACH.0
            + CONFLUENCE_TURN_REACH.1 * river.points.last().map_or(0., |p| p.width);
        for row in &mut rows {
            for (vertex, flow) in [(row.left, &mut row.left_flow), (row.right, &mut row.right_flow)] {
                let depth = inside_ribbon(parent_river, vertex.xz());
                if depth < -2. * turn_reach {
                    continue;
                }
                let amount = CONFLUENCE_TURN * smoothstep(-2. * turn_reach, 0., depth);
                let nearest = parent_rows.iter().min_by(|a, b| {
                    a.center()
                        .xz()
                        .distance_squared(vertex.xz())
                        .total_cmp(&b.center().xz().distance_squared(vertex.xz()))
                });
                if let Some(nearest) = nearest {
                    *flow = steer(*flow, nearest.flow(), amount);
                }
            }
        }
        // The tributary ends on its parent's actual mesh edge, sharing its vertices (the side it
        // comes from, or else the other).
        let near = near_edge(parent_rows, &rows, true);
        let cap = junction_cap(&rows, &near, false)
            .or_else(|| junction_cap(&rows, &near_edge(parent_rows, &rows, false), false));
        // A tributary lying entirely in its parent's channel: nothing to draw.
        let inside = rows.iter().all(|row| {
            inside_ribbon(&rivers[match river.end {
                RiverEnd::Junction(parent) => parent,
                _ => unreachable!(),
            }], row.center().xz())
                > 0.
        });
        if cap.is_none() && inside {
            inside_parent[river_index] = true;
            continue;
        }
        match cap {
            Some(cap) => {
                for pair in rows[..cap.full_rows].windows(2) {
                    builder.ribbon_quad(&pair[0], &pair[1]);
                }
                fan(&mut builder, &cap.outline);
            }
            None => {
                warn!(
                    "River {river_index} never crosses the edge of its parent {:?} (joining it past its end, e.g. where a delta cut it back?): drawn overlapping it",
                    river.end
                );
                for pair in rows.windows(2) {
                    builder.ribbon_quad(&pair[0], &pair[1]);
                }
            }
        }
    }
    starts[rivers.len()] = builder.positions.len();
    for (index, river) in rivers.iter().enumerate() {
        if starts[index + 1] == starts[index] && !inside_parent[index] && river.points.len() >= 4 {
            warn!(
                "River {index} ({} points, from {} to {:?}) shows no water at all",
                river.points.len(),
                river.points[0].position,
                river.end
            );
        }
    }
    (builder.build(), opaque)
}

/// A vertex of a ribbon edge: position, flow velocity, whitewater.
type EdgeVertex = (Vec3, Vec2, f32);

/// Fill a closed outline with a fan from its middle.
fn fan(builder: &mut SurfaceBuilder, outline: &[EdgeVertex]) {
    let n = outline.len() as f32;
    let middle = outline.iter().map(|v| v.0).sum::<Vec3>() / n;
    let middle_flow = outline.iter().map(|v| v.1).sum::<Vec2>() / n;
    let middle_foam = outline.iter().map(|v| v.2).sum::<f32>() / n;
    let center = builder.vertex(middle, middle_flow, middle_foam);
    let vertices: Vec<u32> = outline
        .iter()
        .map(|&(position, flow, foam)| builder.vertex(position, flow, foam))
        .collect();
    for (k, &vertex) in vertices.iter().enumerate() {
        let next = vertices[(k + 1) % vertices.len()];
        builder.indices.extend([center, vertex, next]);
    }
}

/// The edge of the parent ribbon on the tributary's side (`near`), or the other one.
fn near_edge(parent_rows: &[Row], tributary: &[Row], near: bool) -> Vec<EdgeVertex> {
    let (Some(end), Some(approach)) = (
        tributary.last(),
        tributary.get(tributary.len().saturating_sub(6)),
    ) else {
        return Vec::new();
    };
    let approach = approach.center().xz();
    let nearest = parent_rows.iter().min_by(|a, b| {
        a.center()
            .xz()
            .distance_squared(end.center().xz())
            .total_cmp(&b.center().xz().distance_squared(end.center().xz()))
    });
    let left = nearest.is_none_or(|row| {
        row.left.xz().distance(approach) < row.right.xz().distance(approach)
    }) == near;
    parent_rows
        .iter()
        .map(|row| {
            if left {
                (row.left, row.left_flow, row.foam)
            } else {
                (row.right, row.right_flow, row.foam)
            }
        })
        .collect()
}

/// Where the segment `a` -> `b` crosses `c` -> `d` (in xz): the parameters along each.
fn crossing(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> Option<(f32, f32)> {
    let (ab, cd, ac) = ((b - a).xz(), (d - c).xz(), (c - a).xz());
    let denominator = ab.perp_dot(cd);
    if denominator.abs() < 1e-9 {
        return None;
    }
    let t = ac.perp_dot(cd) / denominator;
    let u = ac.perp_dot(ab) / denominator;
    ((0.0..=1.).contains(&t) && (0.0..=1.).contains(&u)).then_some((t, u))
}

/// The end of a tributary on its parent's edge: the rows drawn as quads, then an outline
/// (from the last full row's left vertex along the tributary's left edge to the parent's edge,
/// along the parent's edge, and back along the right edge) to fill.
struct JunctionCap {
    full_rows: usize,
    outline: Vec<EdgeVertex>,
}

/// `edge` is a polyline; with `closed`, a loop (its last vertex repeating the first), and the
/// outline follows the shorter way round between the crossings.
fn junction_cap(rows: &[Row], edge: &[EdgeVertex], closed: bool) -> Option<JunctionCap> {
    // Last crossing of each tributary edge with the parent's edge (where it finally enters the
    // parent: a tributary may brush the parent's edge before running alongside it): (row,
    // point, position along the parent's edge).
    let hit = |side: fn(&Row) -> (Vec3, Vec2)| -> Option<(usize, EdgeVertex, f32)> {
        for k in (1..rows.len()).rev() {
            let (a, b) = (side(&rows[k - 1]).0, side(&rows[k]).0);
            for j in 1..edge.len() {
                if let Some((_, u)) = crossing(a, b, edge[j - 1].0, edge[j].0) {
                    let (c, d) = (edge[j - 1], edge[j]);
                    let vertex = (c.0.lerp(d.0, u), c.1.lerp(d.1, u), c.2 + (d.2 - c.2) * u);
                    return Some((k, vertex, (j - 1) as f32 + u));
                }
            }
        }
        None
    };
    // A tributary joining at a shallow angle, nearly as wide as its parent, may end with one
    // edge still outside it (running alongside): that edge's last vertex is moved onto the
    // parent's edge.
    let snap = |side: fn(&Row) -> (Vec3, Vec2)| -> Option<(usize, EdgeVertex, f32)> {
        let p = side(rows.last()?).0.xz();
        let mut best: Option<(f32, EdgeVertex, f32)> = None;
        for j in 1..edge.len() {
            let (c, d) = (edge[j - 1], edge[j]);
            let cd = (d.0 - c.0).xz();
            let u = ((p - c.0.xz()).dot(cd) / cd.length_squared().max(1e-9)).clamp(0., 1.);
            let distance = p.distance(c.0.xz() + cd * u);
            if best.is_none_or(|b| distance < b.0) {
                let vertex = (c.0.lerp(d.0, u), c.1.lerp(d.1, u), c.2 + (d.2 - c.2) * u);
                best = Some((distance, vertex, (j - 1) as f32 + u));
            }
        }
        best.map(|(_, vertex, at)| (rows.len() - 1, vertex, at))
    };
    let (left, right) = (
        hit(|row| (row.left, row.left_flow)),
        hit(|row| (row.right, row.right_flow)),
    );
    let (left, right) = match (left, right) {
        (Some(left), Some(right)) => (left, right),
        (Some(left), None) => (left, snap(|row| (row.right, row.right_flow))?),
        (None, Some(right)) => (snap(|row| (row.left, row.left_flow))?, right),
        (None, None) => return None,
    };
    let ((left_row, left_hit, left_at), (right_row, right_hit, right_at)) = (left, right);
    let full_rows = left_row.min(right_row);
    let mut outline = Vec::new();
    for row in &rows[full_rows - 1..left_row] {
        outline.push((row.left, row.left_flow, row.foam));
    }
    outline.push(left_hit);
    // Edge vertices strictly between the two crossings.
    let n = if closed { edge.len() - 1 } else { edge.len() };
    let forward = if closed {
        (right_at - left_at).rem_euclid(n as f32) <= n as f32 / 2.
    } else {
        left_at < right_at
    };
    let (from, mut to) = (left_at, right_at);
    if forward {
        if closed && to < from {
            to += n as f32;
        }
        for k in from.floor() as i64 + 1..=to.floor() as i64 {
            outline.push(edge[k.rem_euclid(n as i64) as usize]);
        }
    } else {
        if closed && to > from {
            to -= n as f32;
        }
        let mut k = from.floor() as i64;
        while k > to.floor() as i64 {
            outline.push(edge[k.rem_euclid(n as i64) as usize]);
            k -= 1;
        }
    }
    outline.push(right_hit);
    for row in rows[full_rows - 1..right_row].iter().rev() {
        outline.push((row.right, row.right_flow, row.foam));
    }
    Some(JunctionCap { full_rows, outline })
}
