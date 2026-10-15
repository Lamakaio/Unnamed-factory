//! Vegetation on the planet: the flat map's trees, rocks and ground cover (`map::vegetation`:
//! the models, their levels of detail and the species of each biome), placed on the cube
//! faces' cells of the terrain chunks near the camera.
//!
//! - Large things (trees, bushes, rocks) are placed in chunks within [`PLACE_DISTANCE`] and
//!   drawn far as one merged LOD2 mesh per chunk; chunks farther than [`DROP_DISTANCE`] drop
//!   theirs (the terrain's forest colours carry the landscape from orbit).
//! - Near chunks ([`NEAR_DISTANCE`]) swap to the merged LOD1 mesh and place their ground cover;
//!   LOD0 models are spawned around the camera, as on the flat map.
//!
//! Biomes come from the climate's ground weights (forests where the forest weight, through a
//! clearings noise, is high enough; conifers where it is cold), marsh on wetlands (nothing on
//! salt flats), beaches along the ocean and rock on steep slopes. Everything stands upright on the sphere.

use std::{f32::consts::FRAC_PI_2, sync::Arc};

use bevy::{
    platform::collections::HashMap,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use worldgen::{
    Biome,
    noise::{Noise, hash, smoothstep},
    planet_springs::PlanetSpringKind,
    surface::PlanetSurface,
};

use super::{CHUNK_CELLS, PlanetChunk, PlanetWorld, mesh::distance_to_chunk};
use crate::{
    AppState,
    map::vegetation::{
        GROUND_CELL, GROUND_FADE, Instance, Kind, LARGE_CELL, LEVEL_MARGIN, LOD_FADE,
        LOD0_DISTANCE, MAX_TASKS, METERS_TO_WORLD, NEAR_DISTANCE, PATCH_CELL, PATCH_COVER,
        SPAWN_MARGIN, SPAWNS_PER_FRAME, SPECIES, StandIn, Vegetation, VegetationModelsPlugin,
        ground_rules, large_rules, merged_mesh, patch_rules, spawn_lod0, species_index,
    },
};

/// Chunks closer than this to the camera get their large vegetation placed (merged LOD2 mesh);
/// beyond [`DROP_DISTANCE`] they drop it.
const PLACE_DISTANCE: f32 = 260.;
const DROP_DISTANCE: f32 = 320.;
/// Ground this close above sea level is beach.
const BEACH_HEIGHT: f32 = 0.2;
/// Scale of the rock a hillside spring comes out of (on top of [`METERS_TO_WORLD`]), as on the
/// flat map.
const SPRING_ROCK_SCALE: f32 = 5.;

/// Places and draws the planet's vegetation once the models are ready.
pub(super) struct PlanetVegetationPlugin;

impl Plugin for PlanetVegetationPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(VegetationModelsPlugin).add_systems(
            Update,
            (
                start_placement,
                finish_placement,
                update_levels,
                update_details,
            )
                .chain()
                .after(super::camera::apply)
                .run_if(in_state(AppState::InGame))
                .run_if(resource_exists::<Vegetation>),
        );
    }
}

/// What a chunk's background work produced.
struct Placed {
    large: Vec<Instance>,
    far: Option<Mesh>,
}

#[derive(Component)]
struct PlacementTask(Task<Placed>);

/// A near chunk's merged LOD1 mesh and its ground cover, being built.
#[derive(Component)]
struct NearTask(Task<(Option<Mesh>, Vec<Instance>)>);

/// A chunk's placed vegetation and what it currently shows.
#[derive(Component)]
struct ChunkVegetation {
    large: Arc<Vec<Instance>>,
    /// Ground cover, placed while near.
    ground: Arc<Vec<Instance>>,
    far: Option<Entity>,
    near: bool,
    near_mesh: Option<Entity>,
    spawned: HashMap<u32, Vec<Entity>>,
}

