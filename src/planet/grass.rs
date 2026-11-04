//! Grass blades as geometry very close to the camera: the
//! terrain shader's grass carpet flattens at grazing angles, so real blades stand in front of
//! it. Tiles are 1×1 squares of a face's cells around the point the camera looks at, only
//! where the terrain draws grass (`planet_terrain.wesl`: not forest floors, desert, beaches,
//! snow, steep rock or river beds), coloured like it (lush to dry, alpine where cold).
//!
//! Each tile is turned upright on the sphere with its local x downwind (the local wind,
//! `PlanetSurface::wind_at`): the blade shader (`grass_blades.wesl`) bends blades along local
//! x, and runs its gusts on (along, across the wind) in the face's coordinates (UV1), so they
//! carry on from tile to tile.

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::VisibilityRange,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    platform::collections::HashMap,
    prelude::*,
};
use worldgen::surface::PlanetSurface;

use super::PlanetWorld;
use crate::{
    AppState,
    shaders::{GrassBladeMaterial, GrassBladeShader},
};

/// Blades are shown while the camera is at most this high above the ground...
const MAX_CAMERA_HEIGHT: f32 = 7.;
/// ...on tiles within this distance (cells) of the point it looks at, fading out at this
/// distance from the camera.
const RADIUS: f32 = 6.;
const FADE: std::ops::Range<f32> = 4.5..7.;
/// Blades per tile of full grass, their height and width (world units).
const BLADES: usize = 500;
const HEIGHT: (f32, f32) = (0.05, 0.11);
const WIDTH: f32 = 0.012;
/// New tiles built per frame.
const TILES_PER_FRAME: usize = 12;

pub(super) struct PlanetGrassPlugin;

impl Plugin for PlanetGrassPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            update_near_grass
                .after(super::camera::apply)
                .run_if(in_state(AppState::InGame))
                .run_if(resource_exists::<PlanetWorld>),
        );
    }
}

#[derive(Default)]
struct NearGrass {
    material: Option<Handle<GrassBladeMaterial>>,
    /// Tiles by face and lowest cell coordinates.
    tiles: HashMap<(usize, IVec2), Entity>,
}

