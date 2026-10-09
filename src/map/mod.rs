//! The world map: generation, the continuous terrain height function, adaptive LOD terrain
//! chunks, and the placed buildings.
//!
//! - The continent is generated in the background at startup (`AppState::Loading`) by the
//!   `worldgen` crate (erosion, rivers, lakes).
//! - Water surfaces (sea, lakes, rivers) are meshes built from the generated data ([`water`]).
//! - Terrain heights come from [`heights::TerrainHeights`]: base heightmap + rivers + edits.
//! - Chunks around the camera are meshed in background tasks with a quadtree LOD
//!   ([`mesh`]), and re-meshed when the camera moves enough or an edit touches them.

mod edits;
mod heights;
mod mesh;
mod rivers;
mod spatial;
mod water;

use std::time::Instant;

use bevy::{
    math::I64Vec2,
    platform::collections::HashMap,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use kdtree_collisions::{KdTree, KdValue};
use serde::Deserialize;

pub use edits::TerrainEdit;
pub use heights::TerrainSample;

use worldgen::{Continent, WorldgenSettings};

use crate::{AppState, build::Building, shaders::MapMaterial};
use heights::TerrainHeights;

/// Size of a heightmap cell, in world units.
pub const GRID_SQUARE_SIZE: f32 = 0.25;
/// Size of a terrain chunk, in world units.
pub const CHUNK_SIZE: f32 = 64.;
/// Finest terrain mesh cell, used around rivers and edits.
pub const MIN_CELL: f32 = GRID_SQUARE_SIZE / 4.;

/// Chunks within this distance of the camera (in chunks) are loaded.
const CHUNK_LOAD_RADIUS: i64 = 6;
/// Maximum number of chunk meshes being built at once.
const MAX_MESH_TASKS: usize = 4;
/// A chunk is re-meshed when the camera moved by this fraction of its distance to the chunk...
const REMESH_MOVE_RATIO: f32 = 0.25;
/// ...and at least by this many world units.
const REMESH_MIN_MOVE: f32 = 4.;

const MAP_MATERIAL: &str = "materials/map.mapmat";

pub struct MapPlugin {
    pub seed: u128,
}

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        let seed = self.seed as u32;
        app.add_plugins(water::WaterPlugin)
            .add_systems(Startup, move |commands: Commands| {
                start_generation(commands, seed)
            })
            .add_systems(
                Update,
                (
                    finish_generation.run_if(in_state(AppState::Loading)),
                    (spawn_chunks, schedule_chunk_meshes, finish_chunk_meshes)
                        .chain()
                        .run_if(in_state(AppState::InGame)),
                    bind_gradient_texture.run_if(in_state(AppState::InGame)),
                ),
            );
    }
}

/// The whole map: terrain heights, loaded chunks, and the placed buildings.
#[derive(Resource)]
pub struct Map {
    material: Handle<MapMaterial>,
    /// Base surface gradient texture of the terrain shader, and its world mapping.
    gradient: (Handle<Image>, Vec4),
    heights: TerrainHeights,
    /// Loaded chunk entities.
    chunks: HashMap<I64Vec2, Entity>,
    /// Number of edits that touched each chunk, to know when to re-mesh it.
    chunk_edits: HashMap<I64Vec2, u64>,
    pub entities: KdTree<BuildingInstance, 10>,
    pub continent: Continent,
}

impl Map {
    /// Terrain height under a world position (only x and z are used).
    pub fn get_height(&self, pos: Vec3) -> f32 {
        self.heights.height(pos.xz())
    }

    /// Terrain height and wetness at a world position (only x and z are used).
    pub fn sample(&self, pos: Vec3) -> TerrainSample {
        self.heights.sample(pos.xz())
    }

    /// Apply a terrain edit; the chunks it touches are re-meshed.
    pub fn add_edit(&mut self, edit: TerrainEdit) {
        let (min, max) = edit.bounds();
        self.heights.add_edit(edit);
        let (min, max) = (chunk_coords(min), chunk_coords(max));
        for x in min.x..=max.x {
            for z in min.y..=max.y {
                *self.chunk_edits.entry(I64Vec2::new(x, z)).or_default() += 1;
            }
        }
    }

