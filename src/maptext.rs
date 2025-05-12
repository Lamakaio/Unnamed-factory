use bevy::{pbr::MaterialExtension, prelude::*, render::render_resource::*};

/// This example uses a shader source file from the assets subdirectory
const SHADER_ASSET_PATH: &str = "shaders/extended_material.wgsl";

#[derive(Asset, AsBindGroup, PartialEq, Debug, Clone, Component, Reflect)]
#[reflect(PartialEq)]
pub struct TerrainShader {
    #[texture(100)]
    #[sampler(101)]
    pub mask: Handle<Image>,
    /// The parts of the model that are facing the light source and are not in shadow.
    #[uniform(102)]
    pub highlight_color: LinearRgba,
    /// The parts of the model that are not facing the light source and are in shadow.
    #[uniform(103)]
    pub shadow_color: LinearRgba,
    /// The color of the edge of the model, which gets a slight specular highlight to make the model pop.
    #[uniform(104)]
    pub rim_color: LinearRgba,
    #[uniform(105)]
    pub grass_color: LinearRgba,
    #[uniform(106)]
    pub ocean_color: LinearRgba,
    #[uniform(107)]
    pub mountain_color: LinearRgba,
    #[uniform(108)]
    pub snow_color: LinearRgba,
    #[uniform(109)]
    pub sand_color: LinearRgba,
}

impl MaterialExtension for TerrainShader {
    fn fragment_shader() -> ShaderRef {
        SHADER_ASSET_PATH.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        SHADER_ASSET_PATH.into()
    }
}
