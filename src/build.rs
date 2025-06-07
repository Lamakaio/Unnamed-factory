use std::{process::Child, sync::Arc};

use bevy::{
    asset::{LoadedFolder, RenderAssetUsages},
    math::{NormedVectorSpace, VectorSpace},
    pbr::{
        decal::{ForwardDecal, ForwardDecalMaterial, ForwardDecalMaterialExt},
        wireframe::{Wireframe, WireframeColor},
    },
    prelude::*,
    render::{
        primitives::Aabb,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    }, text::cosmic_text::ttf_parser::ankr::Point,
};

use crate::{
    map::{BuildingInstance, Chunk, GRID_SQUARE_SIZE, IsGround, Map, PatchOp},
    sim::RhaiScript,
};

/// An id for a building, serve to identify which building corresponds to a mesh.
#[derive(Clone, Component, PartialEq, Default)]
pub struct BuildId(pub Handle<Building>);

/// The part currently selected, that follow the mouse
#[derive(Component)]
pub struct SelectedBuild;

/// Whether a part is resizable.
#[derive(Component)]
pub struct Resizable;

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
        app.add_systems(Startup, (setup_parts, setup_highlight));
        app.add_systems(
            Update,
            (
                spawn_build_from_part_id,
                build_follow_cursor,
                place_build,
                snapping_mode,
                select_world_part,
                compute_aabb,
            ),
        );
        app.add_observer(on_add_highlight);
        app.add_observer(on_remove_highlight);
        app.insert_resource(SavedShapes::default());
        app.insert_resource(Snapping::One);
        app.insert_resource(Buildings::default());
    }
}

/// A building (to be modifed with everything needed)
#[derive(Asset, TypePath, Debug)]
pub struct Building {
    pub typ: BuildingType,
    pub name: String,
    pub size: (u64, u64),
    pub script: Option<Handle<RhaiScript>>,
}

/// Split between zoning and individual buildings (and maybe fmroe things in the future, e.g. roads)
#[derive(Debug)]
pub enum BuildingType {
    Zone { color: Color },
    Single { model: Handle<Scene>, scale: f32 },
    Tool { op: PatchOp, color: Color },
}

#[derive(Component)]
pub struct Highlighted;

#[derive(Resource, Default)]
pub struct Buildings(pub Handle<LoadedFolder>);

#[derive(Resource, Default)]
pub struct SavedShapes(pub Vec<Handle<Mesh>>);

pub fn setup_highlight(mut commands: Commands) {
    commands.spawn((
        SpotLight {
            color: bevy::color::palettes::css::ORANGE_RED.into(),
            intensity: 1e9,
            range: 100.,
            outer_angle: 0.1,
            inner_angle: 0.02,
            ..default()
        },
        Transform::from_translation(Vec3::new(0., -10., 0.)),
        HighlightLight,
    ));
}
/// Generate the parts, that will later serve to generate the buttons.
pub fn setup_parts(
    mut meshes: ResMut<Assets<Mesh>>,
    mut shapes: ResMut<SavedShapes>,
    asset_server: Res<AssetServer>,
    mut buildings: ResMut<Buildings>,
) -> Result {
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

    buildings.0 = asset_server.load_folder("buildings");

    Ok(())
}

#[derive(Component)]
struct ToolInstance {
    op: PatchOp,
    radius: f32,
    strength: f32,
    color: Color,
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
    buildings: Res<Assets<Building>>,
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
        let part = buildings.get(&p.0).unwrap(); //FIXME

        match &part.typ {
            BuildingType::Single { model, scale } => commands.entity(e).insert((
                SceneRoot(model.clone()),
                Transform::from_scale(Vec3::splat(*scale)),
                SelectedBuild,
                Visibility::Hidden,
            )),
            BuildingType::Zone { color } => commands.entity(e).insert((
                Mesh3d(shapes.0[0].clone()),
                Wireframe,
                WireframeColor {
                    color: color.clone(),
                },
                Transform::default(),
                SelectedBuild,
                Resizable,
                Visibility::Hidden,
            )),
            BuildingType::Tool { op, color } => commands.entity(e).insert((
                ToolInstance {
                    op: *op,
                    radius: 5.0,
                    strength: 1.0,
                    color: color.clone(),
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
                SelectedBuild,
                Visibility::Hidden,
            )),
        };
    }
}

