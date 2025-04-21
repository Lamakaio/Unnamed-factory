use std::sync::Arc;

use bevy::{
    log::tracing_subscriber::filter::targets::Iter,
    math::{I64Vec2, IVec2},
    pbr::wireframe::{Wireframe, WireframeColor},
    prelude::*,
    utils::HashMap,
};
use kdtree_collisions::{KdTree, KdValue};

use crate::parts::{BuildModelType, Building, BuildingType};

pub struct MapPlugin;
impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Map::default());
        app.add_systems(Update, spawn_chunk);
    }
}

pub const GRID_SQUARE_SIZE: f32 = 1.;

#[derive(PartialEq, Clone)]
pub struct BuildingInstance {
    building: Arc<Building>,
    grid_pos: I64Vec2,
    size: IVec2,
}

impl KdValue for BuildingInstance {
    type Position = i64;

    fn min_x(&self) -> Self::Position {
        self.grid_pos.x
    }

    fn min_y(&self) -> Self::Position {
        self.grid_pos.y
    }

    fn max_x(&self) -> Self::Position {
        self.grid_pos.x + self.building.size.x
    }

    fn max_y(&self) -> Self::Position {
        self.grid_pos.y + self.building.size.y
    }
}
#[derive(Clone)]
pub struct TerrainSquare {
    height: f32,
}

pub struct Chunk {
    grid: Vec<TerrainSquare>,
    chunk_position: I64Vec2,
    cached_mesh: Option<Handle<Mesh>>,
    spawned: bool,
}

#[derive(Component)]
pub struct ChunkMarker(pub I64Vec2);

impl Chunk {
    const CHUNK_SIZE: usize = 64;
    const WORLD_CHUNK_SIZE: f32 = Self::CHUNK_SIZE as f32 * GRID_SQUARE_SIZE;

    fn get_dummy() -> Self {
        let mut grid = Vec::new();
        for _ in 0..Self::CHUNK_SIZE {
            for j in 0..Self::CHUNK_SIZE {
                grid.push(TerrainSquare {
                    height: (j % 2) as f32,
                });
            }
        }
        Self {
            grid,
            chunk_position: (0, 0).into(),
            cached_mesh: None,
            spawned: false,
        }
    }

    fn get_world_pos(&self) -> Vec3 {
        Vec3::new(
            self.chunk_position.x as f32,
            0.,
            self.chunk_position.y as f32,
        ) * Self::WORLD_CHUNK_SIZE
    }

    fn make_mesh(&self) -> impl MeshBuilder {
        Plane3d::default()
            .mesh()
            .size(Self::WORLD_CHUNK_SIZE, Self::WORLD_CHUNK_SIZE)
            .subdivisions(Self::CHUNK_SIZE as u32)
    }

    fn get_mesh(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        if let Some(mesh) = &self.cached_mesh {
            mesh.clone()
        } else {
            let mesh = meshes.add(self.make_mesh());
            self.cached_mesh = Some(mesh.clone());
            mesh
        }
    }
}

#[derive(Resource, Default)]
pub struct Map {
    chunks: HashMap<I64Vec2, Chunk>,
    entities: KdTree<BuildingInstance, 10>,
}

impl Map {
    fn get_chunk_mut<'a>(&'a mut self, pos: &I64Vec2) -> &'a mut Chunk {
        //Apparently it's the best way to insert an element if it doesnt already exists, and get a mut ref to the result.
        self.chunks
            .raw_entry_mut()
            .from_key(pos)
            .or_insert_with(|| (pos.clone(), Chunk::get_dummy()))
            .1
    }
}

pub fn spawn_chunk(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut map: ResMut<Map>,
    camera: Query<&Transform, (With<Camera>, Changed<Transform>)>,
) {
    if let Ok(camera_transform) = camera.get_single() {
        let camera_chunk_pos = camera_transform.translation / Chunk::WORLD_CHUNK_SIZE;
        let chunk_pos = I64Vec2::new(camera_chunk_pos.x as i64, camera_chunk_pos.z as i64);
        let chunk = map.get_chunk_mut(&chunk_pos);
        if !chunk.spawned {
            chunk.spawned = true;
            let mesh = chunk.get_mesh(&mut *meshes);
            let mut entity = commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(
                    materials.add(Color::from(bevy::color::palettes::css::LIGHT_STEEL_BLUE)),
                ),
                Transform::from_translation(chunk.get_world_pos()),
            ));

            for build in map.entities.query_rect(
                chunk_pos.x,
                chunk_pos.x + Chunk::CHUNK_SIZE as i64,
                chunk_pos.y,
                chunk_pos.y + Chunk::CHUNK_SIZE as i64,
            ) {
                let pos = Vec3::new(
                    (build.grid_pos.x - chunk_pos.x) as f32 * GRID_SQUARE_SIZE,
                    0.,
                    (build.grid_pos.y - chunk_pos.y) as f32 * GRID_SQUARE_SIZE,
                );
                match &build.building.typ {
                    BuildingType::Single {
                        model: BuildModelType::Scene(scene),
                    } => entity
                        .with_child((SceneRoot(scene.clone()), Transform::from_translation(pos))),
                    BuildingType::Single {
                        model: BuildModelType::MeshMaterial(mesh, mat),
                    } => entity.with_child((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(mat.clone()),
                        Transform::from_translation(pos),
                    )),
                    BuildingType::Zone { color } => entity.with_child((
                        Mesh3d(todo!()),
                        Wireframe,
                        WireframeColor {
                            color: color.clone(),
                        },
                        Transform::from_translation(pos).with_scale(Vec3::new(
                            build.size.x as f32 * GRID_SQUARE_SIZE,
                            0.1,
                            build.size.y as f32 * GRID_SQUARE_SIZE,
                        )),
                    )),
                };
            }
        }
    }
}
