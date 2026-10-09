//! Vegetation and rocks, placed procedurally per terrain chunk from the biomes.
//!
//! Every chunk gets a deterministic list of instances (species, position, rotation, scale)
//! from jittered grids: large things (trees, bushes, rocks, dead wood) and ground cover
//! (grass, flowers, ferns), with per-biome probabilities, forest clearings from noise, and
//! nothing on water, too steep ground or edited ground (building foundations).
//!
//! Levels of detail (models built by `tools/nature`):
//! - LOD0: the textured model, within [`LOD0_DISTANCE`] of the camera (dithered cross-fade);
//! - LOD1: a low-poly stand-in (~120 triangles, flat colours), in chunks near the camera;
//! - LOD2: an extremely low-poly stand-in (~25 triangles), merged into one mesh per chunk for
//!   every other chunk. It never fades: trees don't disappear in the distance.
//!
//! Ground cover only has LOD0 and fades out. A chunk's vegetation is rebuilt after an edit.

use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::VisibilityRange,
    gltf::{Gltf, GltfMaterial, GltfMesh},
    light::NotShadowCaster,
    math::I64Vec2,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    pbr::gltf::standard_material_from_gltf_material,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use worldgen::{
    Biome,
    noise::{Noise, smoothstep},
};

use super::{CHUNK_SIZE, IsGround, Map, heights::TerrainHeights};
use crate::{
    AppState,
    shaders::{ToonMaterial, toon},
};

/// Model units (metres) to world units: one world unit is 20 m (buildings use the same scale).
pub const METERS_TO_WORLD: f32 = 0.05;

/// LOD0 (textured) models show within this distance of the camera, cross-fading over
/// ± [`LOD_FADE`].
const LOD0_DISTANCE: f32 = 30.;
const LOD_FADE: f32 = 3.;
/// Ground cover fades out between these distances.
const GROUND_FADE: (f32, f32) = (22., 30.);
/// Chunk distance (camera to the chunk, world units) under which chunks show their own
/// instances instead of the merged LOD2 mesh, and under which they also have LOD0 models and
/// ground cover. Leaving takes a margin more, against flickering.
const NEAR_DISTANCE: f32 = 100.;
const DETAIL_DISTANCE: f32 = 40.;
const LEVEL_MARGIN: f32 = 15.;
/// Placement grid cells (world units) for large things and for ground cover.
const LARGE_CELL: f32 = 0.9;
const GROUND_CELL: f32 = 0.35;
/// Chunks whose vegetation is being placed at once, and chunks changing level per frame.
const MAX_TASKS: usize = 4;
const LEVEL_CHANGES_PER_FRAME: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Tree,
    Bush,
    Rock,
    Wood,
    Ground,
}

impl Kind {
    /// Kinds with low-poly stand-ins (LOD1 and LOD2).
    fn has_stand_ins(self) -> bool {
        self != Kind::Ground
    }

    /// Steepest ground (1 - normal.y) it grows or lies on.
    fn max_steepness(self) -> f32 {
        match self {
            Kind::Tree | Kind::Ground => 0.25,
            Kind::Bush | Kind::Wood => 0.35,
            Kind::Rock => 0.7,
        }
    }
}

struct Species {
    /// Model file prefix in `assets/models/nature`.
    name: &'static str,
    kind: Kind,
    /// Random scale range (on top of [`METERS_TO_WORLD`]).
    scale: (f32, f32),
}

