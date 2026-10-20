//! Nature models (trees, bushes, rocks, ground cover; built by `tools/nature`), their levels
//! of detail and their species: what grows in each biome (first written for the flat map; the
//! planet places them, see `vegetation`).
//!
//! Levels of detail, drawn with few entities:
//! - LOD2: an extremely low-poly stand-in (~25 triangles); all of a far chunk's stand-ins are
//!   merged into one mesh. It never fades: trees don't disappear in the distance.
//! - LOD1: a low-poly stand-in (~120 triangles); a near chunk's are merged into one mesh too.
//!   Each vertex carries its object's origin, so the shader fades every object out where its
//!   LOD0 model fades in (`LodToonMaterial`, the same dither as Bevy's visibility ranges).
//! - LOD0: the textured model, spawned as an entity only around the camera (within
//!   [`LOD0_DISTANCE`]), with a `VisibilityRange` cross-fade. Ground cover only has LOD0.

use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::VisibilityRange,
    gltf::{Gltf, GltfMaterial, GltfMesh},
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    pbr::gltf::standard_material_from_gltf_material,
    prelude::*,
};
use worldgen::{Biome, geology::Rock};

use crate::{
    AppState,
    shaders::{LodToonMaterial, LodToonShader, SwayToonMaterial, SwayToonShader, ToonMaterial, toon},
};

/// Model units (metres) to world units: one world unit is 20 m (buildings use the same scale).
pub(super) const METERS_TO_WORLD: f32 = 0.05;

/// LOD0 (textured) models show within this distance of the camera, cross-fading with the
/// merged LOD1 stand-ins over ± [`LOD_FADE`].
pub(super) const LOD0_DISTANCE: f32 = 20.;

pub(super) const LOD_FADE: f32 = 2.;

/// Ground cover fades out between these distances.
pub(super) const GROUND_FADE: (f32, f32) = (14., 18.);

/// LOD0 entities are spawned within their visible distance plus this margin, and despawned
/// beyond it plus twice the margin.
pub(super) const SPAWN_MARGIN: f32 = 3.;

/// Most LOD0 entities spawned per frame.
pub(super) const SPAWNS_PER_FRAME: usize = 600;

/// Chunks closer than this (camera to the chunk, world units) show the merged LOD1 mesh
/// instead of the LOD2 one; leaving takes a margin more, against flickering.
pub(super) const NEAR_DISTANCE: f32 = 90.;

pub(super) const LEVEL_MARGIN: f32 = 15.;

/// Placement grid cells (world units) for large things and for ground cover.
pub(super) const LARGE_CELL: f32 = 0.45;

pub(super) const GROUND_CELL: f32 = 0.28;

/// Placement grid cell of the tall grass patches, and the share of the ground they cover.
pub(super) const PATCH_CELL: f32 = 0.18;

pub(super) const PATCH_COVER: f32 = 0.3;

/// Background tasks at once (placement, near meshes).
pub(super) const MAX_TASKS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Tree,
    Bush,
    Rock,
    Wood,
    Ground,
}

impl Species {
    /// Grass-like species bend in the wind (`SwayToonMaterial`).
    pub(super) fn sways(&self) -> bool {
        matches!(
            self.name,
            "tall_grass" | "dry_tall_grass" | "reeds" | "wispy_grass" | "grass_a" | "grass_b"
        )
    }
}

impl Kind {
    /// Kinds with low-poly stand-ins (LOD1 and LOD2).
    pub(super) fn has_stand_ins(self) -> bool {
        self != Kind::Ground
    }

    /// Steepest ground (1 - normal.y) it grows or lies on.
    pub(super) fn max_steepness(self) -> f32 {
        match self {
            Kind::Tree | Kind::Ground => 0.25,
            Kind::Bush | Kind::Wood => 0.35,
            Kind::Rock => 0.7,
        }
    }
}

pub(super) struct Species {
    /// Model file prefix in `assets/models/nature`.
    pub(super) name: &'static str,
    pub(super) kind: Kind,
    /// Random scale range (on top of [`METERS_TO_WORLD`]).
    pub(super) scale: (f32, f32),
}

