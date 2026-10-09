//! Building placement and selection.
//!
//! - [`StartPlacement`] (triggered by the build menu) spawns the building under the cursor.
//! - While placing, the building follows the cursor (snapped to the grid, S cycles snapping).
//!   A left press + release places it; zones are resized by dragging. Ctrl keeps placing.
//! - Hovering a placed building highlights it, clicking it picks it up again (Bevy picking).

use bevy::{
    asset::LoadedFolder,
    camera::primitives::{Aabb, MeshAabb},
    color::palettes::css::{ORANGE_RED, RED},
    dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings},
    input::common_conditions::input_just_pressed,
    math::{Affine3A, NormedVectorSpace},
    pbr::{
        decal::{ForwardDecal, ForwardDecalMaterial, ForwardDecalMaterialExt},
        wireframe::{Wireframe, WireframeColor},
    },
    picking::pointer::PointerButton,
    prelude::*,
    world_serialization::WorldInstanceReady,
};

use crate::{
    map::{BuildingInstance, Chunk, GRID_SQUARE_SIZE, IsGround, Map, PatchOp},
    sim::RhaiScript,
    ui::PointerOverUi,
};

pub struct BuildPlugin;

impl Plugin for BuildPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InfiniteGridPlugin)
            .init_resource::<Snapping>()
            .init_resource::<PlacementPress>()
            .init_resource::<PlacementCursor>()
            .add_systems(Startup, (load_buildings, spawn_highlight_light, spawn_build_grid))
            .add_systems(
                Update,
                (
                    cycle_snapping.run_if(input_just_pressed(KeyCode::KeyS)),
                    (build_follow_cursor, place_build).chain(),
                    update_build_grid,
                ),
            )
            .add_observer(start_placement)
            .add_observer(highlight_on_enter)
            .add_observer(unhighlight_on_leave)
            .add_observer(pick_up_on_click)
            .add_observer(on_add_highlight)
            .add_observer(on_remove_highlight);
    }
}

/// A building (to be modified with everything needed)
#[derive(Asset, TypePath, Debug)]
pub struct Building {
    pub typ: BuildingType,
    pub name: String,
    pub size: (u64, u64),
    pub script: Option<Handle<RhaiScript>>,
}

/// Split between zoning and individual buildings (and maybe more things in the future, e.g. roads)
#[derive(Debug)]
pub enum BuildingType {
    Zone { color: Color },
    Single { model: Handle<WorldAsset>, scale: f32 },
    Tool { op: PatchOp, color: Color },
}

/// Start placing a building: spawns it under the cursor, replacing any building being placed.
#[derive(Event)]
pub struct StartPlacement(pub Handle<Building>);

/// Which building an entity is an instance of.
#[derive(Clone, Component, PartialEq, Default)]
pub struct BuildId(pub Handle<Building>);

/// The building currently being placed, following the cursor.
#[derive(Component)]
pub struct SelectedBuild;

/// Whether a building can be resized by dragging (zones).
#[derive(Component)]
pub struct Resizable;

/// A placed building under the cursor.
#[derive(Component)]
pub struct Highlighted;

/// Terraforming tool being placed.
#[derive(Component)]
struct ToolInstance {
    op: PatchOp,
    radius: f32,
}

/// Handle to the `buildings` asset folder, keeping all building definitions loaded.
#[derive(Resource)]
pub struct Buildings(pub Handle<LoadedFolder>);

/// Mesh used to display zones.
#[derive(Resource)]
struct ZoneMesh(Handle<Mesh>);

/// Multiples of grid square the placement snaps to.
#[derive(Resource, Default, Clone, Copy)]
pub enum Snapping {
    None,
    #[default]
    One,
    Two,
    Four,
}

impl Snapping {
    /// Distance between snapping points, in world units.
    fn spacing(self) -> Option<f32> {
        match self {
            Snapping::None => None,
            Snapping::One => Some(GRID_SQUARE_SIZE),
            Snapping::Two => Some(2. * GRID_SQUARE_SIZE),
            Snapping::Four => Some(4. * GRID_SQUARE_SIZE),
        }
    }
}

/// Ground point where the left button was pressed during the current placement.
///
/// A building is only placed on release if the press also happened while placing (and not
/// over UI), so the click that started the placement doesn't place it immediately.
#[derive(Resource, Default)]
struct PlacementPress(Option<Vec2>);

/// Ground point under the cursor while placing.
#[derive(Resource, Default)]
struct PlacementCursor(Option<Vec3>);

