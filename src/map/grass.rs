//! Grass blades as geometry, very close to the camera: the terrain shader's grass carpet
//! (`map_material.wesl::grass_carpet`) is a parallax layer, which flattens at grazing angles;
//! close up, real blades stand in front of it. They are built per 1×1 tile around the point
//! the camera looks at, only where the terrain draws grass, bend with the same wind as the
//! carpet (`grass_blades.wesl`), and fade out with distance (Bevy's visibility range dither),
//! leaving the carpet.

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::VisibilityRange,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    platform::collections::HashMap,
    prelude::*,
};
use worldgen::Biome;

use super::Map;
use crate::{
    AppState,
    shaders::{GrassBladeMaterial, GrassBladeShader},
};

/// Blades are shown while the camera is at most this high above the ground...
const MAX_CAMERA_HEIGHT: f32 = 7.;
/// ...on tiles within this distance of the point it looks at, fading out at this distance
/// from the camera.
const RADIUS: f32 = 6.;
const FADE: std::ops::Range<f32> = 4.5..7.;
/// Blades per tile (1×1 world unit) of full grass, their height and width (world units).
const BLADES: usize = 500;
const HEIGHT: (f32, f32) = (0.05, 0.11);
const WIDTH: f32 = 0.012;
/// New tiles built per frame.
const TILES_PER_FRAME: usize = 12;

pub struct GrassPlugin;

impl Plugin for GrassPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_near_grass.run_if(in_state(AppState::InGame)));
    }
}

#[derive(Default)]
struct NearGrass {
    material: Option<Handle<GrassBladeMaterial>>,
    tiles: HashMap<IVec2, Entity>,
}

fn update_near_grass(
    mut commands: Commands,
    camera: Single<&Transform, With<Camera3d>>,
    map: Res<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GrassBladeMaterial>>,
    mut state: Local<NearGrass>,
) {
    let material = state
        .material
        .get_or_insert_with(|| {
            let wind = Vec2::from_angle(map.continent.wind_angle);
            materials.add(GrassBladeMaterial {
                base: StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 1.,
                    // Both faces drawn, but not double-sided lighting: the back face would get
                    // a flipped (downward) normal and fall in shadow.
                    cull_mode: None,
                    ..default()
                },
                extension: GrassBladeShader {
                    tint: LinearRgba::WHITE,
                    snow: Vec4::ZERO,
                    wind: Vec4::new(wind.x, wind.y, 0., 0.),
                },
            })
        })
        .clone();

    // The tiles around the point the camera looks at, if it is close to the ground.
    let eye = camera.translation;
    let height = eye.y - map.get_height(eye);
    let mut wanted = Vec::new();
    if height < MAX_CAMERA_HEIGHT {
        let forward = camera.forward();
        let reach = (height / (-forward.y).max(0.2)).min(RADIUS);
        let center = (eye + forward * reach).xz();
        let r = RADIUS.ceil() as i32;
        let base = center.floor().as_ivec2();
        for dz in -r..=r {
            for dx in -r..=r {
                let tile = base + IVec2::new(dx, dz);
                let middle = tile.as_vec2() + 0.5;
                if middle.distance(center) <= RADIUS && middle.distance(eye.xz()) <= FADE.end + 1. {
                    wanted.push(tile);
                }
            }
        }
    }
    // Drop the tiles no longer wanted, build the missing ones (nearest first).
    state.tiles.retain(|tile, entity| {
        let keep = wanted.contains(tile);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    wanted.retain(|tile| !state.tiles.contains_key(tile));
    wanted.sort_by(|a, b| {
        let distance = |t: &IVec2| (t.as_vec2() + 0.5).distance_squared(eye.xz());
        distance(a).total_cmp(&distance(b))
    });
    for tile in wanted.into_iter().take(TILES_PER_FRAME) {
        let center = tile.as_vec2() + 0.5;
        let ground = map.get_height(center.extend(0.).xzy());
        let entity = match tile_mesh(&map, tile, ground) {
            Some(mesh) => commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material.clone()),
                    // At ground level: the visibility range measures from here.
                    Transform::from_xyz(center.x, ground, center.y),
                    VisibilityRange {
                        start_margin: 0.0..0.0,
                        end_margin: FADE,
                        use_aabb: false,
                    },
                    NotShadowCaster,
                    Pickable::IGNORE,
                ))
                .id(),
            // Remember empty tiles too, so they are not rebuilt every frame.
            None => commands.spawn(Transform::default()).id(),
        };
        state.tiles.insert(tile, entity);
    }
}

/// Grass colour (linear) and density of a biome, as the terrain shader draws it.
fn grass_of(biome: Biome) -> Option<(Vec3, f32)> {
    let srgb = |r: f32, g: f32, b: f32| Vec3::new(r, g, b).powf(2.2);
    match biome {
        Biome::Meadow => Some((srgb(0.40, 0.66, 0.24), 1.)),
        Biome::Steppe => Some((srgb(0.80, 0.72, 0.38), 0.9)),
        Biome::Alpine => Some((srgb(0.60, 0.68, 0.42), 0.6)),
        _ => None,
    }
}

/// The blades of one tile, relative to its center at height `ground`; `None` if it has none.
fn tile_mesh(map: &Map, tile: IVec2, ground: f32) -> Option<Mesh> {
    let origin = tile.as_vec2();
    let center = origin + 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut seed = (tile.x as u32).wrapping_mul(0x9E37_79B9) ^ (tile.y as u32).wrapping_mul(0x85EB_CA6B);
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for _ in 0..BLADES {
        let p = origin + Vec2::new(random(), random());
        let (r1, r2, r3, r4) = (random(), random(), random(), random());
        let Some((x, z)) = map.continent.world_to_cell(p) else {
            continue;
        };
        let biome = map.continent.biome[z * map.continent.size() + x];
        let Some((color, density)) = grass_of(biome) else {
            continue;
        };
        if r1 > density {
            continue;
        }
        let sample = map.sample(p.extend(0.).xzy());
        let steepness = 1. - map.heights.normal(p).y;
        if sample.wetness > 0.02
            || steepness > 0.3
            || sample.height < 0.15
            || map.heights.grass_clearance(p) > r1 * 0.5
        {
            continue;
        }
        let base = (p - center).extend(sample.height - ground).xzy();
        let height = HEIGHT.0 + (HEIGHT.1 - HEIGHT.0) * r2;
        let side = Vec2::from_angle(r3 * std::f32::consts::TAU) * WIDTH / 2.;
        let lean = Vec2::from_angle(r4 * std::f32::consts::TAU) * height * 0.25;
        let tip = base + lean.extend(height).xzy();
        positions.extend([
            (base - side.extend(0.).xzy()).to_array(),
            (base + side.extend(0.).xzy()).to_array(),
            tip.to_array(),
        ]);
        // Dark at the base, lighter at the tip, varied a little per blade.
        let shade = 0.95 + 0.2 * r2;
        let bottom = color * 0.8 * shade;
        let top = color * 1.1 * shade * Vec3::new(1.05, 1.05, 0.9);
        colors.extend([bottom, bottom, top].map(|c| c.extend(1.).to_array()));
        uvs.extend([[0., 0.], [0., 0.], [1., 0.]]);
    }
    if positions.is_empty() {
        return None;
    }
    let count = positions.len();
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            // Lit like the ground under them.
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 1., 0.]; count])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32((0..count as u32).collect())),
    )
}