pub(super) const SPECIES: &[Species] = &[
    // Kenney Nature Kit (low poly, recoloured): the bulk of the forests and rocks.
    Species { name: "oak_round", kind: Kind::Tree, scale: (0.8, 1.2) },
    Species { name: "oak_detailed", kind: Kind::Tree, scale: (0.8, 1.2) },
    Species { name: "oak", kind: Kind::Tree, scale: (0.8, 1.25) },
    Species { name: "oak_fat", kind: Kind::Tree, scale: (0.8, 1.2) },
    Species { name: "poplar", kind: Kind::Tree, scale: (0.8, 1.15) },
    Species { name: "pine_round_a", kind: Kind::Tree, scale: (0.8, 1.3) },
    Species { name: "pine_round_c", kind: Kind::Tree, scale: (0.8, 1.3) },
    Species { name: "pine_round_e", kind: Kind::Tree, scale: (0.8, 1.3) },
    Species { name: "pine_tall_a", kind: Kind::Tree, scale: (0.85, 1.3) },
    Species { name: "pine_tall_c", kind: Kind::Tree, scale: (0.85, 1.3) },
    Species { name: "pine_small", kind: Kind::Tree, scale: (0.6, 1.0) },
    Species { name: "cypress", kind: Kind::Tree, scale: (0.8, 1.2) },
    Species { name: "umbrella_pine", kind: Kind::Tree, scale: (0.9, 1.3) },
    Species { name: "rock_a", kind: Kind::Rock, scale: (0.8, 2.5) },
    Species { name: "rock_b", kind: Kind::Rock, scale: (0.8, 2.5) },
    Species { name: "rock_c", kind: Kind::Rock, scale: (0.8, 2.5) },
    Species { name: "rock_d", kind: Kind::Rock, scale: (0.8, 2.5) },
    Species { name: "rock_e", kind: Kind::Rock, scale: (0.8, 2.5) },
    Species { name: "rock_f", kind: Kind::Rock, scale: (0.8, 2.5) },
    Species { name: "rock_tall_a", kind: Kind::Rock, scale: (0.8, 1.6) },
    Species { name: "rock_tall_b", kind: Kind::Rock, scale: (0.8, 1.6) },
    Species { name: "rock_tall_c", kind: Kind::Rock, scale: (0.8, 1.6) },
    Species { name: "rock_tall_d", kind: Kind::Rock, scale: (0.8, 1.6) },
    Species { name: "stone_a", kind: Kind::Rock, scale: (0.8, 2.5) },
    Species { name: "stone_c", kind: Kind::Rock, scale: (0.8, 2.5) },
    Species { name: "stone_e", kind: Kind::Rock, scale: (0.8, 2.5) },
    Species { name: "cactus_tall", kind: Kind::Tree, scale: (0.7, 1.2) },
    Species { name: "cactus_short", kind: Kind::Tree, scale: (0.7, 1.2) },
    Species { name: "desert_shrub", kind: Kind::Bush, scale: (0.8, 1.4) },
    Species { name: "tall_grass", kind: Kind::Ground, scale: (0.8, 1.3) },
    Species { name: "dry_tall_grass", kind: Kind::Ground, scale: (0.8, 1.3) },
    Species { name: "reeds", kind: Kind::Ground, scale: (0.8, 1.4) },
    Species { name: "stump", kind: Kind::Wood, scale: (0.8, 1.2) },
    Species { name: "logs", kind: Kind::Wood, scale: (0.8, 1.2) },
    // Quaternius Stylized Nature MegaKit (textured): hero trees, boulders, bushes, ground cover.
    Species { name: "common_tree_a", kind: Kind::Tree, scale: (1.1, 1.5) },
    Species { name: "common_tree_b", kind: Kind::Tree, scale: (1.0, 1.4) },
    Species { name: "hero_pine", kind: Kind::Tree, scale: (1.2, 1.6) },
    Species { name: "twisted_tree", kind: Kind::Tree, scale: (0.7, 0.9) },
    Species { name: "dead_tree_a", kind: Kind::Wood, scale: (0.7, 1.0) },
    Species { name: "dead_tree_b", kind: Kind::Wood, scale: (0.7, 1.0) },
    Species { name: "boulder_a", kind: Kind::Rock, scale: (0.6, 1.5) },
    Species { name: "boulder_b", kind: Kind::Rock, scale: (0.6, 1.5) },
    Species { name: "boulder_c", kind: Kind::Rock, scale: (0.6, 1.5) },
    Species { name: "rock_large_a", kind: Kind::Rock, scale: (0.7, 1.6) },
    // Tropical and other biomes (Kenney Nature Kit, see `tools/nature/build.sh`).
    Species { name: "palm_tall", kind: Kind::Tree, scale: (0.8, 1.25) },
    Species { name: "palm_bend", kind: Kind::Tree, scale: (0.8, 1.25) },
    Species { name: "palm_short", kind: Kind::Tree, scale: (0.8, 1.2) },
    Species { name: "palm_detailed", kind: Kind::Tree, scale: (0.8, 1.2) },
    Species { name: "acacia", kind: Kind::Tree, scale: (0.8, 1.3) },
    Species { name: "jungle_a", kind: Kind::Tree, scale: (0.9, 1.4) },
    Species { name: "jungle_b", kind: Kind::Tree, scale: (0.9, 1.4) },
    Species { name: "jungle_c", kind: Kind::Tree, scale: (0.9, 1.3) },
    Species { name: "jungle_d", kind: Kind::Tree, scale: (0.9, 1.4) },
    Species { name: "mangrove", kind: Kind::Tree, scale: (0.8, 1.3) },
    Species { name: "mangrove_b", kind: Kind::Tree, scale: (0.8, 1.3) },
    Species { name: "bamboo", kind: Kind::Bush, scale: (0.8, 1.3) },
    Species { name: "tundra_shrub", kind: Kind::Bush, scale: (0.8, 1.6) },
    Species { name: "rock_large_b", kind: Kind::Rock, scale: (0.7, 1.6) },
    Species { name: "rock_small_a", kind: Kind::Rock, scale: (0.8, 2.) },
    Species { name: "rock_small_b", kind: Kind::Rock, scale: (0.8, 2.) },
    Species { name: "rock_small_c", kind: Kind::Rock, scale: (0.8, 2.) },
    Species { name: "bush_common", kind: Kind::Bush, scale: (0.9, 1.4) },
    Species { name: "bush_flowers", kind: Kind::Bush, scale: (0.9, 1.4) },
    Species { name: "pebbles_a", kind: Kind::Ground, scale: (1.0, 2.0) },
    Species { name: "pebbles_b", kind: Kind::Ground, scale: (1.0, 2.0) },
    Species { name: "fern", kind: Kind::Ground, scale: (0.8, 1.2) },
    Species { name: "flowers_a", kind: Kind::Ground, scale: (0.6, 1.0) },
    Species { name: "flowers_b", kind: Kind::Ground, scale: (0.6, 1.0) },
    Species { name: "grass_a", kind: Kind::Ground, scale: (0.7, 1.2) },
    Species { name: "grass_b", kind: Kind::Ground, scale: (0.7, 1.2) },
    Species { name: "clover", kind: Kind::Ground, scale: (0.8, 1.2) },
    Species { name: "mushrooms", kind: Kind::Ground, scale: (0.8, 1.3) },
    Species { name: "plant", kind: Kind::Ground, scale: (0.8, 1.3) },
    Species { name: "wispy_grass", kind: Kind::Ground, scale: (0.8, 1.2) },
];