/// Spot light moved onto the highlighted building.
#[derive(Component)]
struct HighlightLight;

/// Grid shown while placing, matching the snapping.
#[derive(Component)]
struct BuildGrid;

/// Height of the build grid above the cursor. The grid is a flat plane, so it disappears
/// where the terrain rises above it.
const BUILD_GRID_HEIGHT: f32 = 0.5;
/// Where the highlight light hides when nothing is highlighted.
const HIGHLIGHT_LIGHT_REST: Vec3 = Vec3::new(0., -10., 0.);
/// Height of the highlight light above the highlighted building.
const HIGHLIGHT_LIGHT_DISTANCE: f32 = 10.;

fn load_buildings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    asset_server: Res<AssetServer>,
) {
    commands.insert_resource(ZoneMesh(meshes.add(Cuboid::default())));
    commands.insert_resource(Buildings(asset_server.load_folder("buildings")));
}

fn spawn_highlight_light(mut commands: Commands) {
    commands.spawn((
        Name::new("Highlight light"),
        HighlightLight,
        SpotLight {
            color: ORANGE_RED.into(),
            intensity: 1e9,
            range: 100.,
            outer_angle: 0.1,
            inner_angle: 0.02,
            ..default()
        },
        Transform::from_translation(HIGHLIGHT_LIGHT_REST),
    ));
}

fn spawn_build_grid(mut commands: Commands) {
    let line_color = Color::srgba(1., 1., 1., 0.15);
    let major_line_color = Color::srgba(1., 1., 1., 0.35);
    commands.spawn((
        Name::new("Build grid"),
        BuildGrid,
        InfiniteGrid,
        InfiniteGridSettings {
            x_axis_color: major_line_color,
            z_axis_color: major_line_color,
            minor_line_color: line_color,
            major_line_color,
            fadeout_distance: 40.,
            major_line_interval: 4,
            ..default()
        },
        Visibility::Hidden,
    ));
}

/// Spawn the building to place, replacing the one currently being placed.
fn start_placement(
    start: On<StartPlacement>,
    mut commands: Commands,
    placing: Query<Entity, With<SelectedBuild>>,
    buildings: Res<Assets<Building>>,
    zone_mesh: Res<ZoneMesh>,
    asset_server: Res<AssetServer>,
    mut decal_materials: ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>,
) -> Result {
    let building = buildings
        .get(&start.0)
        .ok_or("tried to place a building that is not loaded")?;
    for entity in &placing {
        commands.entity(entity).despawn();
    }

    let mut entity = commands.spawn((
        Name::new(building.name.clone()),
        BuildId(start.0.clone()),
        SelectedBuild,
        Visibility::Hidden,
    ));
    match &building.typ {
        BuildingType::Single { model, scale } => {
            entity
                .insert((
                    WorldAssetRoot(model.clone()),
                    Transform::from_scale(Vec3::splat(*scale)),
                ))
                .observe(compute_scene_aabb);
        }
        BuildingType::Zone { color } => {
            entity.insert((
                Mesh3d(zone_mesh.0.clone()),
                Wireframe,
                WireframeColor { color: *color },
                Transform::default(),
                Resizable,
            ));
        }
        BuildingType::Tool { op, .. } => {
            entity.insert((
                ToolInstance {
                    op: *op,
                    radius: 5.0,
                },
                ForwardDecal,
                MeshMaterial3d(decal_materials.add(ForwardDecalMaterial {
                    base: StandardMaterial {
                        base_color_texture: Some(asset_server.load("img/circle.png")),
                        alpha_mode: AlphaMode::Blend,
                        base_color: RED.into(),
                        ..default()
                    },
                    extension: ForwardDecalMaterialExt {
                        depth_fade_factor: 1.0,
                    },
                })),
                Transform::from_scale(Vec3::splat(10.0)),
            ));
        }
    }
    Ok(())
}

