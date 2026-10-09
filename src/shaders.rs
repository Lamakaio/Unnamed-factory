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
        ));
        app.init_asset_loader::<MapMaterialLoader>();
    }
}

const MAP_SHADER_ASSET_PATH: &str = "shaders/map_material.wesl";

/// Terrain material extension. Ground colors come from the mesh vertex colors (biomes);
/// `tint` multiplies them.
#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Reflect)]
#[reflect(PartialEq)]
pub struct TerrainShader {
    #[uniform(100)]
    pub tint: LinearRgba,
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