//const DEFAULT_RAY_DISTANCE: f32 = 10.;

fn compute_aabb(
    mut commands: Commands,
    children_query: Query<(&Children, &Transform)>,
    aabb_query: Query<(&Aabb, &Transform)>,
    selected_part_query: Option<Single<(Entity, &Children), (With<SelectedBuild>, Without<Aabb>)>>,
) {
    fn combine_aabb(x: &mut Aabb, y: &Aabb, offset: Vec3A) {
        *x = Aabb::from_min_max(
            x.min().min(y.min() + offset).into(),
            x.max().max(y.max() + offset).into(),
        )
    }
    if let Some(query) = selected_part_query {
        let (entity, children) = *query;
        let mut aabb = Aabb::from_min_max(Vec3::splat(1e10), Vec3::splat(-1e10));
        let mut stack: Vec<(Entity, Vec3)> = children.iter().map(|e| (e, Vec3::ZERO)).collect();
        while let Some((e, position)) = stack.pop() {
            if let Ok((child_aabb, child_transform)) = aabb_query.get(e) {
                let offset = child_transform.translation + position;
                combine_aabb(&mut aabb, child_aabb, offset.into());
            } else if let Ok((child_children, child_transform)) = children_query.get(e) {
                stack.extend(
                    child_children
                        .iter()
                        .map(|e| (e, position + child_transform.translation)),
                );
            }
        }
        commands.entity(entity).insert(aabb);
    }
}

/// Make the selected part follow the cursor
fn build_follow_cursor(
    mut ray_cast: MeshRayCast,
    camera_query: Single<(&Camera, &GlobalTransform)>,
    windows: Single<&Window>,
    selected_part_query: Option<
        Single<
            (
                Entity,
                &mut Transform,
                &Aabb,
                &mut Visibility,
                Option<&Resizable>,
            ),
            With<SelectedBuild>,
        >,
    >,
    map: Res<Map>,
    button: Res<ButtonInput<MouseButton>>,
    snapping: Res<Snapping>,
    mut place_point: Local<Vec2>,
    chunks: Query<&IsGround>,
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
    let (_e, mut part_transform, aabb, mut visibility, resizable) = selpart.into_inner();
    // Cast the ray to get hit to the nearest different object

    let filter = |entity: Entity| chunks.contains(entity);
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
        .mul_vec3(Vec3::from(aabb.half_extents) * part_transform.scale);
    let he_proj = part_transform
        .rotation
        .mul_vec3(Vec3::from(aabb.half_extents) * part_transform.scale)
        .project_onto(Vec3::Y);
    if resizable.is_some()
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
        let center = Vec3::from(aabb.center) * part_transform.scale;
        part_transform.translation =
            Vec3::new(place_point.x, map.get_height(point2d.xxy()), place_point.y) + he_proj
                - center;
    }
}

