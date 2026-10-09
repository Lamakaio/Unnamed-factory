//! Water surfaces: the sea at height 0, lakes at their level, and river ribbons following the
//! water line of each river. They are translucent and ignored by picking, so clicks and
//! placement reach the terrain below.

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use worldgen::{Continent, RiverEnd};

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

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_sea)
            .add_systems(OnEnter(AppState::InGame), spawn_lakes_and_rivers);
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

    /// A ribbon quad (`[a_left, a_right, b_left, b_right]`), split along the diagonal giving
    /// the better triangles: at a pinched bend, the usual diagonal can fall outside the quad
    /// and flip a triangle.
    fn ribbon_quad(&mut self, corners: [Vec3; 4]) {
        let area = |a: usize, b: usize, c: usize| {
            (corners[b] - corners[a]).xz().perp_dot((corners[c] - corners[a]).xz())
        };
        let usual = area(0, 2, 1).min(area(1, 2, 3));
        let other = area(0, 3, 1).min(area(0, 2, 3));
        if other <= usual {
            self.quad(corners);
            return;
        }
        let first = self.positions.len() as u32;
        self.positions
            .extend(corners.map(|corner| corner.to_array()));
        self.indices
            .extend([first, first + 3, first + 1, first, first + 2, first + 3]);
    }

    fn build(self) -> Mesh {
        let normals = vec![[0., 1., 0.]; self.positions.len()];
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(self.indices))
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

/// A ribbon along each river, at its water line.
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
                let center = point.position + Vec3::Y * RIVER_OFFSET;
                let mut vertex = center + Vec3::new(normal.x, 0., normal.y) * half;
                if let Some(&last) = edge.last() {
                    let step = (point.position - points[i - 1].position).xz();
                    let advance = (vertex - last).xz();
                    if advance.dot(*tangent) <= 0. || advance.dot(step) <= 0. {
                        vertex = last.with_y(vertex.y.min(last.y));
                    }
                }
                edge.push(vertex);
            }
            edge
        };
        let (lefts, rights) = (side(-1.), side(1.));
        let edges: Vec<(Vec3, Vec3)> = lefts.into_iter().zip(rights).collect();
        for (pair, ends) in edges.windows(2).zip(points.windows(2)) {
            let ground = continent.surface.bilinear(ends[1].position.xz());
            if ends[1].position.y <= end_level + margin || ground <= end_level {
                break;
            }
            let ((a_left, a_right), (b_left, b_right)) = (pair[0], pair[1]);
            builder.ribbon_quad([a_left, a_right, b_left, b_right]);
        }
    }
    builder.build()
}
