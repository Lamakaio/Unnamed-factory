use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages,
    math::{I64Vec2, NormedVectorSpace},
    pbr::{
        decal::{ForwardDecal, ForwardDecalMaterial, ForwardDecalMaterialExt},
        wireframe::{Wireframe, WireframeColor},
    },
    prelude::*,
    render::{
        primitives::Aabb,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    },
};
use serde::{Deserialize, Serialize};

use crate::map::{Chunk, GRID_SQUARE_SIZE, Map, PatchOp};

/// An id for a building, serve to identify which building corresponds to a mesh.
#[derive(Clone, Component, PartialEq)]
pub struct BuildId(pub Arc<Building>);

/// The part currently selected, that follow the mouse
#[derive(Component)]
pub struct SelectedBuild {
    resizable: bool,
}

/// Multiples of grid square the selection snaps to
#[derive(Resource)]
pub enum Snapping {
    None,
    One,
    Two,
    Four,
}

pub struct BuildPlugin;

impl Plugin for BuildPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_parts);
        app.add_systems(
            Update,
            (
                spawn_build_from_part_id,
                build_follow_cursor,
                place_build,
                snapping_mode,
            ),
        );
        app.insert_resource(Buildings::default());
        app.insert_resource(SavedShapes::default());
        app.insert_resource(Snapping::One);
    }
}

/// A building (to be modifed with everything needed)
pub struct Building {
    pub typ: BuildingType,
    pub config: BuildConfig,
    pub size: I64Vec2,
}

impl PartialEq for Building {
    fn eq(&self, other: &Self) -> bool {
        self.config.name == other.config.name
    }
}

/// Contains the material and mesh for a building. (and maybe pther things in the future)
pub struct BuildModel {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
}

/// Split between zoning and individual buildings (and maybe fmroe things in the future, e.g. roads)
pub enum BuildingType {
    Zone { color: Color },
    Single { model: BuildModel },
    Tool { op: PatchOp },
}

/// in theory, whatever is used to store the building as an asset on disk. Might change in the future.
#[derive(Serialize, Deserialize)]
pub struct BuildConfig {
    pub name: String,
}

impl BuildConfig {
    fn placeholder(i: usize) -> Self {
        Self {
            name: format!("placeholder {i}"),
        }
    }
}
/// A collection of all buildings in the game.
#[derive(Resource, Default)]
pub struct Buildings(pub Vec<Arc<Building>>);

#[derive(Resource, Default)]
pub struct SavedShapes(Vec<Handle<Mesh>>);

/// Generate the parts, that will later serve to generate the buttons.
pub fn setup_parts(
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut parts: ResMut<Buildings>,
    mut shapes: ResMut<SavedShapes>,
    //asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let debug_material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(uv_debug_texture())),
        ..default()
    });

    shapes.0.push(meshes.add(Cuboid::default()));

    shapes.0.push(meshes.add(Tetrahedron::default()));
    shapes.0.push(meshes.add(Capsule3d::default()));
    shapes.0.push(meshes.add(Torus::default()));
    shapes.0.push(meshes.add(Cylinder::default()));
    shapes.0.push(meshes.add(Cone::default()));
    shapes.0.push(meshes.add(ConicalFrustum::default()));
    shapes
        .0
        .push(meshes.add(Sphere::default().mesh().ico(5).unwrap()));
    shapes
        .0
        .push(meshes.add(Sphere::default().mesh().uv(32, 18)));

    let extrusions = [
        meshes.add(Extrusion::new(Rectangle::default(), 1.)),
        meshes.add(Extrusion::new(Capsule2d::default(), 1.)),
        meshes.add(Extrusion::new(Annulus::default(), 1.)),
        meshes.add(Extrusion::new(Circle::default(), 1.)),
        meshes.add(Extrusion::new(Ellipse::default(), 1.)),
        meshes.add(Extrusion::new(RegularPolygon::default(), 1.)),
        meshes.add(Extrusion::new(Triangle2d::default(), 1.)),
    ];

    for (i, shape) in shapes
        .0
        .iter()
        .cloned()
        .chain(extrusions.into_iter())
        .enumerate()
    {
        parts.0.push(Arc::new(Building {
            typ: BuildingType::Single {
                model: BuildModel {
                    mesh: shape.clone(),
                    material: debug_material.clone(),
                },
            },
            config: BuildConfig::placeholder(i),
            size: (1, 1).into(),
        }));
    }

    parts.0.push(Arc::new(Building {
        typ: BuildingType::Zone {
            color: Color::from(bevy::color::palettes::css::LIGHT_GREEN),
        },
        config: BuildConfig {
            name: "a_zonetest".into(),
        },
        size: (0, 0).into(),
    }));

    parts.0.push(Arc::new(Building {
        typ: BuildingType::Tool { op: PatchOp::Up },
        config: BuildConfig {
            name: "patch up".into(),
        },
        size: (0, 0).into(),
    }));

    parts.0.push(Arc::new(Building {
        typ: BuildingType::Tool { op: PatchOp::Down },
        config: BuildConfig {
            name: "patch down".into(),
        },
        size: (0, 0).into(),
    }));

    parts.0.push(Arc::new(Building {
        typ: BuildingType::Tool { op: PatchOp::Flatten },
        config: BuildConfig {
            name: "patch flatten".into(),
        },
        size: (0, 0).into(),
    }));
}