    fn chunk_edits(&self, chunk: I64Vec2) -> u64 {
        self.chunk_edits.get(&chunk).copied().unwrap_or_default()
    }
}

/// Chunk containing a world position.
fn chunk_coords(p: Vec2) -> I64Vec2 {
    (p / CHUNK_SIZE).floor().as_i64vec2()
}

/// An instance of a specific building at a position
/// Might contain other instance-specific stats in the future (damage, etc)
#[derive(PartialEq, Clone, Component)]
pub struct BuildingInstance {
    pub building: Handle<Building>,
    pub pos: Vec2,
    pub half_extents: Vec2,
    pub entity: Entity,
}

impl KdValue for BuildingInstance {
    type Position = f32;

    fn min_x(&self) -> Self::Position {
        self.pos.x
    }

    fn min_y(&self) -> Self::Position {
        self.pos.y
    }

    fn max_x(&self) -> Self::Position {
        self.pos.x + self.half_extents.x
    }

    fn max_y(&self) -> Self::Position {
        self.pos.y + self.half_extents.y
    }
}

/// Terrain operation of a building or tool, see [`TerrainEdit::from_op`].
#[derive(Clone, Copy, Debug, Deserialize)]
pub enum PatchOp {
    Up,
    Down,
    Flatten,
    Smooth,
}

/// The continent, its height function and gradient texture, being generated on the
/// [`AsyncComputeTaskPool`].
#[derive(Resource)]
struct Generation(Task<(Continent, TerrainHeights, (Image, Vec4))>);

/// Generate the world in the background, so the window stays responsive.
fn start_generation(mut commands: Commands, seed: u32) {
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let start = Instant::now();
        let continent = worldgen::generate(&WorldgenSettings {
            seed: seed.into(),
            ..default()
        });
        let heights = TerrainHeights::new(&continent);
        let gradient = heights.gradient_texture();
        info!(
            "World generated in {:.1?} ({} rivers, {} lakes): {:?}",
            start.elapsed(),
            continent.rivers.len(),
            continent.lakes.len(),
            continent.timings
        );
        (continent, heights, gradient)
    });
    commands.insert_resource(Generation(task));
}

/// Once the world is ready, insert the [`Map`] and enter [`AppState::InGame`].
fn finish_generation(
    mut commands: Commands,
    mut generation: ResMut<Generation>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Some((continent, heights, (gradient, gradient_map))) = check_ready(&mut generation.0)
    else {
        return;
    };
    commands.remove_resource::<Generation>();
    commands.insert_resource(Map {
        material: asset_server.load(MAP_MATERIAL),
        gradient: (images.add(gradient), gradient_map),
        heights,
        chunks: default(),
        chunk_edits: default(),
        entities: KdTree::default(),
        continent,
    });
    next_state.set(AppState::InGame);
}

/// Give the terrain material its gradient texture, once loaded (and again after a hot reload).
fn bind_gradient_texture(map: Res<Map>, mut materials: ResMut<Assets<MapMaterial>>) {
    let (texture, mapping) = &map.gradient;
    let bound = materials
        .get(&map.material)
        .is_none_or(|material| material.extension.gradient.as_ref() == Some(texture));
    if !bound && let Some(mut material) = materials.get_mut(&map.material) {
        material.extension.gradient = Some(texture.clone());
        material.extension.gradient_map = *mapping;
    }
}

/// Marks terrain chunk entities, with their chunk coordinates.
#[derive(Component)]
pub struct IsGround(pub I64Vec2);

/// LOD state of a terrain chunk.
#[derive(Component, Default)]
struct TerrainChunk {
    /// Camera position the current mesh was built for.
    built_for: Option<Vec3>,
    /// Number of edits included in the current mesh.
    built_edits: u64,
}

/// A chunk mesh being built.
#[derive(Component)]
struct ChunkMeshTask {
    task: Task<Mesh>,
    camera: Vec3,
    edits: u64,
}

