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
            MaterialPlugin::<WaterMaterial>::default(),
            MaterialPlugin::<ToonMaterial>::default(),
            MaterialPlugin::<LodToonMaterial>::default(),
        ));
        app.init_asset_loader::<MapMaterialLoader>();
    }
}

const MAP_SHADER_ASSET_PATH: &str = "shaders/map_material.wesl";
const WATER_SHADER_ASSET_PATH: &str = "shaders/water.wesl";
const TOON_SHADER_ASSET_PATH: &str = "shaders/toon.wesl";

/// Toon material for objects (vegetation, rocks): the standard material's colours with the
/// terrain's cel lighting. `tint` multiplies them.
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct ToonShader {
    #[uniform(100)]
    pub tint: LinearRgba,
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

/// The toon version of a standard material (e.g. one loaded from a glTF file).
pub fn toon(base: &StandardMaterial) -> ToonMaterial {
    ToonMaterial {
        base: base.clone(),
        extension: ToonShader {
            tint: LinearRgba::WHITE,
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
    /// Mapping from world xz to `ground_*` coordinates: `uv = xz * zw + xy`.
    #[uniform(106)]
    pub ground_map: Vec4,
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
            ground_map: Vec4::ZERO,
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