/// Actually place a part on click
fn place_build(
    mut commands: Commands,
    selected_part_query: Option<
        Single<(Entity, &Transform, Option<&ToolInstance>, &Aabb, &BuildId), With<SelectedBuild>>,
    >,
    mut map: ResMut<Map>,
    buildings: Res<Assets<Building>>,
    button: Res<ButtonInput<MouseButton>>,
    key: Res<ButtonInput<KeyCode>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if button.just_released(MouseButton::Left) {
        if let Some(query) = selected_part_query {
            let (e, transform, tool, aabb, bid) = *query;
            let (trsl, radius, op) = if let Some(ti) = tool {
                (transform.translation, ti.radius, ti.op)
            } else {
                (
                    transform.translation
                        + (Vec3::from(aabb.center) - Vec3::new(0., aabb.half_extents.y - 0.05, 0.))
                            * transform.scale,
                    (aabb.half_extents.xz() * transform.scale.xz()).norm() * 2.,
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
            if let Some(building) = buildings.get(&bid.0) {
                if let BuildingType::Single { .. } = building.typ {
                    let instance = BuildingInstance {
                        building: bid.0.clone(),
                        pos: transform.translation.xz() + aabb.min().xz() * transform.scale.xz(),
                        half_extents: aabb.half_extents.xz(),
                        entity: e,
                    };
                    map.entities.insert(instance.clone());
                    commands.entity(e).insert(instance);
                }
            }
        }
    }
}

fn select_world_part(
    mut commands: Commands,
    selected_part_query: Option<Single<Entity, With<SelectedBuild>>>,
    highlighted_part_query: Option<Single<Entity, With<Highlighted>>>,
    buildings: Query<&BuildingInstance>,
    parent_query: Query<&ChildOf>,
    mut ray_cast: MeshRayCast,
    camera_query: Single<(&Camera, &GlobalTransform)>,
    windows: Single<&Window>,
    keyboard_input: Res<ButtonInput<MouseButton>>,
    mut map: ResMut<Map>,
) {
    if selected_part_query.is_none() {
        let (camera, camera_transform) = *camera_query;

        let Some(cursor_position) = windows.cursor_position() else {
            return;
        };

        // Calculate a ray pointing from the camera into the world based on the cursor's position.
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
            return;
        };

        let settings = MeshRayCastSettings::default().always_early_exit();
        let hits = ray_cast.cast_ray(ray, &settings);

        if let Some((e, _hit)) = hits.first() {
            let mut e = *e;
            //go up the entity hierarchy to get toplevel entity
            while let Ok(ChildOf(parent)) = parent_query.get(e) {
                e = *parent;
            }
            //checks if hit is a building
            if let Ok(instance) = buildings.get(e) {
                //if clicked, select it
                if keyboard_input.just_released(MouseButton::Left) {
                    highlighted_part_query.map(|e| {
                        commands.entity(*e).remove::<Highlighted>();
                    });
                    commands
                        .entity(e)
                        .insert(SelectedBuild)
                        .remove::<BuildingInstance>();
                    map.entities.remove_one(instance.clone());
                } else {
                    //highlight it and remove potential different highlights.
                    if let Some(highlighted_e) = highlighted_part_query {
                        if e != *highlighted_e {
                            commands.entity(*highlighted_e).remove::<Highlighted>();
                            commands.entity(e).insert(Highlighted);
                        }
                    } else {
                        commands.entity(e).insert(Highlighted);
                    }
                }
            } else {
                highlighted_part_query.map(|e| {
                    commands.entity(*e).remove::<Highlighted>();
                });
            }
        }
    }
}

#[derive(Component)]
pub struct HighlightLight;

fn on_add_highlight(
    trigger: Trigger<OnAdd, Highlighted>,
    part_query: Query<(&Transform, &Aabb), With<BuildId>>,
    mut light_query: Single<(&mut Transform, &mut SpotLight), (With<HighlightLight>, Without<BuildId>)>,
) {
    if let Ok((part, aabb)) = part_query.get(trigger.target()) {
        let (light_transform, light) = &mut *light_query;
        let pos = part.translation + Vec3::from(aabb.center) * part.scale;
        const LIGHT_DISTANCE: f32 = 10.;
        light_transform.translation = pos + Vec3::Y * LIGHT_DISTANCE;
        light_transform.look_at(pos, Vec3::Y);
        light.outer_angle = ((Vec3::from(aabb.half_extents) * part.scale).norm()  / LIGHT_DISTANCE).atan();
    }
}

fn on_remove_highlight(
    _trigger: Trigger<OnRemove, Highlighted>,
    mut light_query: Single<&mut Transform, With<HighlightLight>>,
) {
    light_query.translation = Vec3::new(0., -10., 0.);
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
