use bevy::{
    asset::{AssetLoader, LoadContext},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::*,
};
use serde::{Deserialize, Deserializer};

pub struct ShadersPlugin;
impl Plugin for ShadersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<MapMaterial>::default(),
            //MaterialPlugin::<BuildMaterial>::default(),
        ));
        app.init_asset_loader::<MapMaterialLoader>();
    }
}

const MAP_SHADER_ASSET_PATH: &str = "shaders/map_material.wgsl";

#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Component, Reflect)]
#[reflect(PartialEq)]
pub struct TerrainShader {
    #[uniform(100)]
    pub grass_color: LinearRgba,
    #[uniform(101)]
    pub ocean_color: LinearRgba,
    #[uniform(102)]
    pub mountain_color: LinearRgba,
    #[uniform(103)]
    pub snow_color: LinearRgba,
    #[uniform(104)]
    pub sand_color: LinearRgba,
}

impl MaterialExtension for TerrainShader {
    fn fragment_shader() -> ShaderRef {
        MAP_SHADER_ASSET_PATH.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        MAP_SHADER_ASSET_PATH.into()
    }
}

// const BUILD_SHADER_ASSET_PATH: &str = "shaders/extended_material.wgsl";

// #[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Component, Reflect)]
// #[reflect(PartialEq)]
// pub struct BuildShader {
//     //The color modification when a part is selected
//     #[uniform(101)]
//     pub highlight_color: LinearRgba,
// }

// impl MaterialExtension for BuildShader {
//     fn fragment_shader() -> ShaderRef {
//         BUILD_SHADER_ASSET_PATH.into()
//     }

//     fn deferred_fragment_shader() -> ShaderRef {
//         BUILD_SHADER_ASSET_PATH.into()
//     }
// }

fn deser_color<'de, D>(deserializer: D) -> Result<LinearRgba, D::Error>
where D: Deserializer<'de> {
    let buf = <String>::deserialize(deserializer)?;
    Ok(Srgba::hex(buf).unwrap_or(Srgba::WHITE).into())
}

pub type MapMaterial = ExtendedMaterial<StandardMaterial, TerrainShader>;
//pub type BuildMaterial = ExtendedMaterial<StandardMaterial, BuildShader>;

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
    pub grass_color: LinearRgba,
    #[serde(deserialize_with = "deser_color")]
    pub ocean_color: LinearRgba,
    #[serde(deserialize_with = "deser_color")]
    pub mountain_color: LinearRgba,
    #[serde(deserialize_with = "deser_color")]
    pub snow_color: LinearRgba,
    #[serde(deserialize_with = "deser_color")]
    pub sand_color: LinearRgba,
}

// #[derive(Deserialize)]
// pub struct BuildMaterialParams {
//     #[serde(default)]
//     #[serde(flatten)]
//     pub pbr: StandardMaterialParams, 
//     #[serde(deserialize_with = "deser_color")]
//     pub highlight_color: LinearRgba,
// }

#[derive(Default)]
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
            grass_color: mat_params.grass_color,
            ocean_color: mat_params.ocean_color,
            mountain_color: mat_params.mountain_color,
            snow_color: mat_params.snow_color,
            sand_color: mat_params.sand_color,
        };
        Ok(MapMaterial {base, extension})
    }

    fn extensions(&self) -> &[&str] {
        &["mapmat"]
    }
}


// #[derive(Default)]
// pub struct BuildMaterialLoader;

// impl AssetLoader for BuildMaterialLoader {
//     type Asset = BuildMaterial;

//     type Settings = ();

//     type Error = anyhow::Error;

//     async fn load(
//         &self,
//         reader: &mut dyn bevy::asset::io::Reader,
//         _settings: &Self::Settings,
//         load_context: &mut LoadContext<'_>,
//     ) -> Result<Self::Asset, Self::Error> {
//         let mut bytes = Vec::new();

//         reader.read_to_end(&mut bytes).await?;
//         let mat_params = ron::de::from_bytes::<BuildMaterialParams>(&bytes)?;
//         let base = mat_params.pbr.to_mat(load_context);
//         let extension = BuildShader {
//             highlight_color: mat_params.highlight_color,
//         };
//         Ok(BuildMaterial {base, extension})
//     }

//     fn extensions(&self) -> &[&str] {
//         &["bmat"]
//     }
// }