const SPECIES: &[Species] = &[
    Species { name: "broadleaf_round", kind: Kind::Tree, scale: (0.8, 1.2) },
    Species { name: "broadleaf_tall", kind: Kind::Tree, scale: (0.6, 0.9) },
    Species { name: "broadleaf_wide", kind: Kind::Tree, scale: (0.7, 1.0) },
    Species { name: "cypress", kind: Kind::Tree, scale: (0.9, 1.3) },
    Species { name: "pine", kind: Kind::Tree, scale: (0.75, 1.15) },
    Species { name: "dead_pine", kind: Kind::Tree, scale: (0.8, 1.2) },
    Species { name: "bush_round", kind: Kind::Bush, scale: (0.8, 1.3) },
    Species { name: "bush_flowers", kind: Kind::Bush, scale: (0.8, 1.2) },
    Species { name: "bush_leafy", kind: Kind::Bush, scale: (0.8, 1.3) },
    Species { name: "rock_a", kind: Kind::Rock, scale: (0.8, 2.0) },
    Species { name: "rock_b", kind: Kind::Rock, scale: (0.8, 2.0) },
    Species { name: "rock_c", kind: Kind::Rock, scale: (0.8, 2.0) },
    Species { name: "rock_d", kind: Kind::Rock, scale: (0.8, 2.0) },
    Species { name: "rock_large_a", kind: Kind::Rock, scale: (1.0, 3.0) },
    Species { name: "rock_large_b", kind: Kind::Rock, scale: (1.0, 3.0) },
    Species { name: "rock_small_a", kind: Kind::Rock, scale: (1.0, 2.5) },
    Species { name: "rock_small_b", kind: Kind::Rock, scale: (1.0, 2.5) },
    Species { name: "rock_small_c", kind: Kind::Rock, scale: (1.0, 2.5) },
    Species { name: "cliff", kind: Kind::Rock, scale: (0.8, 1.5) },
    Species { name: "logs", kind: Kind::Wood, scale: (0.8, 1.2) },
    Species { name: "stump", kind: Kind::Wood, scale: (0.8, 1.4) },
    Species { name: "fern", kind: Kind::Ground, scale: (0.8, 1.4) },
    Species { name: "flowers_a", kind: Kind::Ground, scale: (0.8, 1.3) },
    Species { name: "flowers_b", kind: Kind::Ground, scale: (0.8, 1.3) },
    Species { name: "flowers_c", kind: Kind::Ground, scale: (0.8, 1.3) },
    Species { name: "grass_a", kind: Kind::Ground, scale: (0.8, 1.4) },
    Species { name: "grass_b", kind: Kind::Ground, scale: (0.8, 1.4) },
];

/// Probability per large placement cell of each species, by biome.
fn large_rules(biome: Biome) -> &'static [(&'static str, f32)] {
    match biome {
        Biome::DeciduousForest => &[
            ("broadleaf_round", 0.34),
            ("broadleaf_wide", 0.2),
            ("broadleaf_tall", 0.14),
            ("bush_leafy", 0.07),
            ("bush_round", 0.04),
            ("logs", 0.01),
            ("stump", 0.01),
            ("rock_b", 0.01),
        ],
        Biome::ConiferForest => &[
            ("pine", 0.68),
            ("dead_pine", 0.02),
            ("broadleaf_tall", 0.04),
            ("bush_leafy", 0.04),
            ("stump", 0.02),
            ("logs", 0.01),
            ("rock_c", 0.02),
        ],
        Biome::Meadow => &[
            ("broadleaf_round", 0.025),
            ("broadleaf_wide", 0.012),
            ("bush_round", 0.035),
            ("bush_flowers", 0.03),
            ("rock_d", 0.008),
            ("rock_small_b", 0.01),
        ],
        Biome::Steppe => &[
            ("cypress", 0.006),
            ("bush_round", 0.012),
            ("rock_a", 0.008),
            ("rock_small_a", 0.012),
        ],
        Biome::Marsh => &[("bush_leafy", 0.05), ("broadleaf_tall", 0.025), ("logs", 0.008)],
        Biome::Alpine => &[
            ("pine", 0.04),
            ("dead_pine", 0.015),
            ("rock_c", 0.03),
            ("rock_large_a", 0.025),
            ("rock_small_c", 0.03),
            ("stump", 0.008),
        ],
        Biome::Rock => &[
            ("rock_large_a", 0.05),
            ("rock_large_b", 0.04),
            ("rock_a", 0.04),
            ("cliff", 0.012),
        ],
        Biome::Snow => &[("rock_large_b", 0.015), ("rock_c", 0.01)],
        Biome::Beach => &[("rock_small_a", 0.008), ("logs", 0.003)],
        Biome::Sea | Biome::Lake => &[],
    }
}

/// Probability per ground cover cell of each species, by biome.
fn ground_rules(biome: Biome) -> &'static [(&'static str, f32)] {
    match biome {
        Biome::DeciduousForest => &[("fern", 0.2)],
        Biome::ConiferForest => &[("fern", 0.12)],
        Biome::Meadow => &[
            ("grass_a", 0.06),
            ("grass_b", 0.06),
            ("flowers_a", 0.03),
            ("flowers_b", 0.03),
            ("flowers_c", 0.025),
        ],
        Biome::Steppe => &[("grass_a", 0.07), ("grass_b", 0.05)],
        Biome::Marsh => &[("grass_b", 0.14), ("fern", 0.04)],
        Biome::Alpine => &[("grass_a", 0.04), ("flowers_b", 0.015)],
        _ => &[],
    }
}

