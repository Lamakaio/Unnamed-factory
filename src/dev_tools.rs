//! Debug tooling: remote inspector, wireframe and debug gizmos.
//!
//! - F2: building bounding boxes
//! - F3: global wireframe
//! - F4: river paths and lakes (on by default)
//! - Clicking the terrain logs its generation data (height, gradient, hydrology).
//!
//! Setting `UF_SCREENSHOT=<path.png>` saves a screenshot after `UF_SCREENSHOT_DELAY` seconds
//! (default 20) and exits. Agents use this to check visuals without a human.

use bevy::{
    camera::primitives::Aabb,
    color::palettes::css::{ORANGE_RED, PURPLE, RED},
    input::common_conditions::input_just_pressed,
    pbr::wireframe::{WireframeConfig, WireframePlugin},
    prelude::*,
    remote::{RemotePlugin, http::RemoteHttpPlugin},
    render::view::screenshot::{Screenshot, save_to_disk},
};

use crate::{
    build::BuildId,
    map::{IsGround, Map},
};

pub struct DevToolsPlugin;

impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            WireframePlugin::default(),
            RemotePlugin::default(),
            RemoteHttpPlugin::default(),
        ))
        .init_gizmo_group::<AabbGizmos>()
        .init_gizmo_group::<RiverGizmos>()
        .add_systems(Startup, configure_gizmos)
        .add_observer(log_terrain_on_click)
        .add_systems(
            Update,
            (
                toggle_gizmos::<AabbGizmos>.run_if(input_just_pressed(KeyCode::F2)),
                toggle_wireframe.run_if(input_just_pressed(KeyCode::F3)),
                toggle_gizmos::<RiverGizmos>.run_if(input_just_pressed(KeyCode::F4)),
                draw_building_aabbs.run_if(gizmos_enabled::<AabbGizmos>),
                draw_rivers.run_if(gizmos_enabled::<RiverGizmos>),
            ),
        );
        if let Ok(path) = std::env::var("UF_SCREENSHOT") {
            let delay = std::env::var("UF_SCREENSHOT_DELAY")
                .ok()
                .and_then(|d| d.parse().ok())
                .unwrap_or(20.0);
            app.insert_resource(AutoScreenshot { path, delay })
                .add_systems(Update, auto_screenshot);
        }
    }
}

/// Bounding boxes of buildings.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct AabbGizmos;

/// River paths and lakes computed by map generation.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct RiverGizmos;

fn configure_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
    config_store.config_mut::<AabbGizmos>().0.enabled = false;
}

/// Run condition: skip building gizmos for a disabled group.
fn gizmos_enabled<T: GizmoConfigGroup>(config_store: Res<GizmoConfigStore>) -> bool {
    config_store.config::<T>().0.enabled
}

fn toggle_gizmos<T: GizmoConfigGroup>(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<T>();
    config.enabled = !config.enabled;
}

fn toggle_wireframe(mut wireframe_config: ResMut<WireframeConfig>) {
    wireframe_config.global = !wireframe_config.global;
}

fn draw_building_aabbs(
    aabbs: Query<(&Aabb, &GlobalTransform), With<BuildId>>,
    mut gizmos: Gizmos<AabbGizmos>,
) {
    for (aabb, transform) in &aabbs {
        gizmos.cube(
            Transform::from_translation(
                Vec3::from(aabb.center) * transform.scale() + transform.translation(),
            )
            .with_scale(
                transform
                    .rotation()
                    .mul_vec3(Vec3::from(aabb.half_extents) * transform.scale() * 2.),
            ),
            ORANGE_RED,
        );
    }
}

fn draw_rivers(map: Res<Map>, mut gizmos: Gizmos<RiverGizmos>) -> Result {
    for river in &map.continent.river_paths {
        let curve = river.position.to_curve()?;
        let len = curve.segments().len();
        gizmos.curve_3d(
            curve,
            (0..=200).map(|i| i as f32 / 200. * len as f32),
            RED,
        );
    }

    for lake in &map.continent.lakes {
        let pos = map.continent.to_world(*lake);
        gizmos.sphere(Isometry3d::from_translation(pos), 10., PURPLE);
    }
    Ok(())
}

/// Log the generation data of the terrain point under a click.
fn log_terrain_on_click(click: On<PointerClick>, chunks: Query<&IsGround>, map: Res<Map>) {
    let Ok(IsGround(chunk_position)) = chunks.get(click.event_target()) else {
        return;
    };
    let Some(index) = click
        .hit
        .position
        .and_then(|pos| map.continent_index(*chunk_position, pos))
    else {
        return;
    };
    let point = &map.continent[index];
    let hydro = map.continent.get_hydro(index.0, index.1);
    info!(
        "terrain {index:?}: height {} grad {} - water {} momentum {} - visit {}",
        point.height, point.grad, hydro.amount, hydro.momentum, hydro.visit
    );
}

/// Screenshot request from the `UF_SCREENSHOT` environment variable.
#[derive(Resource)]
struct AutoScreenshot {
    path: String,
    delay: f32,
}

/// Take the requested screenshot once the delay has passed, then exit a second later.
fn auto_screenshot(
    mut commands: Commands,
    request: Res<AutoScreenshot>,
    time: Res<Time>,
    mut taken: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = time.elapsed_secs();
    if !*taken && elapsed >= request.delay {
        *taken = true;
        info!("Saving screenshot to {}", request.path);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(request.path.clone()));
    } else if *taken && elapsed >= request.delay + 1.0 {
        exit.write(AppExit::Success);
    }
}