impl ChunkVegetation {
    fn despawn_details(&mut self, commands: &mut Commands) {
        for (_, entities) in self.spawned.drain() {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
    }

    fn despawn_all(&mut self, commands: &mut Commands) {
        self.despawn_details(commands);
        for entity in [self.far.take(), self.near_mesh.take()]
            .into_iter()
            .flatten()
        {
            commands.entity(entity).despawn();
        }
    }
}

type Running = Or<(With<PlacementTask>, With<NearTask>)>;

/// Place the large vegetation of chunks coming within reach (nearest first), drop it from
/// chunks out of reach.
fn start_placement(
    mut commands: Commands,
    world: Res<PlanetWorld>,
    vegetation: Res<Vegetation>,
    camera: Single<&Transform, With<Camera3d>>,
    mut chunks: Query<
        (
            Entity,
            &PlanetChunk,
            &Transform,
            Option<&mut ChunkVegetation>,
        ),
        Without<PlacementTask>,
    >,
    running: Query<(), Running>,
) {
    let camera = camera.translation;
    let mut candidates = Vec::new();
    for (entity, chunk, transform, state) in &mut chunks {
        let distance = distance_to_chunk(&world.surface, chunk, camera);
        match state {
            Some(mut state) if distance > DROP_DISTANCE => {
                state.despawn_all(&mut commands);
                commands
                    .entity(entity)
                    .remove::<(ChunkVegetation, NearTask)>();
            }
            None if distance < PLACE_DISTANCE => {
                candidates.push((distance, entity, *chunk, transform.translation));
            }
            _ => {}
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let available = MAX_TASKS.saturating_sub(running.iter().len());
    for (_, entity, chunk, origin) in candidates.into_iter().take(available) {
        let (surface, stand_ins) = (world.surface.clone(), vegetation.lod2.clone());
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let mut large = place(&surface, &chunk, LARGE_CELL, 1, large_rules, false);
            large.extend(spring_rocks(&surface, &chunk));
            let far = merged_mesh(&stand_ins, &large, origin, false);
            Placed { large, far }
        });
        commands.entity(entity).insert(PlacementTask(task));
    }
}

fn finish_placement(
    mut commands: Commands,
    vegetation: Res<Vegetation>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(Entity, &Transform, &mut PlacementTask)>,
) {
    for (entity, transform, mut task) in &mut chunks {
        let Some(placed) = check_ready(&mut task.0) else {
            continue;
        };
        let far = placed.far.map(|mesh| {
            commands
                .spawn((
                    Name::new("planet vegetation (far)"),
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(vegetation.far_material.clone()),
                    Transform::from_translation(transform.translation),
                    Pickable::IGNORE,
                ))
                .id()
        });
        commands
            .entity(entity)
            .remove::<PlacementTask>()
            .insert(ChunkVegetation {
                large: Arc::new(placed.large),
                ground: Arc::new(Vec::new()),
                far,
                near: false,
                near_mesh: None,
                spawned: HashMap::default(),
            });
    }
}

/// Switch chunks between their far (LOD2) and near (LOD1 + ground cover) states by camera
/// distance; the far mesh stays until the near one is built.
fn update_levels(
    mut commands: Commands,
    world: Res<PlanetWorld>,
    vegetation: Res<Vegetation>,
    camera: Single<&Transform, With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(
        Entity,
        &PlanetChunk,
        &Transform,
        &mut ChunkVegetation,
        Option<&mut NearTask>,
    )>,
    running: Query<(), Running>,
) {
    let camera = camera.translation;
    let mut available = MAX_TASKS.saturating_sub(running.iter().len());
    for (entity, chunk, transform, mut state, task) in &mut chunks {
        let distance = distance_to_chunk(&world.surface, chunk, camera);
        let near =
            distance < NEAR_DISTANCE || (state.near && distance < NEAR_DISTANCE + LEVEL_MARGIN);
        let origin = transform.translation;
        match (near, state.near, task) {
            (true, false, None) if available > 0 => {
                available -= 1;
                let (surface, stand_ins, large, chunk) = (
                    world.surface.clone(),
                    vegetation.lod1.clone(),
                    state.large.clone(),
                    *chunk,
                );
                commands
                    .entity(entity)
                    .insert(NearTask(AsyncComputeTaskPool::get().spawn(async move {
                        near_detail(&surface, &chunk, &stand_ins, &large, origin)
                    })));
            }
            (true, false, Some(mut task)) => {
                let Some((mesh, ground)) = check_ready(&mut task.0) else {
                    continue;
                };
                state.near_mesh = mesh.map(|mesh| {
                    commands
                        .spawn((
                            Name::new("planet vegetation (near)"),
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(vegetation.near_material.clone()),
                            Transform::from_translation(origin),
                            Pickable::IGNORE,
                        ))
                        .id()
                });
                if let Some(far) = state.far {
                    commands.entity(far).insert(Visibility::Hidden);
                }
                state.ground = Arc::new(ground);
                state.near = true;
                commands.entity(entity).remove::<NearTask>();
            }
            (false, true, _) | (false, false, Some(_)) => {
                commands.entity(entity).remove::<NearTask>();
                if let Some(near_mesh) = state.near_mesh.take() {
                    commands.entity(near_mesh).despawn();
                }
                state.despawn_details(&mut commands);
                state.ground = Arc::new(Vec::new());
                if let Some(far) = state.far {
                    commands.entity(far).insert(Visibility::Inherited);
                }
                state.near = false;
            }
            _ => {}
        }
    }
}

/// A near chunk's merged LOD1 mesh and ground cover (tall grass patches included).
fn near_detail(
    surface: &PlanetSurface,
    chunk: &PlanetChunk,
    stand_ins: &[StandIn],
    large: &[Instance],
    origin: Vec3,
) -> (Option<Mesh>, Vec<Instance>) {
    let mesh = merged_mesh(stand_ins, large, origin, true);
    let mut ground = place(surface, chunk, GROUND_CELL, 2, ground_rules, false);
    ground.extend(place(surface, chunk, PATCH_CELL, 3, patch_rules, true));
    (mesh, ground)
}

/// Spawn the LOD0 models (and ground cover) of near chunks around the camera and despawn the
/// ones left behind, as on the flat map (`map::vegetation::update_details`).
fn update_details(
    mut commands: Commands,
    world: Res<PlanetWorld>,
    vegetation: Res<Vegetation>,
    camera: Single<&Transform, With<Camera3d>>,
    mut chunks: Query<(&PlanetChunk, &mut ChunkVegetation)>,
    mut last: Local<Option<(Vec3, usize)>>,
    mut pending: Local<bool>,
) {
    let camera = camera.translation;
    let near_chunks = chunks.iter().filter(|(_, state)| state.near).count();
    if !*pending
        && last
            .is_some_and(|(position, near)| position.distance(camera) < 0.5 && near == near_chunks)
    {
        return;
    }
    *last = Some((camera, near_chunks));
    *pending = false;
    let mut budget = SPAWNS_PER_FRAME;
    let reach = LOD0_DISTANCE.max(GROUND_FADE.1) + 3. * SPAWN_MARGIN;
    for (chunk, mut state) in &mut chunks {
        if !state.near {
            continue;
        }
        if distance_to_chunk(&world.surface, chunk, camera) > reach {
            state.despawn_details(&mut commands);
            continue;
        }
        let state = &mut *state;
        let instances = state
            .large
            .iter()
            .map(|instance| (instance, LOD0_DISTANCE + LOD_FADE, false))
            .chain(
                state
                    .ground
                    .iter()
                    .map(|instance| (instance, GROUND_FADE.1, true)),
            );
        for (index, (instance, visible, ground)) in instances.enumerate() {
            let index = index as u32;
            let distance = instance.position.distance(camera);
            let spawned = state.spawned.contains_key(&index);
            if spawned && distance > visible + 2. * SPAWN_MARGIN {
                for entity in state.spawned.remove(&index).unwrap_or_default() {
                    commands.entity(entity).despawn();
                }
            } else if !spawned && distance < visible + SPAWN_MARGIN {
                if budget == 0 {
                    *pending = true;
                    continue;
                }
                budget -= 1;
                let entities = spawn_lod0(&mut commands, &vegetation, instance, ground);
                state.spawned.insert(index, entities);
            }
        }
    }
}

/// Instances of one jittered grid (`spacing` world units apart) over a chunk, as on the flat
/// map (`map::vegetation::Placement::place`), on the face's cells.
fn place(
    surface: &PlanetSurface,
    chunk: &PlanetChunk,
    spacing: f32,
    salt: u32,
    rules: fn(Biome) -> &'static [(&'static str, f32)],
    patchy: bool,
) -> Vec<Instance> {
    // About one world unit per cell (the equiangular cells vary little in size).
    let cell_length = surface.radius * FRAC_PI_2 / surface.n as f32;
    let per_chunk = (CHUNK_CELLS as f32 * cell_length / spacing).round() as i64;
    let step = CHUNK_CELLS as f32 / per_chunk as f32;
    let seed = surface.seed() ^ salt.wrapping_mul(0x9E37_79B9);
    let face_seed = seed ^ (chunk.face as u32 + 1).wrapping_mul(0x85EB_CA6B);
    let clearings = Noise::new(seed ^ 0xC1EA);
    let patches = Noise::new(seed ^ 0x7A11);
    let random = |x: i64, z: i64, k: u32| {
        hash(face_seed.wrapping_add(k), x as u32, z as u32) as f32 / u32::MAX as f32
    };
    let first = (chunk.min / step).round().as_i64vec2();
    let mut instances = Vec::new();
    for gz in 0..per_chunk {
        for gx in 0..per_chunk {
            let (x, z) = (first.x + gx, first.y + gz);
            let p = Vec2::new(x as f32 + random(x, z, 1), z as f32 + random(x, z, 2)) * step - 0.5;
            let radial = surface.direction(chunk.face, p);
            let world = radial * surface.radius;
            // Patches (tall grass) cover about `PATCH_COVER` of the ground.
            if patchy && patches.fbm3(world / 5., 3) < 0.55 - 1.4 * PATCH_COVER {
                continue;
            }
            // Forests have clearings and denser groves.
            let groves = smoothstep(-0.45, 0.35, clearings.fbm3(world / 14., 3));
            let table = rules(biome(surface, chunk.face, p, cell_length, groves));
            if table.is_empty() {
                continue;
            }
            let density = 0.45 + 0.75 * groves;
            let mut roll = random(x, z, 3) / density;
            let Some(&(name, _)) = table.iter().find(|(_, probability)| {
                roll -= probability;
                roll < 0.
            }) else {
                continue;
            };
            let species = species_index(name);
            let kind = SPECIES[species as usize].kind;
            let height = surface.height(chunk.face, p);
            // (Land below sea level is fine away from the ocean.)
            let lowest = if surface.ocean.bilinear(chunk.face, p) < 0.02 {
                f32::MIN
            } else if kind == Kind::Rock {
                0.03
            } else {
                0.12
            };
            if height < lowest
                || surface.wetness(chunk.face, p) > 0.02
                || 1. - normal(surface, chunk.face, p).dot(radial) > kind.max_steepness()
            {
                continue;
            }
            let (low, high) = SPECIES[species as usize].scale;
            let scale = METERS_TO_WORLD * (low + (high - low) * random(x, z, 4));
            // Rocks sink a little into the ground.
            let sink = if kind == Kind::Rock { 0.25 * scale } else { 0. };
            instances.push(Instance {
                species,
                position: radial * (surface.radius + height - sink),
                rotation: Quat::from_rotation_arc(Vec3::Y, radial)
                    * Quat::from_rotation_y(random(x, z, 5) * std::f32::consts::TAU),
                scale,
            });
        }
    }
    instances
}