/// Spawn the chunk entities around the camera when it moves.
fn spawn_chunks(
    mut commands: Commands,
    mut map: ResMut<Map>,
    // `Single` skips the system on frames where the camera did not move.
    camera: Single<&Transform, (With<Camera3d>, Changed<Transform>)>,
) {
    let center = chunk_coords(camera.translation.xz());
    for x in -CHUNK_LOAD_RADIUS..=CHUNK_LOAD_RADIUS {
        for z in -CHUNK_LOAD_RADIUS..=CHUNK_LOAD_RADIUS {
            if x * x + z * z > CHUNK_LOAD_RADIUS * CHUNK_LOAD_RADIUS {
                continue;
            }
            let chunk = center + I64Vec2::new(x, z);
            if map.chunks.contains_key(&chunk) {
                continue;
            }
            let origin = chunk.as_vec2() * CHUNK_SIZE;
            let entity = commands
                .spawn((
                    Name::new(format!("chunk {} {}", chunk.x, chunk.y)),
                    IsGround(chunk),
                    TerrainChunk::default(),
                    Transform::from_xyz(origin.x, 0., origin.y),
                    Visibility::default(),
                ))
                .id();
            map.chunks.insert(chunk, entity);
        }
    }
}

/// Start building meshes for the chunks that need one, nearest and edited chunks first.
fn schedule_chunk_meshes(
    mut commands: Commands,
    map: Res<Map>,
    camera: Single<&Transform, With<Camera3d>>,
    chunks: Query<(Entity, &IsGround, &TerrainChunk), Without<ChunkMeshTask>>,
    running: Query<(), With<ChunkMeshTask>>,
) {
    let available = MAX_MESH_TASKS.saturating_sub(running.iter().len());
    if available == 0 {
        return;
    }
    let camera = camera.translation;
    let mut candidates: Vec<(bool, f32, Entity, I64Vec2)> = chunks
        .iter()
        .filter_map(|(entity, IsGround(chunk), state)| {
            let min = chunk.as_vec2() * CHUNK_SIZE;
            let nearest = camera.xz().clamp(min, min + CHUNK_SIZE);
            let ground = Vec3::new(nearest.x, 0., nearest.y);
            let distance = camera.distance(ground.with_y(map.get_height(ground)));
            let edited = map.chunk_edits(*chunk) > state.built_edits;
            let moved = state.built_for.is_none_or(|built_for| {
                built_for.distance(camera) > (distance * REMESH_MOVE_RATIO).max(REMESH_MIN_MOVE)
            });
            (edited || moved).then_some((edited, distance, entity, *chunk))
        })
        .collect();
    // Edited chunks first (the player is waiting for them), then by distance.
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));

    for (_, _, entity, chunk) in candidates.into_iter().take(available) {
        let heights = map.heights.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let start = Instant::now();
            let mesh = mesh::build_chunk_mesh(&heights, chunk, camera);
            debug!(
                "chunk {chunk} meshed in {:.1?}: {} vertices",
                start.elapsed(),
                mesh.count_vertices()
            );
            mesh
        });
        commands.entity(entity).insert(ChunkMeshTask {
            task,
            camera,
            edits: map.chunk_edits(chunk),
        });
    }
}

/// Swap in the chunk meshes that finished building.
fn finish_chunk_meshes(
    mut commands: Commands,
    map: Res<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(
        Entity,
        &mut TerrainChunk,
        &mut ChunkMeshTask,
        Option<&Mesh3d>,
    )>,
) {
    for (entity, mut state, mut task, mesh3d) in &mut chunks {
        let Some(mesh) = check_ready(&mut task.task) else {
            continue;
        };
        match mesh3d {
            Some(mesh3d) => {
                if let Err(error) = meshes.insert(&mesh3d.0, mesh) {
                    warn!("Could not replace a chunk mesh: {error}");
                }
            }
            None => {
                commands.entity(entity).insert((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(map.material.clone()),
                ));
            }
        }
        state.built_for = Some(task.camera);
        state.built_edits = task.edits;
        commands.entity(entity).remove::<ChunkMeshTask>();
    }
}