/// Probability per large placement cell of each species, by biome.
pub(super) fn large_rules(biome: Biome) -> &'static [(&'static str, f32)] {
    match biome {
        Biome::DeciduousForest => &[
            ("oak_round", 0.2),
            ("oak_detailed", 0.12),
            ("oak", 0.12),
            ("oak_fat", 0.08),
            ("poplar", 0.06),
            ("common_tree_a", 0.03),
            ("common_tree_b", 0.03),
            ("bush_common", 0.05),
            ("bush_flowers", 0.02),
            ("stump", 0.01),
            ("logs", 0.008),
            ("boulder_a", 0.008),
        ],
        Biome::ConiferForest => &[
            ("pine_round_a", 0.17),
            ("pine_round_c", 0.15),
            ("pine_round_e", 0.13),
            ("pine_tall_a", 0.1),
            ("pine_tall_c", 0.1),
            ("pine_small", 0.05),
            ("hero_pine", 0.02),
            ("poplar", 0.02),
            ("stump", 0.015),
            ("logs", 0.01),
            ("rock_c", 0.01),
            ("boulder_b", 0.01),
        ],
        Biome::Meadow => &[
            ("oak_round", 0.02),
            ("oak", 0.015),
            ("common_tree_a", 0.008),
            ("twisted_tree", 0.002),
            ("bush_common", 0.035),
            ("bush_flowers", 0.03),
            ("rock_e", 0.006),
            ("boulder_c", 0.004),
        ],
        Biome::Steppe => &[
            ("cypress", 0.01),
            ("umbrella_pine", 0.008),
            ("bush_common", 0.01),
            ("rock_a", 0.01),
            ("rock_c", 0.008),
            ("stone_e", 0.005),
        ],
        Biome::Marsh => &[
            ("poplar", 0.03),
            ("bush_common", 0.05),
            ("dead_tree_a", 0.006),
            ("logs", 0.006),
        ],
        Biome::Alpine => &[
            ("pine_small", 0.04),
            ("pine_tall_c", 0.02),
            ("dead_tree_b", 0.01),
            ("rock_tall_a", 0.015),
            ("rock_tall_b", 0.01),
            ("rock_b", 0.03),
            ("stone_a", 0.02),
            ("stump", 0.006),
        ],
        Biome::Rock => &[
            ("rock_tall_a", 0.03),
            ("rock_tall_b", 0.025),
            ("rock_tall_c", 0.02),
            ("rock_tall_d", 0.02),
            ("rock_d", 0.04),
            ("rock_f", 0.03),
            ("boulder_a", 0.02),
        ],
        Biome::Snow => &[("stone_c", 0.015), ("rock_tall_d", 0.008)],
        Biome::Rainforest => &[
            ("jungle_a", 0.2),
            ("jungle_b", 0.17),
            ("jungle_c", 0.13),
            ("jungle_d", 0.13),
            ("palm_tall", 0.02),
            ("bamboo", 0.05),
            ("bush_common", 0.07),
            ("logs", 0.006),
        ],
        Biome::CloudForest => &[
            ("jungle_c", 0.14),
            ("jungle_a", 0.1),
            ("pine_tall_a", 0.08),
            ("pine_round_a", 0.06),
            ("twisted_tree", 0.02),
            ("bamboo", 0.04),
            ("bush_common", 0.07),
            ("boulder_b", 0.008),
        ],
        Biome::Savanna => &[
            ("acacia", 0.035),
            ("bush_common", 0.02),
            ("dead_tree_b", 0.002),
            ("rock_e", 0.004),
            ("boulder_c", 0.003),
        ],
        Biome::Riparian => &[
            ("poplar", 0.12),
            ("common_tree_a", 0.04),
            ("oak_round", 0.03),
            ("bush_common", 0.1),
            ("bush_flowers", 0.02),
        ],
        Biome::Oasis => &[
            ("palm_tall", 0.09),
            ("palm_detailed", 0.05),
            ("palm_bend", 0.04),
            ("palm_short", 0.04),
            ("bush_common", 0.08),
            ("acacia", 0.01),
        ],
        Biome::Mangrove => &[("mangrove", 0.2), ("mangrove_b", 0.15), ("palm_short", 0.01)],
        Biome::Tundra => &[
            ("tundra_shrub", 0.06),
            ("stone_a", 0.015),
            ("stone_c", 0.01),
            ("boulder_c", 0.006),
            ("rock_small_a", 0.008),
        ],
        Biome::AlpineMeadow => &[
            ("bush_flowers", 0.01),
            ("tundra_shrub", 0.02),
            ("rock_b", 0.02),
            ("stone_a", 0.02),
            ("rock_tall_a", 0.008),
            ("boulder_b", 0.006),
        ],
        Biome::Beach => &[("stone_e", 0.006), ("logs", 0.003)],
        Biome::TropicalBeach => &[
            ("palm_tall", 0.05),
            ("palm_bend", 0.06),
            ("palm_short", 0.04),
            ("palm_detailed", 0.03),
            ("bush_common", 0.01),
        ],
        Biome::Desert => &[
            ("desert_shrub", 0.03),
            ("cactus_short", 0.012),
            ("cactus_tall", 0.006),
            ("rock_a", 0.01),
            ("rock_c", 0.008),
            ("dead_tree_b", 0.002),
        ],
        Biome::Erg => &[("desert_shrub", 0.004), ("dead_tree_b", 0.0008)],
        Biome::Badlands => &[
            ("desert_shrub", 0.012),
            ("rock_tall_d", 0.006),
            ("rock_large_b", 0.006),
            ("dead_tree_b", 0.002),
        ],
        Biome::Sea | Biome::Lake => &[],
    }
}

