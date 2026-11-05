use bevy::{
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::*,
    shader::ShaderRef,
};

pub struct ShadersPlugin;
impl Plugin for ShadersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<PlanetTerrainMaterial>::default(),
            MaterialPlugin::<PlanetWaterMaterial>::default(),
            MaterialPlugin::<ToonMaterial>::default(),
            MaterialPlugin::<LodToonMaterial>::default(),
            MaterialPlugin::<SwayToonMaterial>::default(),
            MaterialPlugin::<GrassBladeMaterial>::default(),
            MaterialPlugin::<CloudMaterial>::default(),
        ));
    }
}

const PLANET_TERRAIN_SHADER_ASSET_PATH: &str = "shaders/planet_terrain.wesl";
const CLOUDS_SHADER_ASSET_PATH: &str = "shaders/clouds.wesl";

/// The cloud layer (`clouds.wesl`): a shell round the planet showing the sky map's clouds
/// (`worldgen::sky`), toon-lit. `layer.x` is the shell's radius; `tint` the clouds' colour
/// (hazy skies yellow them).
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct CloudMaterial {
    #[uniform(0)]
    pub layer: Vec4,
    #[uniform(1)]
    pub tint: LinearRgba,
    #[texture(2)]
    #[sampler(3)]
    pub sky: Handle<Image>,
}

impl Material for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        CLOUDS_SHADER_ASSET_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // (Seen from above and from below.)
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

const PLANET_WATER_SHADER_ASSET_PATH: &str = "shaders/planet_water.wesl";

/// The planet's water: the flat map's toon water on the sphere (`planet_water.wesl`). Meshes
/// carry 2D pattern coordinates in vertex colour red/green; rivers also their flow velocity
/// (UV0, in those coordinates) and (whitewater, opacity) (UV1), lakes and the sea the wind
/// (UV0: direction × speed in m/s, in those coordinates).
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct PlanetWaterShader {
    #[uniform(100)]
    pub tint: LinearRgba,
    /// x: the sea level radius, y: cells along a face edge, z: 1 with a sea wave field.
    #[uniform(101)]
    pub planet: Vec4,
    /// The sea wave field of one cube face, by cell (see `worldgen::sea`): travel time
    /// to the shore, depth, exposure, wind speed.
    #[texture(102)]
    #[sampler(103)]
    pub sea: Option<Handle<Image>>,
    /// Wind direction (in the pattern coordinates) where the mesh doesn't carry its own.
    #[uniform(104)]
    pub wind: Vec4,
    /// The sky map (`worldgen::sky`: cloud density, convective, haze, dust by direction)
    /// for cloud shadows and haze, and the cloud layer's radius (x; 0: no clouds).
    #[texture(105)]
    #[sampler(106)]
    pub sky_map: Option<Handle<Image>>,
    #[uniform(107)]
    pub sky: Vec4,
}

impl MaterialExtension for PlanetWaterShader {
    fn fragment_shader() -> ShaderRef {
        PLANET_WATER_SHADER_ASSET_PATH.into()
    }
}

pub type PlanetWaterMaterial = ExtendedMaterial<StandardMaterial, PlanetWaterShader>;

