use bevy::{
    asset::{AssetLoader, LoadContext},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::*,
    shader::ShaderRef,
};
use serde::{Deserialize, Deserializer};

pub struct ShadersPlugin;
impl Plugin for ShadersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<MapMaterial>::default(),
            MaterialPlugin::<PlanetTerrainMaterial>::default(),
            MaterialPlugin::<PlanetWaterMaterial>::default(),
            MaterialPlugin::<WaterMaterial>::default(),
            MaterialPlugin::<ToonMaterial>::default(),
            MaterialPlugin::<LodToonMaterial>::default(),
            MaterialPlugin::<SwayToonMaterial>::default(),
            MaterialPlugin::<GrassBladeMaterial>::default(),
        ));
        app.init_asset_loader::<MapMaterialLoader>();
    }
}

const MAP_SHADER_ASSET_PATH: &str = "shaders/map_material.wesl";
const PLANET_TERRAIN_SHADER_ASSET_PATH: &str = "shaders/planet_terrain.wesl";
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
    /// The sea wave field of one cube face, by cell (see `worldgen::planet_sea`): travel time
    /// to the shore, depth, exposure, wind speed.
    #[texture(102)]
    #[sampler(103)]
    pub sea: Option<Handle<Image>>,
    /// Wind direction (in the pattern coordinates) where the mesh doesn't carry its own.
    #[uniform(104)]
    pub wind: Vec4,
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
    /// x: the sea level radius.
    #[uniform(101)]
    pub planet: Vec4,
    /// Prevailing wind direction (in face coordinates).
    #[uniform(108)]
    pub wind: Vec4,
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
const WATER_SHADER_ASSET_PATH: &str = "shaders/water.wesl";
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
}

impl MaterialExtension for ToonShader {
    fn fragment_shader() -> ShaderRef {
        TOON_SHADER_ASSET_PATH.into()
    }
}

pub type ToonMaterial = ExtendedMaterial<StandardMaterial, ToonShader>;

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
/// (`map::grass`): `grass_blades.wesl` bends each blade with the wind (UV0.x: 0 at the base,
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
        },
    }
}

/// Water material extension (sea, lakes, rivers): toon water drawn by the shader, coloured by
/// the depth of water behind each pixel (from the depth prepass), with shore foam and
/// animated glints. `tint` multiplies it.
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct WaterShader {
    #[uniform(100)]
    pub tint: LinearRgba,
    /// Sea wave field over the continent (see `map::sea`).
    #[texture(101)]
    #[sampler(102)]
    pub sea: Option<Handle<Image>>,
    /// Mapping from world xz to `sea` coordinates: `uv = xz * zw + xy`. Zero until bound.
    #[uniform(103)]
    pub sea_map: Vec4,
    /// Prevailing wind direction (x, z).
    #[uniform(104)]
    pub wind: Vec4,
}

impl MaterialExtension for WaterShader {
    fn fragment_shader() -> ShaderRef {
        WATER_SHADER_ASSET_PATH.into()
    }
}

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterShader>;

/// Terrain material extension. The shader draws the ground procedurally from the `ground`
/// material weights (see `map::ground`); `tint` multiplies it. Normals come from `gradient`
/// (see `TerrainHeights::gradient_texture`). Mesh vertex color red is the river/lake wetness.
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct TerrainShader {
    #[uniform(100)]
    pub tint: LinearRgba,
    /// Base surface height gradient over the continent.
    #[texture(101)]
    #[sampler(102)]
    pub gradient: Option<Handle<Image>>,
    /// Mapping from world xz to `gradient` coordinates: `uv = xz * zw + xy`.
    #[uniform(103)]
    pub gradient_map: Vec4,
    /// Ground material weights per continent cell (two textures, see `map::ground`).
    #[texture(104)]
    pub ground_a: Option<Handle<Image>>,
    #[texture(105)]
    pub ground_b: Option<Handle<Image>>,
    #[texture(107)]
    pub ground_c: Option<Handle<Image>>,
    /// Mapping from world xz to `ground_*` coordinates: `uv = xz * zw + xy`.
    #[uniform(106)]
    pub ground_map: Vec4,
    /// Prevailing wind direction (x, z).
    #[uniform(108)]
    pub wind: Vec4,
}

impl MaterialExtension for TerrainShader {
    fn fragment_shader() -> ShaderRef {
        MAP_SHADER_ASSET_PATH.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        MAP_SHADER_ASSET_PATH.into()
    }
}

