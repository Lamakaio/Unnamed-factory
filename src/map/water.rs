//! Water surfaces: the sea at height 0, lakes at their level, and river ribbons following the
//! water line of each river. They are translucent and ignored by picking, so clicks and
//! placement reach the terrain below.

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use worldgen::Continent;

use super::Map;
use crate::AppState;

/// Size of the sea plane, in world units.
const SEA_SIZE: f32 = 8000.;
/// River ribbons extend this far past the channel edges, under the banks.
const RIVER_OVERLAP: f32 = 0.25;
/// Rivers float this much above their water line, to avoid fighting with lakes and the sea.
const RIVER_OFFSET: f32 = 0.02;

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_sea)
            .add_systems(OnEnter(AppState::InGame), spawn_lakes_and_rivers);
    }
}

/// Shared material of all water surfaces.
#[derive(Resource)]
struct WaterMaterial(Handle<StandardMaterial>);

fn spawn_sea(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.12, 0.32, 0.55, 0.85),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.08,
        reflectance: 0.6,
        // River ribbons wind either way depending on the flow direction.
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(WaterMaterial(material.clone()));
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
    material: Res<WaterMaterial>,
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
        let points = &river.points;
        let edges: Vec<(Vec3, Vec3)> = (0..points.len())
            .map(|i| {
                let previous = points[i.saturating_sub(1)].position;
                let next = points[(i + 1).min(points.len() - 1)].position;
                let normal = (next - previous).xz().normalize_or_zero().perp();
                let half = points[i].width / 2. + RIVER_OVERLAP;
                let center = points[i].position + Vec3::Y * RIVER_OFFSET;
                let offset = Vec3::new(normal.x, 0., normal.y) * half;
                (center - offset, center + offset)
            })
            .collect();
        for pair in edges.windows(2) {
            let ((a_left, a_right), (b_left, b_right)) = (pair[0], pair[1]);
            builder.quad([a_left, a_right, b_left, b_right]);
        }
    }
    builder.build()
}