/// The planet's terrain: the flat map's toon ground on the sphere (`planet_terrain.wesl`).
/// Meshes carry the face's cell coordinates (UV0), and the ground materials from the climate:
/// colour = (dryness, forest, marsh / -salt flat, wet bed), UV1 = (cold + snow, coast).
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct PlanetTerrainShader {
    #[uniform(100)]
    pub tint: LinearRgba,
    /// x: the sea level radius, y: cells along a face edge, z: world units per erosion unit
    /// (`Planet::strata_scale`), w: 1 with a rock field.
    #[uniform(101)]
    pub planet: Vec4,
    /// The rock field of one cube face, by cell: rock id, strata offset (see `worldgen::geology`).
    #[texture(102)]
    #[sampler(103)]
    pub rock: Option<Handle<Image>>,
    /// Prevailing wind direction (in face coordinates).
    #[uniform(108)]
    pub wind: Vec4,

    /// The world (`worldgen::world::World`): life on land (0..1: without, bare regolith,
    /// mudflats, no flowers), how oxidised the ground is (0..1: grey and beige → rusty), soil
    /// depth (1: Earth's; thin soil shows more rock).
    #[uniform(109)]
    pub world: Vec4,
    /// Plants under the star (`worldgen::world::Star::foliage`: hue turn of the greens,
    /// brightness); (0, 1): as they are.
    #[uniform(110)]
    pub foliage: Vec4,
    /// The sky map (`worldgen::sky`: cloud density, convective, haze, dust by direction)
    /// for cloud shadows and haze, and the cloud layer's radius (x; 0: no clouds).
    #[texture(111)]
    #[sampler(112)]
    pub sky_map: Option<Handle<Image>>,
    #[uniform(113)]
    pub sky: Vec4,
}

impl MaterialExtension for PlanetTerrainShader {
    fn fragment_shader() -> ShaderRef {
        PLANET_TERRAIN_SHADER_ASSET_PATH.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        PLANET_TERRAIN_SHADER_ASSET_PATH.into()
    }
}

pub type PlanetTerrainMaterial = ExtendedMaterial<StandardMaterial, PlanetTerrainShader>;
const TOON_SHADER_ASSET_PATH: &str = "shaders/toon.wesl";
const SWAY_SHADER_ASSET_PATH: &str = "shaders/toon_sway.wesl";
const GRASS_BLADES_SHADER_ASSET_PATH: &str = "shaders/grass_blades.wesl";

/// Toon material for objects (vegetation, rocks): the standard material's colours with the
/// terrain's cel lighting. `tint` multiplies them; snow covers upward faces between the world
/// heights `snow.x` and `snow.y` (disabled when `snow.y <= snow.x`).
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct ToonShader {
    #[uniform(100)]
    pub tint: LinearRgba,
    #[uniform(102)]
    pub snow: Vec4,
    /// Plants under the star (`worldgen::world::Star::foliage`: hue turn of the greens,
    /// brightness); (0, 1): as they are.
    #[uniform(104)]
    pub foliage: Vec4,
    /// The sky map (`worldgen::sky`: cloud density, convective, haze, dust by direction)
    /// for cloud shadows and haze, and the cloud layer's radius (x; 0: no clouds).
    #[texture(105)]
    #[sampler(106)]
    pub sky_map: Option<Handle<Image>>,
    #[uniform(107)]
    pub sky: Vec4,
}

impl MaterialExtension for ToonShader {
    fn fragment_shader() -> ShaderRef {
        TOON_SHADER_ASSET_PATH.into()
    }
}

pub type ToonMaterial = ExtendedMaterial<StandardMaterial, ToonShader>;

/// Plants as they are (see `ToonShader::foliage`).
pub const NO_FOLIAGE_TINT: Vec4 = Vec4::new(0., 1., 0., 0.);

/// [`ToonShader`] for merged stand-ins of many objects (far levels of detail), each fading out
/// where its detailed model fades in: vertices carry their object's origin (UV0 = x, z;
/// UV1.x = y), and `lod` is the detailed models' visibility range end margin (`x..y`). Not in
/// the depth prepass, which would hide the detailed models behind faded-out stand-ins.
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct LodToonShader {
    #[uniform(100)]
    pub tint: LinearRgba,
    #[uniform(101)]
    pub lod: Vec4,
    #[uniform(102)]
    pub snow: Vec4,
    /// Plants under the star (`worldgen::world::Star::foliage`: hue turn of the greens,
    /// brightness); (0, 1): as they are.
    #[uniform(104)]
    pub foliage: Vec4,
    /// The sky map (`worldgen::sky`: cloud density, convective, haze, dust by direction)
    /// for cloud shadows and haze, and the cloud layer's radius (x; 0: no clouds).
    #[texture(105)]
    #[sampler(106)]
    pub sky_map: Option<Handle<Image>>,
    #[uniform(107)]
    pub sky: Vec4,
}

