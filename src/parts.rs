use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    utils::HashMap,
};

#[derive(Clone, Copy, Hash, PartialEq, Eq, Component)]
pub struct PartId(usize);

#[derive(Component)]
pub struct SelectedPart;
pub struct PartsPlugin;

impl Plugin for PartsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_parts);
        app.add_systems(
            Update,
            (spawn_parts_from_part_id, part_follow_cursor, place_part),
        );
        app.insert_resource(PartIdMap::default());
    }
}

pub struct Part {
    pub name: String,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
pub struct PartIdMap(pub HashMap<PartId, Part>);

pub fn setup_parts(
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut parts: ResMut<PartIdMap>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let debug_material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(uv_debug_texture())),
        ..default()
    });

    let shapes = [
        meshes.add(Cuboid::default()),
        meshes.add(Tetrahedron::default()),
        meshes.add(Capsule3d::default()),
        meshes.add(Torus::default()),
        meshes.add(Cylinder::default()),
        meshes.add(Cone::default()),
        meshes.add(ConicalFrustum::default()),
        meshes.add(Sphere::default().mesh().ico(5).unwrap()),
        meshes.add(Sphere::default().mesh().uv(32, 18)),
    ];

    let extrusions = [
        meshes.add(Extrusion::new(Rectangle::default(), 1.)),
        meshes.add(Extrusion::new(Capsule2d::default(), 1.)),
        meshes.add(Extrusion::new(Annulus::default(), 1.)),
        meshes.add(Extrusion::new(Circle::default(), 1.)),
        meshes.add(Extrusion::new(Ellipse::default(), 1.)),
        meshes.add(Extrusion::new(RegularPolygon::default(), 1.)),
        meshes.add(Extrusion::new(Triangle2d::default(), 1.)),
    ];

    for (i, shape) in shapes.into_iter().chain(extrusions.into_iter()).enumerate() {
        parts.0.insert(
            PartId(i),
            Part {
                name: format!("part number {i}"),
                mesh: shape,
                material: debug_material.clone(),
            },
        );
    }
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

fn spawn_parts_from_part_id(
    mut commands: Commands,
    parts: Res<PartIdMap>,
    interaction_query: Query<(Entity, &PartId), Without<Transform>>,
) {
    for (e, p) in &interaction_query {
        let part = &parts.0[p];
        commands.entity(e).insert((
            Mesh3d(part.mesh.clone()),
            MeshMaterial3d(part.material.clone()),
            Transform::default(),
            SelectedPart,
        ));
    }
}

//const DEFAULT_RAY_DISTANCE: f32 = 10.;

fn part_follow_cursor(
    mut ray_cast: MeshRayCast,
    camera_query: Single<(&Camera, &GlobalTransform)>,
    windows: Single<&Window>,
    selected_part_query: Option<Single<(Entity, &mut Transform), With<SelectedPart>>>,
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
    let (e, mut part_transform) = selpart.into_inner();
    // Cast the ray to get hit to the nearest different object
    let filter = |entity: Entity| entity != e;
    let settings = RayCastSettings::default().always_early_exit().with_filter(&filter);
    let hits = ray_cast.cast_ray(ray, &settings);
    
    let (point, normal) = if let Some((_, hit)) = hits.first() {
        (hit.point, hit.normal.normalize())
    } else {
        (Vec3::ZERO, Vec3::Y)
    };
    
    part_transform.translation = point;
    part_transform.rotation = Quat::from_rotation_arc(Vec3::Y, normal);
}

fn place_part(
    mut commands: Commands,
    selected_part_query: Option<Single<(Entity,), With<SelectedPart>>>,
    button: Res<ButtonInput<MouseButton>>,
) {
    if button.just_pressed(MouseButton::Left) {
        if let Some(query) = selected_part_query {
            let (e,) = *query;
            commands.entity(e).remove::<SelectedPart>();
        }
    }
}