/// Probability per patch cell of each tall grass species, by biome (only inside patches).
pub(super) fn patch_rules(biome: Biome) -> &'static [(&'static str, f32)] {
    match biome {
        Biome::Meadow => &[("tall_grass", 0.45), ("wispy_grass", 0.12)],
        Biome::Steppe => &[("dry_tall_grass", 0.5)],
        Biome::Marsh => &[("reeds", 0.6), ("tall_grass", 0.2)],
        Biome::Alpine => &[("dry_tall_grass", 0.15)],
        Biome::Savanna => &[("dry_tall_grass", 0.6), ("tall_grass", 0.1)],
        Biome::Riparian | Biome::Oasis => &[("reeds", 0.25), ("tall_grass", 0.35)],
        Biome::AlpineMeadow => &[("tall_grass", 0.15), ("wispy_grass", 0.1)],
        Biome::Tundra => &[("wispy_grass", 0.1)],
        _ => &[],
    }
}

/// Probability per ground cover cell of each species, by biome.
pub(super) fn ground_rules(biome: Biome) -> &'static [(&'static str, f32)] {
    match biome {
        Biome::DeciduousForest => &[("fern", 0.18), ("plant", 0.04), ("mushrooms", 0.02)],
        Biome::ConiferForest => &[("fern", 0.1), ("mushrooms", 0.02)],
        Biome::Meadow => &[
            ("grass_a", 0.05),
            ("grass_b", 0.05),
            ("clover", 0.03),
            ("flowers_a", 0.025),
            ("flowers_b", 0.02),
        ],
        Biome::Steppe => &[("grass_b", 0.07), ("grass_a", 0.03)],
        Biome::Desert => &[("pebbles_a", 0.05), ("pebbles_b", 0.03)],
        Biome::Badlands => &[("pebbles_b", 0.02)],
        Biome::Marsh => &[("grass_a", 0.12), ("plant", 0.04)],
        Biome::Alpine => &[("grass_b", 0.04), ("pebbles_a", 0.02)],
        Biome::Rainforest | Biome::CloudForest => &[("fern", 0.3), ("plant", 0.12), ("mushrooms", 0.01)],
        Biome::Savanna => &[("grass_b", 0.06), ("grass_a", 0.02)],
        Biome::Mangrove => &[("plant", 0.03)],
        Biome::Riparian | Biome::Oasis => &[("grass_a", 0.08), ("fern", 0.04), ("plant", 0.04)],
        Biome::Tundra => &[("grass_b", 0.05), ("pebbles_a", 0.04), ("pebbles_b", 0.02)],
        Biome::AlpineMeadow => &[
            ("grass_a", 0.05),
            ("flowers_a", 0.04),
            ("flowers_b", 0.03),
            ("clover", 0.02),
            ("pebbles_a", 0.02),
        ],
        Biome::Rock => &[("pebbles_a", 0.03)],
        Biome::Beach => &[("pebbles_b", 0.02), ("pebbles_a", 0.01)],
        _ => &[],
    }
}