/// Creates a colorful test pattern
fn uv_debug_texture() -> Image {
    const TEXTURE_SIZE: usize = 8;

    let mut palette: [u8; 32] = [
        255, 102, 159, 255, 255, 159, 102, 255, 236, 255, 102, 255, 121, 255, 102, 255, 102, 255,
        198, 255, 102, 198, 255, 255, 121, 102, 255, 255, 236, 102, 255, 255,
    ];

    let mut texture_data = [0; TEXTURE_SIZE * TEXTURE_SIZE * 4];
    for y in 0..TEXTURE_SIZE {
        let offset = TEXTURE_SIZE * y * 4;
        texture_data[offset..(offset + TEXTURE_SIZE * 4)].copy_from_slice(&palette);
        palette.rotate_right(4);
    }

    Image::new_fill(
        Extent3d {
            width: TEXTURE_SIZE as u32,
            height: TEXTURE_SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &texture_data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[derive(Component)]
struct ToolInstance {
    op: PatchOp,
    radius: f32,
    strength: f32,
}

/// Spawn the actual building mesh when a BuildId is spawned
fn spawn_build_from_part_id(
    mut commands: Commands,
    shapes: Res<SavedShapes>,
    interaction_query: Query<(Entity, &BuildId), Without<Transform>>,
    button: Res<ButtonInput<MouseButton>>,
    selected_part_query: Option<Single<Entity, With<SelectedBuild>>>,
    asset_server: Res<AssetServer>,
    mut decal_standard_materials: ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>,
) {
    if button.pressed(MouseButton::Left) {
        return;
    }

    if let Some(selpart) = selected_part_query {
        if !interaction_query.is_empty() {
            commands.entity(*selpart).despawn()
        };
    }

    for (e, p) in &interaction_query {
        let part = &p.0;

        match &part.typ {
            BuildingType::Single { model } => commands.entity(e).insert((
                Mesh3d(model.mesh.clone()),
                MeshMaterial3d(model.material.clone()),
                Transform::default(),
                SelectedBuild { resizable: false },
                Visibility::Hidden,
            )),
            BuildingType::Zone { color } => commands.entity(e).insert((
                Mesh3d(shapes.0[0].clone()),
                Wireframe,
                WireframeColor {
                    color: color.clone(),
                },
                Transform::default(),
                SelectedBuild { resizable: true },
                Visibility::Hidden,
            )),
            BuildingType::Tool { op } => commands.entity(e).insert((
                ToolInstance {
                    op: *op,
                    radius: 5.0,
                    strength: 1.0,
                },
                ForwardDecal,
                MeshMaterial3d(decal_standard_materials.add(ForwardDecalMaterial {
                    base: StandardMaterial {
                        base_color_texture: Some(asset_server.load("img/circle.png")),
                        alpha_mode: AlphaMode::Blend,
                        base_color: bevy::color::palettes::css::RED.into(),
                        ..default()
                    },
                    extension: ForwardDecalMaterialExt {
                        depth_fade_factor: 1.0,
                    },
                })),
                Transform::from_scale(Vec3::splat(10.0)),
                SelectedBuild { resizable: false },
                Visibility::Hidden,
            )),
        };
    }
}

//const DEFAULT_RAY_DISTANCE: f32 = 10.;

/// Make the selected part follow the cursor
fn build_follow_cursor(
    mut ray_cast: MeshRayCast,
    camera_query: Single<(&Camera, &GlobalTransform)>,
    windows: Single<&Window>,
    selected_part_query: Option<
        Single<(
            Entity,
            &mut Transform,
            &SelectedBuild,
            &Aabb,
            &mut Visibility,
        )>,
    >,
    //map: Res<Map>,
    button: Res<ButtonInput<MouseButton>>,
    snapping: Res<Snapping>,
    mut place_point: Local<Vec2>,
) {
    let Some(selpart) = selected_part_query else {
        return;
    };
    let (camera, camera_transform) = *camera_query;

    let Some(cursor_position) = windows.cursor_position() else {
        return;
    };

    // Calculate a ray pointing from the camera into the world based on the cursor's position.
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };
    let (e, mut part_transform, selected_build, aabb, mut visibility) = selpart.into_inner();

    // Cast the ray to get hit to the nearest different object

    let filter = |entity: Entity| entity != e;
    let settings = MeshRayCastSettings::default()
        .always_early_exit()
        .with_filter(&filter);
    let hits = ray_cast.cast_ray(ray, &settings);

    let (point, _normal) = if let Some((_, hit)) = hits.first() {
        *visibility = Visibility::Visible;
        (hit.point, hit.normal.normalize())
    } else {
        *visibility = Visibility::Hidden;
        (Vec3::ZERO, Vec3::Y)
    };

    let point2d = Vec2::new(point.x, point.z);

    let point2d = match *snapping {
        Snapping::None => point2d,
        Snapping::One => (point2d / GRID_SQUARE_SIZE).round() * GRID_SQUARE_SIZE,
        Snapping::Two => (point2d / (2. * GRID_SQUARE_SIZE)).round() * 2. * GRID_SQUARE_SIZE,
        Snapping::Four => (point2d / (4. * GRID_SQUARE_SIZE)).round() * 4. * GRID_SQUARE_SIZE,
    };

    let he = part_transform
        .rotation
        .mul_vec3(Vec3::from(aabb.half_extents));
    let he_proj = part_transform
        .rotation
        .mul_vec3(Vec3::from(aabb.half_extents))
        .project_onto(Vec3::Y);
    if selected_build.resizable
        && (button.pressed(MouseButton::Left) || button.just_pressed(MouseButton::Left))
    {
        let scale = point2d - *place_point;
        let scale = scale.abs().max(Vec2::new(0.1, 0.1)) * scale.signum();
        part_transform.scale = Vec3::new(scale.x, 1., scale.y);
        part_transform.translation =
            Vec3::new(place_point.x, 0., place_point.y) + he * part_transform.scale;
    } else if !button.just_released(MouseButton::Left) {
        *place_point = point2d;
        //part_transform.rotation = Quat::from_rotation_arc(Vec3::Y, normal);
        part_transform.translation = Vec3::new(place_point.x, point.y, place_point.y) + he_proj;
    }
}

/// Actually place a part on click
fn place_build(
    mut commands: Commands,
    selected_part_query: Option<
        Single<(Entity, &Transform, Option<&ToolInstance>, &Aabb), With<SelectedBuild>>,
    >,
    mut map: ResMut<Map>,
    button: Res<ButtonInput<MouseButton>>,
    key: Res<ButtonInput<KeyCode>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if button.just_released(MouseButton::Left) {
        if let Some(query) = selected_part_query {
            let (e, transform, tool, aabb) = *query;
            let (trsl, radius, op) = if let Some(ti) = tool {
                (transform.translation, ti.radius, ti.op)
            } else {
                (
                    transform.translation - Vec3::new(0., aabb.half_extents.y - 0.05, 0.),
                    aabb.half_extents.xz().norm() * 2.,
                    PatchOp::Flatten,
                )
            };
            let chunk_pos_x = (transform.translation.x / Chunk::WORLD_CHUNK_SIZE).floor() as i64;
            let chunk_pos_z = (transform.translation.z / Chunk::WORLD_CHUNK_SIZE).floor() as i64;
            let chunk = map.get_chunk_mut(&(chunk_pos_x, chunk_pos_z).into());
            //TODO too convoluted here. Make separate chunk intersect detection.
            let add_patches = chunk.patch(&mut *meshes, &trsl, radius, op);
            for (off_x, off_z) in add_patches {
                let chunk = map.get_chunk_mut(&(chunk_pos_x + off_x, chunk_pos_z + off_z).into());
                chunk.patch(&mut *meshes, &trsl, radius, op);
            }
            if !(key.pressed(KeyCode::ControlLeft) || key.pressed(KeyCode::ControlRight)) {
                commands.entity(e).remove::<SelectedBuild>();
            }
        }
    }
}

/// Change the snapping mode by cycling on pressing S
fn snapping_mode(mut snapping: ResMut<Snapping>, keyboard_input: Res<ButtonInput<KeyCode>>) {
    if keyboard_input.just_pressed(KeyCode::KeyS) {
        *snapping = match &*snapping {
            Snapping::None => Snapping::One,
            Snapping::One => Snapping::Two,
            Snapping::Two => Snapping::Four,
            Snapping::Four => Snapping::None,
        }
    }
}
