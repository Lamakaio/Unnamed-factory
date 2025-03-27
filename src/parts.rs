use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages, math::NormedVectorSpace, pbr::wireframe::{Wireframe, WireframeColor}, prelude::*, render::{
        primitives::Aabb,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    }, scene::SceneInstance
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Component)]
pub struct BuildId(pub Arc<Building>);

#[derive(Component)]
pub struct SelectedBuild {
    resizable: bool,
}

pub struct BuildPlugin;

impl Plugin for BuildPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_parts);
        app.add_systems(
            Update,
            (spawn_build_from_part_id, build_follow_cursor, place_build),
        );
        app.insert_resource(Buildings::default());
        app.insert_resource(SavedShapes::default());
    }
}

pub struct Building {
    pub typ: BuildingType,
    pub config: BuildConfig,
}

pub enum BuildModelType {
    Scene(Handle<Scene>),
    MeshMaterial(Handle<Mesh>, Handle<StandardMaterial>),
}

pub enum BuildingType {
    Zone { color: Color },
    Single { model: BuildModelType },
}
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

#[derive(Resource, Default)]
pub struct Buildings(pub Vec<Arc<Building>>);

#[derive(Resource, Default)]
pub struct SavedShapes(Vec<Handle<Mesh>>);

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
                model: BuildModelType::MeshMaterial(shape.clone(), debug_material.clone()),
            },
            config: BuildConfig::placeholder(i),
        }));
    }

    parts.0.push(Arc::new(Building {
        typ: BuildingType::Zone {
            color: Color::from(bevy::color::palettes::css::LIGHT_GREEN),
        },
        config: BuildConfig {
            name: "a_zonetest".into(),
        },
    }))
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

fn spawn_build_from_part_id(
    mut commands: Commands,
    shapes: Res<SavedShapes>,
    interaction_query: Query<(Entity, &BuildId), Without<Transform>>,
    button: Res<ButtonInput<MouseButton>>,
) {
    if button.pressed(MouseButton::Left) {
        return;
    }
    for (e, p) in &interaction_query {
        let part = &p.0;

        match &part.typ {
            BuildingType::Single {
                model: BuildModelType::Scene(scene),
            } => commands.entity(e).insert((
                SceneRoot(scene.clone()),
                Transform::default(),
                SelectedBuild { resizable: false },
            )),
            BuildingType::Single {
                model: BuildModelType::MeshMaterial(mesh, mat),
            } => commands.entity(e).insert((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::default(),
                SelectedBuild { resizable: false },
            )),
            BuildingType::Zone { color } => commands.entity(e).insert((
                Mesh3d(shapes.0[0].clone()),
                Wireframe,
                WireframeColor {
                    color: color.clone(),
                },
                Transform::default(),
                SelectedBuild { resizable: true },
            )),
        };
    }
}

//const DEFAULT_RAY_DISTANCE: f32 = 10.;

fn build_follow_cursor(
    mut ray_cast: MeshRayCast,
    scene_spawner: Res<SceneSpawner>,
    camera_query: Single<(&Camera, &GlobalTransform)>,
    windows: Single<&Window>,
    selected_part_query: Option<Single<(Entity, &mut Transform, &SelectedBuild, &Aabb)>>,
    button: Res<ButtonInput<MouseButton>>,
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
    let (e, mut part_transform, selected_build, aabb) = selpart.into_inner();

    // Cast the ray to get hit to the nearest different object

    let filter = |entity: Entity| entity != e;
    let settings = RayCastSettings::default()
        .always_early_exit()
        .with_filter(&filter);
    let hits = ray_cast.cast_ray(ray, &settings);

    let (point, normal) = if let Some((_, hit)) = hits.first() {
        (hit.point, hit.normal.normalize())
    } else {
        (Vec3::ZERO, Vec3::Y)
    };

    if selected_build.resizable && button.pressed(MouseButton::Left) {

    }
    else {
        dbg!(point, aabb.half_extents);
        let he = Vec3::from(aabb.half_extents);
        part_transform.rotation = Quat::from_rotation_arc(Vec3::Y, normal);
        part_transform.translation = point + part_transform.rotation.mul_vec3(he);
    }

}

fn place_build(
    mut commands: Commands,
    selected_part_query: Option<Single<(Entity,), With<SelectedBuild>>>,
    button: Res<ButtonInput<MouseButton>>,
) {
    if button.just_released(MouseButton::Left) {
        if let Some(query) = selected_part_query {
            let (e,) = *query;
            commands.entity(e).remove::<SelectedBuild>();
        }
    }
}