impl MaterialExtension for LodToonShader {
    fn fragment_shader() -> ShaderRef {
        TOON_SHADER_ASSET_PATH.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.push("LOD_FADE".into());
        }
        Ok(())
    }
}

pub type LodToonMaterial = ExtendedMaterial<StandardMaterial, LodToonShader>;

/// [`ToonShader`] for vegetation swaying in the wind (tall grass, reeds): `toon_sway.wesl`
/// bends the models in the vertex shader. Not in the depth prepass (it would not sway).
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct SwayToonShader {
    #[uniform(100)]
    pub tint: LinearRgba,
    #[uniform(102)]
    pub snow: Vec4,
    /// Prevailing wind direction (x, z).
    #[uniform(103)]
    pub wind: Vec4,
    /// Plants under the star (`worldgen::world::Star::foliage`: hue turn of the greens,
    /// brightness); (0, 1): as they are.
    #[uniform(104)]
    pub foliage: Vec4,
    /// The sky map (`worldgen::sky`: cloud density, convective, haze, dust by direction)
    /// for cloud shadows and haze, and the cloud layer's radius (x; 0: no clouds).
    #[texture(105)]
    #[sampler(106)]
    pub sky_map: Option<Handle<Image>>,
    #[uniform(107)]
    pub sky: Vec4,
}

impl MaterialExtension for SwayToonShader {
    fn vertex_shader() -> ShaderRef {
        SWAY_SHADER_ASSET_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        TOON_SHADER_ASSET_PATH.into()
    }

    fn enable_prepass() -> bool {
        false
    }
}

pub type SwayToonMaterial = ExtendedMaterial<StandardMaterial, SwayToonShader>;

/// [`ToonShader`] for the grass blades drawn as geometry close to the camera
/// (`planet::grass`): `grass_blades.wesl` bends each blade with the wind (UV0.x: 0 at the base,
/// 1 at the tip). Same bindings as [`SwayToonShader`].
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct GrassBladeShader {
    #[uniform(100)]
    pub tint: LinearRgba,
    #[uniform(102)]
    pub snow: Vec4,
    /// Prevailing wind direction (x, z).
    #[uniform(103)]
    pub wind: Vec4,
    /// Plants under the star (`worldgen::world::Star::foliage`: hue turn of the greens,
    /// brightness); (0, 1): as they are.
    #[uniform(104)]
    pub foliage: Vec4,
    /// The sky map (`worldgen::sky`: cloud density, convective, haze, dust by direction)
    /// for cloud shadows and haze, and the cloud layer's radius (x; 0: no clouds).
    #[texture(105)]
    #[sampler(106)]
    pub sky_map: Option<Handle<Image>>,
    #[uniform(107)]
    pub sky: Vec4,
}

impl MaterialExtension for GrassBladeShader {
    fn vertex_shader() -> ShaderRef {
        GRASS_BLADES_SHADER_ASSET_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        TOON_SHADER_ASSET_PATH.into()
    }

    fn enable_prepass() -> bool {
        false
    }
}

pub type GrassBladeMaterial = ExtendedMaterial<StandardMaterial, GrassBladeShader>;

/// The toon version of a standard material (e.g. one loaded from a glTF file), with snow
/// between the world heights `snow.x` and `snow.y`.
pub fn toon(base: &StandardMaterial, snow: Vec4) -> ToonMaterial {
    ToonMaterial {
        base: base.clone(),
        extension: ToonShader {
            tint: LinearRgba::WHITE,
            snow,
            foliage: NO_FOLIAGE_TINT,
            sky_map: None,
            sky: Vec4::ZERO,
        },
    }
}