fn species_index(name: &str) -> u16 {
    SPECIES
        .iter()
        .position(|s| s.name == name)
        .unwrap_or_else(|| panic!("unknown species {name}")) as u16
}

pub struct VegetationPlugin;

impl Plugin for VegetationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_models).add_systems(
            Update,
            (
                prepare_models.run_if(not(resource_exists::<Vegetation>)),
                (start_placement, finish_placement, update_levels)
                    .chain()
                    .run_if(in_state(AppState::InGame))
                    .run_if(resource_exists::<Vegetation>),
            ),
        );
    }
}

/// The nature models being loaded: per species, the glTF of each level of detail.
#[derive(Resource)]
struct VegetationModels(Vec<Vec<Handle<Gltf>>>);

fn load_models(mut commands: Commands, asset_server: Res<AssetServer>) {
    let models = SPECIES
        .iter()
        .map(|species| {
            let levels = if species.kind.has_stand_ins() { 3 } else { 1 };
            (0..levels)
                .map(|lod| asset_server.load(format!("models/nature/{}_lod{lod}.glb", species.name)))
                .collect()
        })
        .collect();
    commands.insert_resource(VegetationModels(models));
}

/// A mesh with its material.
type Part = (Handle<Mesh>, Handle<ToonMaterial>);

/// Geometry of a LOD2 stand-in, merged into the chunks' far meshes.
#[derive(Default)]
struct StandIn {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// The loaded vegetation models, ready to place.
#[derive(Resource)]
struct Vegetation {
    lod0: Vec<Vec<Part>>,
    lod1: Vec<Vec<Part>>,
    lod2: Arc<Vec<StandIn>>,
    far_material: Handle<ToonMaterial>,
}

/// Once every model is loaded, convert their materials to toon materials and extract the LOD2
/// geometry.
fn prepare_models(
    mut commands: Commands,
    models: Option<Res<VegetationModels>>,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    gltf_materials: Res<Assets<GltfMaterial>>,
    meshes: Res<Assets<Mesh>>,
    mut toon_materials: ResMut<Assets<ToonMaterial>>,
) {
    let Some(models) = models else {
        return;
    };
    if !models
        .0
        .iter()
        .flatten()
        .all(|handle| asset_server.is_loaded_with_dependencies(handle))
    {
        return;
    }
    let mut parts = |handle: &Handle<Gltf>| -> Vec<Part> {
        let Some(gltf) = gltfs.get(handle) else {
            return Vec::new();
        };
        gltf.meshes
            .iter()
            .filter_map(|mesh| gltf_meshes.get(mesh))
            .flat_map(|mesh| &mesh.primitives)
            .map(|primitive| {
                let base = primitive
                    .material
                    .as_ref()
                    .and_then(|material| gltf_materials.get(material))
                    .map(standard_material_from_gltf_material)
                    .unwrap_or_default();
                (primitive.mesh.clone(), toon_materials.add(toon(&base)))
            })
            .collect()
    };
    let lod0 = models.0.iter().map(|levels| parts(&levels[0])).collect();
    let lod1 = models
        .0
        .iter()
        .map(|levels| levels.get(1).map(&mut parts).unwrap_or_default())
        .collect();
    let lod2 = models
        .0
        .iter()
        .map(|levels| {
            let mut stand_in = StandIn::default();
            let Some(gltf) = levels.get(2).and_then(|handle| gltfs.get(handle)) else {
                return stand_in;
            };
            for primitive in gltf
                .meshes
                .iter()
                .filter_map(|mesh| gltf_meshes.get(mesh))
                .flat_map(|mesh| &mesh.primitives)
            {
                if let Some(mesh) = meshes.get(&primitive.mesh) {
                    append_mesh(&mut stand_in, mesh);
                }
            }
            stand_in
        })
        .collect();
    let far_material = toon_materials.add(toon(&StandardMaterial {
        perceptual_roughness: 1.,
        ..default()
    }));
    commands.insert_resource(Vegetation {
        lod0,
        lod1,
        lod2: Arc::new(lod2),
        far_material,
    });
    info!("Vegetation models ready");
}

/// Append a mesh's triangles (positions, normals, colours) to a stand-in.
fn append_mesh(stand_in: &mut StandIn, mesh: &Mesh) {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return;
    };
    let first = stand_in.positions.len() as u32;
    stand_in
        .positions
        .extend(positions.iter().map(|p| Vec3::from_array(*p)));
    match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(normals)) => stand_in
            .normals
            .extend(normals.iter().map(|n| Vec3::from_array(*n))),
        _ => stand_in
            .normals
            .extend(std::iter::repeat_n(Vec3::Y, positions.len())),
    }
    match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(colors)) => stand_in.colors.extend(colors),
        _ => stand_in
            .colors
            .extend(std::iter::repeat_n([0.5, 0.5, 0.5, 1.], positions.len())),
    }
    match mesh.indices() {
        Some(indices) => stand_in
            .indices
            .extend(indices.iter().map(|i| first + i as u32)),
        None => stand_in
            .indices
            .extend((0..positions.len() as u32).map(|i| first + i)),
    }
}

