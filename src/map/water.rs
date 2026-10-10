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
use worldgen::{Continent, RiverEnd, RiverPoint};

use super::Map;
use crate::{
    AppState,
    shaders::{WaterMaterial, WaterShader},
};

/// Size of the sea plane, in world units.
const SEA_SIZE: f32 = 8000.;
/// River ribbons extend this far past the channel edges, under the banks.
const RIVER_OVERLAP: f32 = 0.25;
/// Rivers float this much above their water line, to avoid fighting with lakes and the sea.
const RIVER_OFFSET: f32 = 0.02;
/// River ribbon directions are measured over this many points on each side.
const RIVER_TANGENT_SPAN: usize = 2;
/// Rivers steeper than this (water line slope) run in steps: pools between small falls.
const CASCADE_SLOPE: f32 = 0.18;
/// Step height of cascades, relative to the channel depth (the pools stay above the bed).
const CASCADE_STEP: f32 = 0.45;
/// Pools keep this fraction of the natural slope (centred on the natural water line, so the
/// water never strays far from the carved channel); the falls take the rest.
const POOL_TILT: f32 = 0.35;
/// Horizontal extent of a fall, relative to its drop.
const FALL_RUN: f32 = 0.3;

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
    for (name, mesh) in [
        ("Lakes", lakes_mesh(continent)),
        ("Rivers", rivers_mesh(continent)),
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
    /// Rivers only: (across, travel time) and (speed, whitewater), see [`Row`].
    flow: Vec<[f32; 2]>,
    flow_b: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl SurfaceBuilder {
    fn quad(&mut self, corners: [Vec3; 4]) {
        let first = self.positions.len() as u32;
        self.positions
            .extend(corners.map(|corner| corner.to_array()));
        self.indices
            .extend([first, first + 2, first + 1, first + 1, first + 2, first + 3]);
    }

    /// A ribbon quad between two river rows, split along the diagonal giving the better
    /// triangles: at a pinched bend, the usual diagonal can fall outside the quad and flip a
    /// triangle.
    fn ribbon_quad(&mut self, a: &Row, b: &Row) {
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
        self.flow.extend([
            [-a.half, a.time],
            [a.half, a.time],
            [-b.half, b.time],
            [b.half, b.time],
        ]);
        self.flow_b.extend([
            [a.speed, a.foam],
            [a.speed, a.foam],
            [b.speed, b.foam],
            [b.speed, b.foam],
        ]);
        if other <= usual {
            self.indices
                .extend([first, first + 2, first + 1, first + 1, first + 2, first + 3]);
        } else {
            self.indices
                .extend([first, first + 3, first + 1, first, first + 2, first + 3]);
        }
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

/// One quad per lake cell, plus the shore cells around (above the water, so the surface reaches
/// under the shore without floating over the outlet).
fn lakes_mesh(continent: &Continent) -> Mesh {
    let size = continent.size();
    let cell = continent.settings.cell_size;
    let mut level = vec![None; size * size];
    for lake in &continent.lakes {
        for c in &lake.cells {
            for dz in -2..=2i32 {
                for dx in -2..=2i32 {
                    let (x, z) = (c.x as i32 + dx, c.y as i32 + dz);
                    if x < 0 || z < 0 || x as usize >= size || z as usize >= size {
                        continue;
                    }
                    let i = z as usize * size + x as usize;
                    let is_lake = (dx, dz) == (0, 0);
                    // Shore and shallow fringe, but not the outlet valley below the dam.
                    if is_lake || continent.heights[i] > lake.level - 0.15 {
                        level[i] = Some(lake.level);
                    }
                }
            }
        }
    }
    let mut builder = SurfaceBuilder::default();
    for (i, level) in level.iter().enumerate() {
        let Some(level) = *level else {
            continue;
        };
        let center = continent.cell_to_world(i % size, i / size);
        let h = cell / 2.;
        builder.quad([
            Vec3::new(center.x - h, level, center.y - h),
            Vec3::new(center.x + h, level, center.y - h),
            Vec3::new(center.x - h, level, center.y + h),
            Vec3::new(center.x + h, level, center.y + h),
        ]);
    }
    builder.build()
}

/// A cross-section of a river ribbon.
struct Row {
    left: Vec3,
    right: Vec3,
    /// Distance from the center line to the ribbon edges.
    half: f32,
    /// Travel time of the water from the source (seconds): the shader scrolls its flow pattern
    /// along it, so faster water has longer streaks and the pattern never distorts.
    time: f32,
    /// Flow speed, world units per second.
    speed: f32,
    /// Whitewater, 0..1 (rapids, and below falls).
    foam: f32,
}

/// Water line and flow of a river, per point.
struct ProfilePoint {
    level: f32,
    speed: f32,
    foam: f32,
    /// Height of the fall right after this point (0 if none).
    drop: f32,
}

/// The water line of a river (in steps where steep, see [`POOL_TILT`]) and its flow.
fn river_profile(points: &[RiverPoint]) -> Vec<ProfilePoint> {
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
        let step = CASCADE_STEP * points[i].depth;
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
        let mid = (top + points[end].position.y) / 2.;
        for (level, point) in levels[i..=end].iter_mut().zip(&points[i..=end]) {
            *level = mid + POOL_TILT * (point.position.y - mid);
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
            }
        })
        .collect()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// A ribbon along each river, at its water line (see [`river_profile`]).
fn rivers_mesh(continent: &Continent) -> Mesh {
    let mut builder = SurfaceBuilder::default();
    for river in &continent.rivers {
        // Stop where the river reaches the water it flows into (its water line, or the ground
        // under it, at that level): overlapping translucent surfaces show as a darker patch.
        // Close to the sea level, the infinite sea surface already covers the carved channel.
        let (end_level, margin) = match river.end {
            RiverEnd::Sea => (0., 0.12),
            RiverEnd::Lake(lake) => (continent.lakes[lake].level, 1e-3),
            RiverEnd::Junction(_) => (f32::MIN, 0.),
        };
        let points = &river.points;
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
        let mut time = 0.;
        let mut rows: Vec<Row> = Vec::with_capacity(points.len() + 8);
        for (i, here) in profile.iter().enumerate() {
            if i > 0 {
                let run = (points[i].position - points[i - 1].position).xz().length();
                time += run * 2. / (here.speed + profile[i - 1].speed);
            }
            let half = points[i].width / 2. + RIVER_OVERLAP;
            rows.push(Row {
                left: lefts[i],
                right: rights[i],
                half,
                time,
                speed: here.speed,
                foam: here.foam,
            });
            // A fall: a steep row a little downstream, at the next pool's level.
            if here.drop > 0. && i + 1 < points.len() {
                let run = (points[i + 1].position - points[i].position).xz().length();
                let t = (FALL_RUN * here.drop / run.max(1e-3)).min(0.45);
                let down = Vec3::Y * here.drop;
                rows.push(Row {
                    left: lefts[i].lerp(lefts[i + 1], t).with_y(lefts[i].y) - down,
                    right: rights[i].lerp(rights[i + 1], t).with_y(rights[i].y) - down,
                    half,
                    time: time + 0.05,
                    speed: here.speed.max(2.),
                    foam: 1.,
                });
            }
        }
        for pair in rows.windows(2) {
            let center = (pair[1].left + pair[1].right) / 2.;
            let ground = continent.surface.bilinear(center.xz());
            if center.y - RIVER_OFFSET <= end_level + margin || ground <= end_level {
                break;
            }
            builder.ribbon_quad(&pair[0], &pair[1]);
        }
    }
    builder.build()
}