fn deser_color<'de, D>(deserializer: D) -> Result<LinearRgba, D::Error>
where D: Deserializer<'de> {
    let buf = <String>::deserialize(deserializer)?;
    Ok(Srgba::hex(buf).unwrap_or(Srgba::WHITE).into())
}

pub type MapMaterial = ExtendedMaterial<StandardMaterial, TerrainShader>;

#[derive(Deserialize)]
#[serde(default)]
pub struct StandardMaterialParams {
    #[serde(deserialize_with = "deser_color")]
    base_color: LinearRgba,
    base_color_texture: Option<String>,
    #[serde(deserialize_with = "deser_color")]
    emissive: LinearRgba,
    emissive_texture: Option<String>,
    perceptual_roughness: f32,
    metallic: f32,
    metallic_roughness_texture: Option<String>,
    reflectance: f32,
    diffuse_transmission: f32,
    normal_map_texture: Option<String>,
    occlusion_texture: Option<String>,
    double_sided: bool,
    unlit: bool,
    alpha: bool,
}
impl Default for StandardMaterialParams {
    fn default() -> Self {
        Self {
            base_color: LinearRgba::WHITE,
            base_color_texture: None,
            emissive: LinearRgba::BLACK,
            emissive_texture: None,
            perceptual_roughness: 0.5,
            metallic: 0.,
            metallic_roughness_texture: None,
            reflectance: 0.5,
            diffuse_transmission: 0.,
            normal_map_texture: None,
            occlusion_texture: None,
            double_sided: false,
            unlit: false,
            alpha: false,
        }
    }
}

impl StandardMaterialParams {
    fn to_mat(self, ctx: &mut LoadContext<'_>) -> StandardMaterial{
        StandardMaterial {
            base_color: self.base_color.into(),
            base_color_texture: self.base_color_texture.map(|s| ctx.load(s)),
            emissive: self.emissive,
            emissive_texture: self.emissive_texture.map(|s| ctx.load(s)),
            perceptual_roughness: self.perceptual_roughness,
            metallic: self.metallic,
            metallic_roughness_texture: self.metallic_roughness_texture.map(|s| ctx.load(s)),
            reflectance: self.reflectance,
            diffuse_transmission: self.diffuse_transmission,
            normal_map_texture: self.normal_map_texture.map(|s| ctx.load(s)),
            occlusion_texture: self.occlusion_texture.map(|s| ctx.load(s)),
            double_sided: self.double_sided,
            unlit: self.unlit,
            alpha_mode: if self.alpha {AlphaMode::Blend} else {AlphaMode::Opaque},
            ..Default::default()
        }
    }
}

#[derive(Deserialize)]
pub struct MapMaterialParams {
    #[serde(default)]
    pub pbr: StandardMaterialParams, 
    #[serde(deserialize_with = "deser_color")]
    pub tint: LinearRgba,
}

#[derive(Default, TypePath)]
pub struct MapMaterialLoader;

impl AssetLoader for MapMaterialLoader {
    type Asset = MapMaterial;

    type Settings = ();

    type Error = anyhow::Error;

    async fn load(
        &self,
        reader: &mut dyn bevy::asset::io::Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();

        reader.read_to_end(&mut bytes).await?;
        let mat_params = ron::de::from_bytes::<MapMaterialParams>(&bytes)?;
        let base = mat_params.pbr.to_mat(load_context);
        let extension = TerrainShader {
            tint: mat_params.tint,
            gradient: None,
            gradient_map: Vec4::ZERO,
            ground_a: None,
            ground_b: None,
            ground_c: None,
            ground_map: Vec4::ZERO,
            wind: Vec4::new(1., 0., 0., 0.),
        };
        Ok(MapMaterial {base, extension})
    }

    fn extensions(&self) -> &[&str] {
        &["mapmat"]
    }
}

//     type Settings = ();

//     type Error = anyhow::Error;

//     async fn load(
//         &self,
//         reader: &mut dyn bevy::asset::io::Reader,
//         _settings: &Self::Settings,
//         load_context: &mut LoadContext<'_>,
//     ) -> Result<Self::Asset, Self::Error> {
//         let mut bytes = Vec::new();

//     fn extensions(&self) -> &[&str] {
//         &["bmat"]
//     }
// }