/// One placed tree, bush, rock...
#[derive(Clone, Copy)]
struct Instance {
    species: u16,
    position: Vec3,
    yaw: f32,
    scale: f32,
}

impl Instance {
    fn transform(&self) -> Transform {
        Transform::from_translation(self.position)
            .with_rotation(Quat::from_rotation_y(self.yaw))
            .with_scale(Vec3::splat(self.scale))
    }
}

/// What placement needs from the world, cheap to send to a task.
#[derive(Clone)]
struct Placement {
    heights: TerrainHeights,
    biome: Arc<Vec<Biome>>,
    size: usize,
    cell_size: f32,
    seed: u32,
}

impl Placement {
    fn biome(&self, p: Vec2) -> Biome {
        let g = (p / self.cell_size + (self.size / 2) as f32).round();
        if g.x < 0. || g.y < 0. || g.x >= self.size as f32 || g.y >= self.size as f32 {
            return Biome::Sea;
        }
        self.biome[g.y as usize * self.size + g.x as usize]
    }

    /// Instances of one grid (`cell` spacing) over a chunk.
    fn place(
        &self,
        chunk: I64Vec2,
        cell: f32,
        salt: u32,
        rules: fn(Biome) -> &'static [(&'static str, f32)],
    ) -> Vec<Instance> {
        let per_chunk = (CHUNK_SIZE / cell).round() as i64;
        let cell = CHUNK_SIZE / per_chunk as f32;
        let seed = self.seed ^ salt.wrapping_mul(0x9E37_79B9);
        let clearings = Noise::new(seed ^ 0xC1EA);
        let random = |x: i64, z: i64, k: u32| {
            worldgen::noise::hash(seed.wrapping_add(k), x as u32, z as u32) as f32 / u32::MAX as f32
        };
        let mut instances = Vec::new();
        for gz in 0..per_chunk {
            for gx in 0..per_chunk {
                let (x, z) = (chunk.x * per_chunk + gx, chunk.y * per_chunk + gz);
                let p = Vec2::new(x as f32 + random(x, z, 1), z as f32 + random(x, z, 2)) * cell;
                let table = rules(self.biome(p));
                if table.is_empty() {
                    continue;
                }
                // Forests have clearings and denser groves.
                let density = 0.45 + 0.75 * smoothstep(-0.45, 0.35, clearings.fbm(p / 14., 3));
                let mut roll = random(x, z, 3) / density;
                let Some(&(name, _)) = table.iter().find(|(_, probability)| {
                    roll -= probability;
                    roll < 0.
                }) else {
                    continue;
                };
                let species = species_index(name);
                let kind = SPECIES[species as usize].kind;
                let sample = self.heights.sample(p);
                let steepness = 1. - self.heights.normal(p).y;
                let lowest = if kind == Kind::Rock { 0.03 } else { 0.12 };
                if sample.wetness > 0.02
                    || sample.height < lowest
                    || steepness > kind.max_steepness()
                    || self.heights.edited(p)
                {
                    continue;
                }
                let (low, high) = SPECIES[species as usize].scale;
                let scale = METERS_TO_WORLD * (low + (high - low) * random(x, z, 4));
                // Rocks sink a little into the ground.
                let sink = if kind == Kind::Rock { 0.25 * scale } else { 0. };
                instances.push(Instance {
                    species,
                    position: Vec3::new(p.x, sample.height - sink, p.y),
                    yaw: random(x, z, 5) * std::f32::consts::TAU,
                    scale,
                });
            }
        }
        instances
    }
}

