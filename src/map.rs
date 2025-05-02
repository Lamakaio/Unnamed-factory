use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages,
    math::{I64Vec2, IVec2},
    pbr::wireframe::{Wireframe, WireframeColor},
    platform::collections::HashMap,
    prelude::*,
    render::mesh::{Indices, PrimitiveTopology},
};
use kdtree_collisions::{KdTree, KdValue};

use crate::parts::{Building, BuildingType};

pub struct MapPlugin;
impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Map::default());
        app.add_systems(Update, spawn_chunk);
    }
}

pub const GRID_SQUARE_SIZE: f32 = 1.;
/// An instance of a specific building at a position
/// Might contain other instance-specific stats in the future (damage, etc)
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

/// A single point of terrain, with an height.
/// Potentially contains terrain type for texturing (and other stuff ?)
#[derive(Clone)]
pub struct TerrainPoint {
    height: f32,
}

/// A chunk, containing terrain data
pub struct Chunk {
    grid: Vec<TerrainPoint>,
    chunk_position: I64Vec2,
    cached_mesh: Option<Handle<Mesh>>,
    spawned: bool,
}

#[derive(Component)]
pub struct ChunkMarker(pub I64Vec2);

impl Chunk {
    const CHUNK_SIZE: u32 = 64;
    const WORLD_CHUNK_SIZE: f32 = Self::CHUNK_SIZE as f32 * GRID_SQUARE_SIZE;

    /// get a dummy terrain chunk for testing purpose
    fn get_dummy() -> Self {
        let mut grid = Vec::new();
        for _ in 0..Self::CHUNK_SIZE {
            for j in 0..Self::CHUNK_SIZE {
                grid.push(TerrainPoint {
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

    /// Get the in-world position of the origin of the chunk.
    fn get_world_pos(&self) -> Vec3 {
        Vec3::new(
            self.chunk_position.x as f32,
            0.,
            self.chunk_position.y as f32,
        ) * Self::WORLD_CHUNK_SIZE
    }

    /// Generates the mesh for a chunk.
    // TODO: a way to regenerate mesh on terrain change
    fn make_mesh(&self) -> Mesh {
        let mut vertex_positions = Vec::with_capacity(Self::CHUNK_SIZE.pow(2) as usize);
        let mut indices = Vec::with_capacity(((Self::CHUNK_SIZE - 1).pow(2) * 6) as usize);
        let chunk_pos = self.get_world_pos();
        for (i, sq) in self.grid.iter().enumerate() {
            let offset_x = GRID_SQUARE_SIZE * (i as u32 % Self::CHUNK_SIZE) as f32;
            let offset_z = GRID_SQUARE_SIZE * (i as u32 / Self::CHUNK_SIZE) as f32;
            vertex_positions.push([chunk_pos.x + offset_x, sq.height, chunk_pos.z + offset_z]);
        }
        for x in 1..Self::CHUNK_SIZE {
            for z in 1..Self::CHUNK_SIZE {
                fn id(x: u32, z: u32) -> u32 {
                    x + z * Chunk::CHUNK_SIZE
                }
                //top top left triangle
                indices.extend(&[id(x, z), id(x, z - 1), id(x - 1, z - 1)]);
                //top left left triangle
                indices.extend(&[id(x, z), id(x - 1, z - 1), id(x - 1, z)]);
            }
        }

        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vertex_positions)
        .with_inserted_indices(Indices::U32(indices))
        .with_computed_smooth_normals()
    }

    /// Get a handle to the mesh of the chunk, generating it on the fly if necessary.
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

/// The whole map. Contains chunks, and a kd-tree of building instances in the map.
#[derive(Resource, Default)]
pub struct Map {
    chunks: HashMap<I64Vec2, Chunk>,
    entities: KdTree<BuildingInstance, 10>,
}

impl Map {
    /// Get a mutable reference to a chunk (and make/ load it if it doesnt already exists)
    fn get_chunk_mut<'a>(&'a mut self, pos: &I64Vec2) -> &'a mut Chunk {
        //Apparently it's the best way to insert an element if it doesnt already exists, and get a mut ref to the result.
        self.chunks
            .raw_entry_mut()
            .from_key(pos)
            .or_insert_with(|| (pos.clone(), Chunk::get_dummy()))
            .1
    }
}

/// Handles the spawning of chunks when the camera is close enough. (Currently only spawns the chunk the camera is on)
pub fn spawn_chunk(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut map: ResMut<Map>,
    camera: Query<&Transform, (With<Camera>, Changed<Transform>)>,
) -> Result {
    let camera_transform = camera.single()?;
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
                    model,
                } => entity.with_child((
                    Mesh3d(model.mesh.clone()),
                    MeshMaterial3d(model.material.clone()),
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
    Ok(())
}