/// Rock outcrops the hillside springs of a chunk spout from (`map::vegetation::spring_rocks`):
/// a tall rock right behind the spout (hiding its top) and boulders on both sides.
fn spring_rocks(surface: &PlanetSurface, chunk: &PlanetChunk) -> Vec<Instance> {
    let (min, max) = (chunk.min - 0.5, chunk.min - 0.5 + CHUNK_CELLS as f32);
    let mut instances = Vec::new();
    for spring in surface.springs() {
        let (face, p) = surface.face_coords(spring.direction);
        if !matches!(spring.kind, PlanetSpringKind::Hillside)
            || face != chunk.face
            || p.cmplt(min).any()
            || p.cmpge(max).any()
        {
            continue;
        }
        let tall = ["rock_tall_a", "rock_tall_b", "rock_tall_c", "rock_tall_d"]
            [(p.x as usize + p.y as usize) % 4];
        let flank = spring.width * 0.4 + 0.3;
        for (k, (name, offset, scale)) in [
            (tall, Vec2::new(-0.45, 0.), SPRING_ROCK_SCALE),
            ("boulder_a", Vec2::new(-0.15, flank), SPRING_ROCK_SCALE * 0.55),
            ("boulder_c", Vec2::new(-0.2, -flank), SPRING_ROCK_SCALE * 0.45),
        ]
        .into_iter()
        .enumerate()
        {
            let direction = spring.direction_of(offset, surface.radius);
            let height = surface.height_at(direction) - 0.08;
            // Upright, facing upstream, turned a little.
            let upstream = (-spring.downstream).reject_from(direction).normalize();
            let facing = Quat::from_mat3(&Mat3::from_cols(upstream, direction, upstream.cross(direction)));
            instances.push(Instance {
                species: species_index(name),
                position: direction * (surface.radius + height),
                rotation: facing * Quat::from_rotation_y(((p.x + k as f32) * 7.3).sin() * 0.5),
                scale: METERS_TO_WORLD * scale,
            });
        }
    }
    instances
}