/// Once a building's glTF scene has spawned, give its root an [`Aabb`] covering all its meshes.
fn compute_scene_aabb(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    nodes: Query<(&Transform, Option<&Mesh3d>)>,
    meshes: Res<Assets<Mesh>>,
) {
    let root = ready.entity;
    let (mut min, mut max) = (Vec3::MAX, Vec3::MIN);
    let mut stack: Vec<(Entity, Affine3A)> = children
        .get(root)
        .map(|c| c.iter().map(|e| (e, Affine3A::IDENTITY)).collect())
        .unwrap_or_default();
    while let Some((entity, parent_affine)) = stack.pop() {
        let Ok((transform, mesh)) = nodes.get(entity) else {
            continue;
        };
        let affine = parent_affine * transform.compute_affine();
        if let Some(aabb) = mesh
            .and_then(|mesh| meshes.get(&mesh.0))
            .and_then(|mesh| mesh.get_aabb())
        {
            let (aabb_min, aabb_max) = (Vec3::from(aabb.min()), Vec3::from(aabb.max()));
            for i in 0..8 {
                let corner = Vec3::select(
                    BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0),
                    aabb_max,
                    aabb_min,
                );
                let corner = affine.transform_point3(corner);
                min = min.min(corner);
                max = max.max(corner);
            }
        }
        if let Ok(grandchildren) = children.get(entity) {
            stack.extend(grandchildren.iter().map(|e| (e, affine)));
        }
    }
    if min.cmple(max).all() {
        commands.entity(root).insert(Aabb::from_min_max(min, max));
    }
}

/// Make the building being placed follow the cursor, or resize it while dragging a zone.
fn build_follow_cursor(
    mut ray_cast: MeshRayCast,
    camera_query: Single<(&Camera, &GlobalTransform)>,
    windows: Single<&Window>,
    placing: Option<
        Single<(&mut Transform, &Aabb, &mut Visibility, Has<Resizable>), With<SelectedBuild>>,
    >,
    map: Res<Map>,
    button: Res<ButtonInput<MouseButton>>,
    snapping: Res<Snapping>,
    mut press: ResMut<PlacementPress>,
    mut cursor: ResMut<PlacementCursor>,
    over_ui: PointerOverUi,
    chunks: Query<(), With<IsGround>>,
) {
    let Some(placing) = placing else {
        press.0 = None;
        cursor.0 = None;
        return;
    };
    let (mut transform, aabb, mut visibility, resizable) = placing.into_inner();
    let (camera, camera_transform) = *camera_query;

    let Some(ray) = windows
        .cursor_position()
        .and_then(|cursor| camera.viewport_to_world(camera_transform, cursor).ok())
    else {
        return;
    };

    // Only the terrain counts as a placement surface.
    let filter = |entity: Entity| chunks.contains(entity);
    let settings = MeshRayCastSettings::default()
        .always_early_exit()
        .with_filter(&filter);
    let Some((_, hit)) = ray_cast.cast_ray(ray, &settings).first() else {
        *visibility = Visibility::Hidden;
        cursor.0 = None;
        return;
    };
    *visibility = Visibility::Visible;
    cursor.0 = Some(hit.point);

    let point2d = hit.point.xz();
    let point2d = match snapping.spacing() {
        Some(spacing) => (point2d / spacing).round() * spacing,
        None => point2d,
    };

    if button.just_pressed(MouseButton::Left) && !over_ui.get() {
        press.0 = Some(point2d);
    }

    let half_extents = transform
        .rotation
        .mul_vec3(Vec3::from(aabb.half_extents) * transform.scale);
    match press.0 {
        Some(anchor) if resizable => {
            let scale = point2d - anchor;
            let scale = scale.abs().max(Vec2::splat(0.1)) * scale.signum();
            transform.scale = Vec3::new(scale.x, 1., scale.y);
            transform.translation =
                Vec3::new(anchor.x, 0., anchor.y) + half_extents * transform.scale;
        }
        _ => {
            let center = Vec3::from(aabb.center) * transform.scale;
            transform.translation = Vec3::new(point2d.x, map.get_height(point2d.xxy()), point2d.y)
                + half_extents.project_onto(Vec3::Y)
                - center;
        }
    }
}

/// Place the building on release: flatten the terrain under it (or apply the tool), register it
/// in the map, and with Ctrl held start placing another one.
fn place_build(
    mut commands: Commands,
    placing: Option<
        Single<(Entity, &Transform, Option<&ToolInstance>, &Aabb, &BuildId), With<SelectedBuild>>,
    >,
    mut map: ResMut<Map>,
    buildings: Res<Assets<Building>>,
    button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut press: ResMut<PlacementPress>,
) {
    if !button.just_released(MouseButton::Left) || press.0.take().is_none() {
        return;
    }
    let Some(placing) = placing else {
        return;
    };
    let (entity, transform, tool, aabb, build_id) = *placing;

    let (center, radius, op) = if let Some(tool) = tool {
        (transform.translation, tool.radius, tool.op)
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
    let add_patches = chunk.patch(&mut meshes, &center, radius, op);
    for (off_x, off_z) in add_patches {
        let chunk = map.get_chunk_mut(&(chunk_pos_x + off_x, chunk_pos_z + off_z).into());
        chunk.patch(&mut meshes, &center, radius, op);
    }

    commands.entity(entity).remove::<SelectedBuild>();
    if tool.is_some() {
        // Tools only act once.
        commands.entity(entity).despawn();
    }
    if let Some(building) = buildings.get(&build_id.0)
        && let BuildingType::Single { .. } = building.typ
    {
        let instance = BuildingInstance {
            building: build_id.0.clone(),
            pos: transform.translation.xz() + aabb.min().xz() * transform.scale.xz(),
            half_extents: aabb.half_extents.xz(),
            entity,
        };
        map.entities.insert(instance.clone());
        commands.entity(entity).insert(instance);
    }
    if keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        commands.trigger(StartPlacement(build_id.0.clone()));
    }
}

