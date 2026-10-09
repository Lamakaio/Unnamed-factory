//! Ground materials per continent cell, baked into two textures for the terrain shader, which
//! draws every material procedurally (see `assets/shaders/map_material.wesl`).
//!
//! - `ground[0]`: sand, rock, snow, marsh weights.
//! - `ground[1]`: dryness (0 lush .. 1 dry grass), deciduous forest floor, conifer forest floor,
//!   alpine grass.
//!
//! Steep slopes, river beds and the sea floor are decided per pixel by the shader.

use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use worldgen::{Biome, Continent, noise::smoothstep};

/// The ground textures and the mapping from world xz to their coordinates
/// (`uv = xz * zw + xy`).
pub fn ground_textures(continent: &Continent) -> ([Image; 2], Vec4) {
    let size = continent.size();
    let mut a = Vec::with_capacity(size * size * 4);
    let mut b = Vec::with_capacity(size * size * 4);
    for i in 0..size * size {
        let biome = continent.biome[i];
        let weights = |table: &[(Biome, f32)]| {
            table
                .iter()
                .find(|(b, _)| *b == biome)
                .map_or(0., |(_, w)| *w)
        };
        let sand = weights(&[(Biome::Beach, 1.), (Biome::Sea, 1.), (Biome::Lake, 0.4)]);
        let rock = weights(&[(Biome::Rock, 1.), (Biome::Alpine, 0.25), (Biome::Snow, 0.2)]);
        let marsh = weights(&[(Biome::Marsh, 1.), (Biome::Lake, 0.6)]);
        let deciduous = weights(&[(Biome::DeciduousForest, 1.)]);
        let conifer = weights(&[(Biome::ConiferForest, 1.)]);
        // Continuous fields where the climate is continuous, so grass and snow change gradually.
        let temperature = continent.temperature[i];
        let snow = smoothstep(0.12, 0.04, temperature);
        let alpine = smoothstep(0.4, 0.2, temperature) * (1. - snow);
        let dryness = smoothstep(1.05, 0.35, continent.moisture[i]);
        a.extend([sand, rock, snow, marsh].map(unorm));
        b.extend([dryness, deciduous, conifer, alpine].map(unorm));
    }
    let image = |data| {
        let mut image = Image::new(
            Extent3d {
                width: size as u32,
                height: size as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::linear();
        image
    };
    // Texel k is centered on world (k - size / 2) * cell.
    let cell = continent.settings.cell_size;
    let scale = 1. / (cell * size as f32);
    let offset = ((size / 2) as f32 + 0.5) / size as f32;
    (
        [image(a), image(b)],
        Vec4::new(offset, offset, scale, scale),
    )
}

fn unorm(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}