pub(super) fn species_index(name: &str) -> u16 {
    SPECIES
        .iter()
        .position(|s| s.name == name)
        .unwrap_or_else(|| panic!("unknown species {name}")) as u16
}

/// Loads the nature models and prepares them ([`Vegetation`]) once the world exists (the flat
/// map or the planet).
pub(super) struct VegetationModelsPlugin;

impl Plugin for VegetationModelsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_models).add_systems(
            Update,
            prepare_models
                .run_if(not(resource_exists::<Vegetation>))
                .run_if(in_state(AppState::InGame)),
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
type Part = (Handle<Mesh>, PartMaterial);

#[derive(Clone)]
pub(super) enum PartMaterial {
    Toon(Handle<ToonMaterial>),
    Sway(Handle<SwayToonMaterial>),
}

/// Geometry of a stand-in, merged into the chunks' meshes.
#[derive(Default)]
pub(super) struct StandIn {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// The loaded vegetation models, ready to place.
#[derive(Resource)]
pub(super) struct Vegetation {
    lod0: Vec<Vec<Part>>,
    pub(super) lod1: Arc<Vec<StandIn>>,
    pub(super) lod2: Arc<Vec<StandIn>>,
    pub(super) far_material: Handle<ToonMaterial>,
    pub(super) near_material: Handle<LodToonMaterial>,
}

/// Once every model is loaded, convert their materials to toon materials and extract the
/// stand-in geometry. (No snow on objects: the toon shader's snow goes by world height, which
/// means nothing on a sphere.)
fn prepare_models(
    mut commands: Commands,
    models: Option<Res<VegetationModels>>,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    gltf_materials: Res<Assets<GltfMaterial>>,
    meshes: Res<Assets<Mesh>>,
    mut toon_materials: ResMut<Assets<ToonMaterial>>,
    mut lod_materials: ResMut<Assets<LodToonMaterial>>,
    mut sway_materials: ResMut<Assets<SwayToonMaterial>>,
) {
    let (snow, wind) = (Vec4::ZERO, Vec4::new(1., 0., 0., 0.));
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
    let primitives = |handle: &Handle<Gltf>| {
        gltfs
            .get(handle)
            .into_iter()
            .flat_map(|gltf| &gltf.meshes)
            .filter_map(|mesh| gltf_meshes.get(mesh))
            .flat_map(|mesh| mesh.primitives.clone())
            .collect::<Vec<_>>()
    };
    let lod0 = models
        .0
        .iter()
        .zip(SPECIES)
        .map(|(levels, species)| {
            primitives(&levels[0])
                .into_iter()
                .map(|primitive| {
                    let base = primitive
                        .material
                        .as_ref()
                        .and_then(|material| gltf_materials.get(material))
                        .map(standard_material_from_gltf_material)
                        .unwrap_or_default();
                    let material = if species.sways() {
                        PartMaterial::Sway(sway_materials.add(SwayToonMaterial {
                            base,
                            extension: SwayToonShader {
                                tint: LinearRgba::WHITE,
                                snow,
                                wind,
                            },
                        }))
                    } else {
                        PartMaterial::Toon(toon_materials.add(toon(&base, snow)))
                    };
                    (primitive.mesh, material)
                })
                .collect()
        })
        .collect();
    let stand_ins = |level: usize| -> Vec<StandIn> {
        models
            .0
            .iter()
            .map(|levels| {
                let mut stand_in = StandIn::default();
                for primitive in levels.get(level).map(primitives).unwrap_or_default() {
                    if let Some(mesh) = meshes.get(&primitive.mesh) {
                        append_mesh(&mut stand_in, mesh);
                    }
                }
                stand_in
            })
            .collect()
    };
    let flat = StandardMaterial {
        perceptual_roughness: 1.,
        ..default()
    };
    let near_material = lod_materials.add(LodToonMaterial {
        base: flat.clone(),
        extension: LodToonShader {
            tint: LinearRgba::WHITE,
            lod: Vec4::new(LOD0_DISTANCE - LOD_FADE, LOD0_DISTANCE + LOD_FADE, 0., 0.),
            snow,
        },
    });
    commands.insert_resource(Vegetation {
        lod0,
        lod1: Arc::new(stand_ins(1)),
        lod2: Arc::new(stand_ins(2)),
        far_material: toon_materials.add(toon(&flat, snow)),
        near_material,
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
pub(super) struct Instance {
    pub(super) species: u16,
    pub(super) position: Vec3,
    pub(super) rotation: Quat,
    pub(super) scale: f32,
}

impl Instance {
    fn transform(&self) -> Transform {
        Transform::from_translation(self.position)
            .with_rotation(self.rotation)
            .with_scale(Vec3::splat(self.scale))
    }
}

/// The stand-ins of `instances` merged into one mesh, relative to `origin`. With
/// `with_origins`, each vertex also carries its instance's world origin (UV0 = x, z;
/// UV1.x = y), for the per-object fade of [`LodToonMaterial`].
pub(super) fn merged_mesh(
    stand_ins: &[StandIn],
    instances: &[Instance],
    origin: Vec3,
    with_origins: bool,
) -> Option<Mesh> {
    let mut merged = StandIn::default();
    let (mut uv0, mut uv1) = (Vec::new(), Vec::new());
    for instance in instances {
        let stand_in = &stand_ins[instance.species as usize];
        if stand_in.positions.is_empty() {
            continue;
        }
        let rotation = instance.rotation;
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
        if with_origins {
            let count = stand_in.positions.len();
            uv0.extend(std::iter::repeat_n(instance.position.xz().to_array(), count));
            uv1.extend(std::iter::repeat_n([instance.position.y, 0.], count));
        }
    }
    if merged.positions.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, merged.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, merged.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, merged.colors)
        .with_inserted_indices(Indices::U32(merged.indices));
    if with_origins {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv0);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
    }
    Some(mesh)
}

/// Spawn the LOD0 model of an instance (its parts), cross-fading out at its LOD distance
/// (ground cover: fading out, casting no shadow).
pub(super) fn spawn_lod0(
    commands: &mut Commands,
    vegetation: &Vegetation,
    instance: &Instance,
    ground: bool,
) -> Vec<Entity> {
    let range = if ground {
        GROUND_FADE.0..GROUND_FADE.1
    } else {
        LOD0_DISTANCE - LOD_FADE..LOD0_DISTANCE + LOD_FADE
    };
    vegetation.lod0[instance.species as usize]
        .iter()
        .map(|(mesh, material)| {
            let mut entity = commands.spawn((
                Mesh3d(mesh.clone()),
                instance.transform(),
                VisibilityRange {
                    start_margin: 0.0..0.0,
                    end_margin: range.clone(),
                    use_aabb: false,
                },
                Pickable::IGNORE,
            ));
            match material {
                PartMaterial::Toon(material) => entity.insert(MeshMaterial3d(material.clone())),
                PartMaterial::Sway(material) => entity.insert(MeshMaterial3d(material.clone())),
            };
            if ground {
                entity.insert(NotShadowCaster);
            }
            entity.id()
        })
        .collect()
}

/// The rocks scattered over rocky ground, by rock type (shares of the rocks placed): rounded
/// granite boulders, angular basalt blocks, sandstone towers, pale limestone, small shale
/// rocks, metamorphic slabs.
pub(super) fn scattered_rocks(rock: Rock) -> &'static [(&'static str, f32)] {
    match rock {
        Rock::Granite => &[
            ("boulder_a", 0.25),
            ("boulder_b", 0.25),
            ("boulder_c", 0.2),
            ("rock_large_a", 0.15),
            ("rock_small_a", 0.15),
        ],
        Rock::Metamorphic => &[
            ("rock_a", 0.2),
            ("rock_b", 0.2),
            ("rock_c", 0.2),
            ("rock_tall_c", 0.15),
            ("rock_small_b", 0.25),
        ],
        Rock::Basalt => &[
            ("rock_tall_a", 0.25),
            ("rock_tall_b", 0.25),
            ("rock_d", 0.2),
            ("rock_f", 0.2),
            ("rock_small_c", 0.1),
        ],
        Rock::Sandstone => &[
            ("rock_tall_d", 0.25),
            ("rock_large_b", 0.25),
            ("rock_e", 0.2),
            ("rock_tall_a", 0.1),
            ("rock_small_a", 0.2),
        ],
        Rock::Shale => &[
            ("rock_small_a", 0.3),
            ("rock_small_b", 0.3),
            ("rock_small_c", 0.2),
            ("stone_e", 0.2),
        ],
        Rock::Limestone => &[
            ("stone_a", 0.3),
            ("stone_c", 0.3),
            ("stone_e", 0.15),
            ("rock_large_a", 0.15),
            ("rock_tall_d", 0.1),
        ],
    }
}