/// The far mesh of a chunk: every LOD2 stand-in merged, relative to the chunk origin.
fn far_mesh(stand_ins: &[StandIn], instances: &[Instance], origin: Vec3) -> Option<Mesh> {
    let mut merged = StandIn::default();
    for instance in instances {
        let stand_in = &stand_ins[instance.species as usize];
        if stand_in.positions.is_empty() {
            continue;
        }
        let rotation = Quat::from_rotation_y(instance.yaw);
        let first = merged.positions.len() as u32;
        merged.positions.extend(
            stand_in
                .positions
                .iter()
                .map(|p| rotation * (*p * instance.scale) + instance.position - origin),
        );
        merged
            .normals
            .extend(stand_in.normals.iter().map(|n| rotation * *n));
        merged.colors.extend(&stand_in.colors);
        merged
            .indices
            .extend(stand_in.indices.iter().map(|i| first + i));
    }
    (!merged.positions.is_empty()).then(|| {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, merged.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, merged.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, merged.colors)
            .with_inserted_indices(Indices::U32(merged.indices))
    })
}

/// Placement result of a chunk.
struct Placed {
    large: Vec<Instance>,
    ground: Vec<Instance>,
    far: Option<Mesh>,
}

/// A chunk's vegetation being placed.
#[derive(Component)]
struct PlacementTask {
    task: Task<Placed>,
    edits: u64,
}

/// A chunk's placed vegetation and what it currently shows.
#[derive(Component)]
struct ChunkVegetation {
    large: Arc<Vec<Instance>>,
    ground: Arc<Vec<Instance>>,
    /// Merged LOD2 mesh, shown while the chunk is far.
    far: Option<Entity>,
    /// 0: far mesh only; 1: LOD1 instances; 2: also LOD0 models and ground cover.
    level: u8,
    /// Parents of the LOD1 instances (level 1+) and of the LOD0 / ground cover (level 2).
    near: Option<Entity>,
    detail: Option<Entity>,
    /// Number of edits included.
    edits: u64,
}

/// Place vegetation in chunks that have none (or whose ground was edited since).
fn start_placement(
    mut commands: Commands,
    map: Res<Map>,
    vegetation: Res<Vegetation>,
    chunks: Query<(Entity, &IsGround, Option<&ChunkVegetation>), Without<PlacementTask>>,
    running: Query<(), With<PlacementTask>>,
    mut placement: Local<Option<Placement>>,
) {
    let placement = placement.get_or_insert_with(|| Placement {
        heights: map.heights.clone(),
        biome: Arc::new(map.continent.biome.clone()),
        size: map.continent.size(),
        cell_size: map.continent.settings.cell_size,
        seed: map.continent.settings.seed as u32,
    });
    // Edits change the heights: keep the snapshot current.
    placement.heights = map.heights.clone();
    let mut available = MAX_TASKS.saturating_sub(running.iter().len());
    for (entity, IsGround(chunk), current) in &chunks {
        if available == 0 {
            break;
        }
        let edits = map.chunk_edits(*chunk);
        if current.is_some_and(|current| current.edits >= edits) {
            continue;
        }
        let (placement, stand_ins, chunk) =
            (placement.clone(), vegetation.lod2.clone(), *chunk);
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let large = placement.place(chunk, LARGE_CELL, 1, large_rules);
            let ground = placement.place(chunk, GROUND_CELL, 2, ground_rules);
            let origin = (chunk.as_vec2() * CHUNK_SIZE).extend(0.).xzy();
            let far = far_mesh(&stand_ins, &large, origin);
            Placed { large, ground, far }
        });
        commands.entity(entity).insert(PlacementTask { task, edits });
        available -= 1;
    }
}