fn update_near_grass(
    mut commands: Commands,
    camera: Single<&Transform, With<Camera3d>>,
    world: Res<PlanetWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GrassBladeMaterial>>,
    mut state: Local<NearGrass>,
) {
    let surface = &world.surface;
    let material = state
        .material
        .get_or_insert_with(|| {
            materials.add(GrassBladeMaterial {
                base: StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 1.,
                    // Both faces drawn, but not double-sided lighting.
                    cull_mode: None,
                    ..default()
                },
                extension: GrassBladeShader {
                    tint: LinearRgba::WHITE,
                    snow: Vec4::ZERO,
                    // Tiles are turned with their x downwind.
                    wind: Vec4::new(1., 0., 0., 0.),
                },
            })
        })
        .clone();

    // The tiles around the point the camera looks at, if it is close to the ground.
    let eye = camera.translation;
    let up = eye.normalize_or_zero();
    let height = eye.length() - surface.radius - surface.height_at(up);
    let mut wanted = Vec::new();
    if height < MAX_CAMERA_HEIGHT {
        let forward = camera.forward();
        let reach = (height / (-forward.dot(up)).max(0.2)).min(RADIUS);
        let (face, center) = surface.face_coords((eye + forward * reach).normalize());
        let r = RADIUS.ceil() as i32;
        let base = center.floor().as_ivec2();
        for dz in -r..=r {
            for dx in -r..=r {
                let tile = base + IVec2::new(dx, dz);
                let middle = tile.as_vec2() + 0.5;
                if middle.distance(center) <= RADIUS
                    && surface.position(face, middle).distance(eye) <= FADE.end + 1.
                {
                    wanted.push((face, tile));
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
    let distance = |(face, tile): &(usize, IVec2)| {
        surface
            .direction(*face, tile.as_vec2() + 0.5)
            .distance_squared(up)
    };
    wanted.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    for (face, tile) in wanted.into_iter().take(TILES_PER_FRAME) {
        let entity = match tile_mesh(surface, face, tile) {
            Some((mesh, transform)) => commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material.clone()),
                    // At ground level: the visibility range measures from here.
                    transform,
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
        state.tiles.insert((face, tile), entity);
    }
}

/// Grass colour (linear) and density at a point, as the terrain shader draws it; `None` where
/// it draws no grass.
fn grass_at(surface: &PlanetSurface, face: usize, p: Vec2) -> Option<(Vec3, f32)> {
    if !surface.life {
        return None;
    }
    let srgb = |r: f32, g: f32, b: f32| Vec3::new(r, g, b).powf(2.2);
    let Some(weights) = &surface.weights else {
        return Some((srgb(0.40, 0.66, 0.24), 1.));
    };
    let [dryness, forest, desert, snow, cold] = weights.each_ref().map(|w| w.bilinear(face, p));
    // (Greener along rivers in dry country, as the terrain: see `mesh::ground_materials`.)
    let riparian = surface.riparian.bilinear(face, p);
    let dryness = dryness * (1. - super::mesh::RIPARIAN_GREEN * riparian);
    let desert = desert * (1. - riparian);
    if forest > 0.5
        || desert > 0.5
        || snow > 0.5
        || surface.marsh.bilinear(face, p).abs() > 0.5
        || surface.lava.bilinear(face, p) > 0.4
        || surface.ice.bilinear(face, p) > 0.2
        || surface.badlands.bilinear(face, p) > 0.5
        || surface.geothermal.bilinear(face, p) > 0.3
    {
        return None;
    }
    let dry = (dryness * 3.).round() / 3.;
    let lush = srgb(0.40, 0.66, 0.24).lerp(srgb(0.80, 0.72, 0.38), dry);
    let alpine = (1. - forest) * cold * 0.8;
    let color = if alpine > 0.5 {
        srgb(0.60, 0.68, 0.42)
    } else {
        lush
    };
    let density = if alpine > 0.5 { 0.6 } else { 1. - 0.1 * dry };
    Some((color, density * (1. - forest)))
}

/// The blades of one tile and its transform (upright, local x downwind, at its centre on the
/// ground); `None` if it has none.
fn tile_mesh(surface: &PlanetSurface, face: usize, tile: IVec2) -> Option<(Mesh, Transform)> {
    let origin = tile.as_vec2();
    let center = origin + 0.5;
    let up = surface.direction(face, center);
    let ground = surface.position(face, center);
    let (wind, _) = surface.wind_at(face, center);
    let downwind = wind
        .reject_from(up)
        .try_normalize()
        .unwrap_or(up.any_orthonormal_vector());
    let rotation = Quat::from_mat3(&Mat3::from_cols(downwind, up, downwind.cross(up)));
    let to_local = rotation.inverse();
    // The wind in face coordinates, for the gust pattern (along / across it).
    let ex = surface.direction(face, center + Vec2::X) - surface.direction(face, center - Vec2::X);
    let ey = surface.direction(face, center + Vec2::Y) - surface.direction(face, center - Vec2::Y);
    let wind_2d = Vec2::new(
        downwind.dot(ex) / ex.length_squared(),
        downwind.dot(ey) / ey.length_squared(),
    )
    .normalize_or(Vec2::X);
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut pattern: Vec<[f32; 2]> = Vec::new();
    let mut seed = (tile.x as u32).wrapping_mul(0x9E37_79B9)
        ^ (tile.y as u32).wrapping_mul(0x85EB_CA6B)
        ^ (face as u32 + 1).wrapping_mul(0xC2B2_AE35);
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for _ in 0..BLADES {
        let p = origin + Vec2::new(random(), random());
        let (r1, r2, r3, r4) = (random(), random(), random(), random());
        let Some((color, density)) = grass_at(surface, face, p) else {
            continue;
        };
        if r1 > density {
            continue;
        }
        let height = surface.height(face, p);
        let position = surface.direction(face, p) * (surface.radius + height);
        let e = 0.2;
        let dx = surface.position(face, p + Vec2::X * e) - surface.position(face, p - Vec2::X * e);
        let dy = surface.position(face, p + Vec2::Y * e) - surface.position(face, p - Vec2::Y * e);
        let steepness = 1. - dx.cross(dy).normalize_or_zero().dot(up).abs();
        // (The terrain shader puts sand below 0.15–0.45 above the water.)
        let beach = height < 0.3 && surface.ocean.bilinear(face, p) > super::mesh::COAST.0;
        if surface.wetness(face, p) > 0.02 || steepness > 0.3 || beach {
            continue;
        }
        let base = to_local * (position - ground);
        let blade = HEIGHT.0 + (HEIGHT.1 - HEIGHT.0) * r2;
        let side = Vec2::from_angle(r3 * std::f32::consts::TAU) * WIDTH / 2.;
        let lean = Vec2::from_angle(r4 * std::f32::consts::TAU) * blade * 0.25;
        let tip = base + lean.extend(blade).xzy();
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
        let along = [p.dot(wind_2d), p.dot(wind_2d.perp())];
        pattern.extend([along; 3]);
    }
    if positions.is_empty() {
        return None;
    }
    let count = positions.len();
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    // Lit like the ground under them.
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 1., 0.]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, pattern)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32((0..count as u32).collect()));
    Some((
        mesh,
        Transform::from_translation(ground).with_rotation(rotation),
    ))
}