/// The flat map's biome for a point of a face, from the climate's ground weights, the
/// height (beaches) and the slope of the cells (rock); `groves` (0..1) decides forests where
/// the forest weight is middling. Sea and lakes have no vegetation.
fn biome(surface: &PlanetSurface, face: usize, p: Vec2, cell_length: f32, groves: f32) -> Biome {
    let height = surface.heights.bilinear(face, p);
    let ocean = surface.ocean.bilinear(face, p);
    if ocean > 0.5 || height <= 0. && ocean > 0.02 || surface.lake_depth.bilinear(face, p) > 0.02 {
        return Biome::Sea;
    }
    // (Beaches only along the ocean: dry basins below sea level are land.)
    if height < BEACH_HEIGHT && ocean > 0.02 {
        return Biome::Beach;
    }
    // Wetlands: marsh, or bare salt flats.
    let marsh = surface.marsh.bilinear(face, p);
    if marsh > 0.5 {
        return Biome::Marsh;
    }
    if marsh < -0.5 {
        return Biome::Sea;
    }
    let slope = Vec2::new(
        surface.heights.bilinear(face, p + Vec2::X) - surface.heights.bilinear(face, p - Vec2::X),
        surface.heights.bilinear(face, p + Vec2::Y) - surface.heights.bilinear(face, p - Vec2::Y),
    )
    .length()
        / (2. * cell_length);
    let Some(weights) = &surface.weights else {
        return Biome::Meadow;
    };
    let [dryness, forest, desert, snow, cold] = weights.each_ref().map(|w| w.bilinear(face, p));
    if snow > 0.5 {
        Biome::Snow
    } else if slope > 0.9 {
        Biome::Rock
    } else if desert > 0.5 {
        Biome::Desert
    } else if forest * (0.6 + 0.8 * groves) > 0.45 {
        if cold > 0.5 {
            Biome::ConiferForest
        } else {
            Biome::DeciduousForest
        }
    } else if cold > 0.6 {
        Biome::Alpine
    } else if dryness > 0.65 {
        Biome::Steppe
    } else {
        Biome::Meadow
    }
}

/// The surface's normal at a point of a face (outward).
fn normal(surface: &PlanetSurface, face: usize, p: Vec2) -> Vec3 {
    let e = 0.3;
    let dx = surface.position(face, p + Vec2::X * e) - surface.position(face, p - Vec2::X * e);
    let dy = surface.position(face, p + Vec2::Y * e) - surface.position(face, p - Vec2::Y * e);
    let normal = dx.cross(dy).normalize_or_zero();
    if normal.dot(surface.direction(face, p)) < 0. {
        -normal
    } else {
        normal
    }
}