/// Swap in finished placements (replacing the chunk's previous vegetation, if any).
fn finish_placement(
    mut commands: Commands,
    vegetation: Res<Vegetation>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(Entity, &IsGround, &mut PlacementTask, Option<&ChunkVegetation>)>,
) {
    for (entity, IsGround(chunk), mut task, previous) in &mut chunks {
        let Some(placed) = check_ready(&mut task.task) else {
            continue;
        };
        if let Some(previous) = previous {
            for old in [previous.far, previous.near, previous.detail].into_iter().flatten() {
                commands.entity(old).despawn();
            }
        }
        let origin = chunk.as_vec2() * CHUNK_SIZE;
        let far = placed.far.map(|mesh| {
            commands
                .spawn((
                    Name::new("vegetation (far)"),
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(vegetation.far_material.clone()),
                    Transform::from_xyz(origin.x, 0., origin.y),
                    Pickable::IGNORE,
                ))
                .id()
        });
        commands.entity(entity).remove::<PlacementTask>().insert(ChunkVegetation {
            large: Arc::new(placed.large),
            ground: Arc::new(placed.ground),
            far,
            level: 0,
            near: None,
            detail: None,
            edits: task.edits,
        });
    }
}

/// Switch chunks between far mesh, LOD1 instances, and full detail, by camera distance.
fn update_levels(
    mut commands: Commands,
    map: Res<Map>,
    vegetation: Res<Vegetation>,
    camera: Single<&Transform, With<Camera3d>>,
    mut chunks: Query<(&IsGround, &mut ChunkVegetation)>,
) {
    let camera = camera.translation;
    let mut changes = 0;
    for (IsGround(chunk), mut state) in &mut chunks {
        let min = chunk.as_vec2() * CHUNK_SIZE;
        let nearest = camera.xz().clamp(min, min + CHUNK_SIZE);
        let ground = Vec3::new(nearest.x, 0., nearest.y);
        let distance = camera.distance(ground.with_y(map.get_height(ground)));
        let level = |enter: f32, current: bool| {
            distance < enter || (current && distance < enter + LEVEL_MARGIN)
        };
        let target = if level(DETAIL_DISTANCE, state.level >= 2) {
            2
        } else if level(NEAR_DISTANCE, state.level >= 1) {
            1
        } else {
            0
        };
        if target == state.level || changes >= LEVEL_CHANGES_PER_FRAME {
            continue;
        }
        changes += 1;
        // LOD1 instances.
        if target >= 1 && state.near.is_none() {
            let parent = commands
                .spawn((Name::new("vegetation (near)"), Transform::default(), Visibility::default()))
                .id();
            for instance in state.large.iter() {
                for (mesh, material) in &vegetation.lod1[instance.species as usize] {
                    commands.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        instance.transform(),
                        VisibilityRange {
                            start_margin: LOD0_DISTANCE - LOD_FADE..LOD0_DISTANCE + LOD_FADE,
                            end_margin: f32::MAX..f32::MAX,
                            use_aabb: false,
                        },
                        Pickable::IGNORE,
                        ChildOf(parent),
                    ));
                }
            }
            state.near = Some(parent);
        } else if target == 0 && let Some(near) = state.near.take() {
            commands.entity(near).despawn();
        }
        // LOD0 models and ground cover.
        if target == 2 && state.detail.is_none() {
            let parent = commands
                .spawn((Name::new("vegetation (detail)"), Transform::default(), Visibility::default()))
                .id();
            for instance in state.large.iter() {
                for (mesh, material) in &vegetation.lod0[instance.species as usize] {
                    commands.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        instance.transform(),
                        VisibilityRange {
                            start_margin: 0.0..0.0,
                            end_margin: LOD0_DISTANCE - LOD_FADE..LOD0_DISTANCE + LOD_FADE,
                            use_aabb: false,
                        },
                        Pickable::IGNORE,
                        ChildOf(parent),
                    ));
                }
            }
            for instance in state.ground.iter() {
                for (mesh, material) in &vegetation.lod0[instance.species as usize] {
                    commands.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        instance.transform(),
                        VisibilityRange {
                            start_margin: 0.0..0.0,
                            end_margin: GROUND_FADE.0..GROUND_FADE.1,
                            use_aabb: false,
                        },
                        NotShadowCaster,
                        Pickable::IGNORE,
                        ChildOf(parent),
                    ));
                }
            }
            state.detail = Some(parent);
        } else if target < 2 && let Some(detail) = state.detail.take() {
            commands.entity(detail).despawn();
        }
        // The far mesh shows only while the chunk has no instances.
        if let Some(far) = state.far {
            commands.entity(far).insert(if target == 0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        state.level = target;
    }
}