/// Show the grid while placing, at the cursor height and matching the snapping.
fn update_build_grid(
    grid: Single<
        (&mut Transform, &mut Visibility, &mut InfiniteGridSettings),
        With<BuildGrid>,
    >,
    cursor: Res<PlacementCursor>,
    snapping: Res<Snapping>,
) {
    let (mut transform, mut visibility, mut settings) = grid.into_inner();
    match (cursor.0, snapping.spacing()) {
        (Some(point), Some(spacing)) => {
            visibility.set_if_neq(Visibility::Visible);
            // Keep x/z at the origin so grid lines line up with the snapping points.
            transform.translation = Vec3::new(0., point.y + BUILD_GRID_HEIGHT, 0.);
            settings.scale = 1. / spacing;
        }
        _ => {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

/// Highlight placed buildings under the cursor, unless something is being placed.
fn highlight_on_enter(
    enter: On<PointerEnter>,
    mut commands: Commands,
    buildings: Query<(), With<BuildingInstance>>,
    highlighted: Query<Entity, With<Highlighted>>,
    placing: Query<(), With<SelectedBuild>>,
) {
    let target = enter.event_target();
    if !placing.is_empty() || !buildings.contains(target) {
        return;
    }
    for entity in &highlighted {
        commands.entity(entity).remove::<Highlighted>();
    }
    commands.entity(target).insert(Highlighted);
}

fn unhighlight_on_leave(
    leave: On<PointerLeave>,
    mut commands: Commands,
    highlighted: Query<(), With<Highlighted>>,
) {
    let target = leave.event_target();
    if highlighted.contains(target) {
        commands.entity(target).remove::<Highlighted>();
    }
}

/// Clicking a placed building picks it up for placing again.
fn pick_up_on_click(
    click: On<PointerClick>,
    mut commands: Commands,
    buildings: Query<&BuildingInstance>,
    placing: Query<(), With<SelectedBuild>>,
    mut map: ResMut<Map>,
) {
    let target = click.event_target();
    if click.button != PointerButton::Primary || !placing.is_empty() {
        return;
    }
    let Ok(instance) = buildings.get(target) else {
        return;
    };
    map.entities.remove_one(instance.clone());
    commands
        .entity(target)
        .remove::<(Highlighted, BuildingInstance)>()
        .insert(SelectedBuild);
}

fn on_add_highlight(
    add: On<Add<Highlighted>>,
    buildings: Query<(&Transform, &Aabb), With<BuildId>>,
    light: Single<(&mut Transform, &mut SpotLight), (With<HighlightLight>, Without<BuildId>)>,
) {
    let Ok((building, aabb)) = buildings.get(add.entity) else {
        return;
    };
    let (mut light_transform, mut light) = light.into_inner();
    let pos = building.translation + Vec3::from(aabb.center) * building.scale;
    light_transform.translation = pos + Vec3::Y * HIGHLIGHT_LIGHT_DISTANCE;
    light_transform.look_at(pos, Vec3::Y);
    light.outer_angle = ((Vec3::from(aabb.half_extents) * building.scale).norm()
        / HIGHLIGHT_LIGHT_DISTANCE)
        .atan();
}

fn on_remove_highlight(
    _remove: On<Remove<Highlighted>>,
    mut light: Single<&mut Transform, With<HighlightLight>>,
) {
    light.translation = HIGHLIGHT_LIGHT_REST;
}

/// Cycle the snapping mode.
fn cycle_snapping(mut snapping: ResMut<Snapping>) {
    *snapping = match *snapping {
        Snapping::None => Snapping::One,
        Snapping::One => Snapping::Two,
        Snapping::Two => Snapping::Four,
        Snapping::Four => Snapping::None,
    }
}
