//! Debug tooling: remote inspector, wireframe and debug gizmos.
//!
//! - F2: building bounding boxes
//! - F3: global wireframe
//! - F4: river center lines
//! - Clicking the terrain logs its generation data (height, gradient, hydrology).
//!
//! Setting `UF_SCREENSHOT=<path.png>` saves a screenshot after `UF_SCREENSHOT_DELAY` seconds
//! (default 20) and exits. Agents use this to check visuals without a human.
//!
//! Lab mode, `UF_LAB=<dir>`: the game keeps running (shaders hot-reload) and takes screenshots
//! on request. Each line of `<dir>/shots.txt` is `<file.png> x z [dx dy dz]` (camera target and
//! offset, as `UF_CAMERA`); the file is removed once read, and `<dir>/done.txt` is written once
//! the screenshots are saved.

use bevy::{
    camera::primitives::Aabb,
    color::palettes::css::{ORANGE_RED, RED},
    input::common_conditions::input_just_pressed,
    pbr::wireframe::{WireframeConfig, WireframePlugin},
    prelude::*,
    remote::{RemotePlugin, http::RemoteHttpPlugin},
    render::view::screenshot::{Screenshot, save_to_disk},
};

use bevy::camera_controller::pan_orbit_camera::prelude::component::PanOrbitCamera;

use crate::{
    AppState,
    build::BuildId,
    map::{ChunkMeshTask, GroundMesh, Map},
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
                draw_rivers
                    .run_if(gizmos_enabled::<RiverGizmos>)
                    .run_if(in_state(AppState::InGame)),
            ),
        );
        if let Ok(dir) = std::env::var("UF_LAB") {
            app.insert_resource(Lab {
                dir: dir.into(),
                ..default()
            })
            .add_systems(Update, lab_shots.run_if(in_state(AppState::InGame)));
        }
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
    config_store.config_mut::<RiverGizmos>().0.enabled = false;
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

fn draw_rivers(map: Res<Map>, mut gizmos: Gizmos<RiverGizmos>) {
    for river in &map.continent.rivers {
        gizmos.linestrip(
            river
                .points
                .iter()
                .map(|point| point.position + Vec3::Y * 0.1),
            RED,
        );
    }
}

/// Log the generation data of the terrain point under a click.
fn log_terrain_on_click(
    click: On<PointerClick>,
    chunks: Query<(), With<GroundMesh>>,
    map: Option<Res<Map>>,
) {
    let (true, Some(map), Some(pos)) = (
        chunks.contains(click.event_target()),
        map,
        click.hit.position,
    ) else {
        return;
    };
    let sample = map.sample(pos);
    let continent = &map.continent;
    let Some((x, z)) = continent.world_to_cell(pos.xz()) else {
        info!(
            "terrain {pos}: height {} (off the continent)",
            sample.height
        );
        return;
    };
    let i = z * continent.size() + x;
    info!(
        "terrain ({x}, {z}): height {} wetness {} - base {} drainage {} lake {:?} - {:?} {:?} \
         moisture {:.2} temperature {:.2}",
        sample.height,
        sample.wetness,
        continent.heights[i],
        continent.drainage[i],
        continent.lake_of[i],
        continent.landform[i],
        continent.biome[i],
        continent.moisture[i],
        continent.temperature[i]
    );
}

/// Screenshot request from the `UF_SCREENSHOT` environment variable.
#[derive(Resource)]
struct AutoScreenshot {
    path: String,
    delay: f32,
}

/// Seconds over which the frame rate is measured before an automatic screenshot.
const FPS_WINDOW: f32 = 3.;

/// Take the requested screenshot once the delay has passed, then exit a second later.
fn auto_screenshot(
    mut commands: Commands,
    request: Res<AutoScreenshot>,
    time: Res<Time>,
    mut taken: Local<bool>,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = time.elapsed_secs();
    // Frame rate over the last seconds before the screenshot.
    if elapsed >= request.delay - FPS_WINDOW && !*taken {
        *frames += 1;
    }
    if !*taken && elapsed >= request.delay {
        *taken = true;
        info!(
            "Saving screenshot to {} ({:.0} fps over the last {FPS_WINDOW} s)",
            request.path,
            *frames as f32 / FPS_WINDOW
        );
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(request.path.clone()));
    } else if *taken && elapsed >= request.delay + 1.0 {
        exit.write(AppExit::Success);
    }
}

/// Lab mode state (`UF_LAB`).
#[derive(Resource, Default)]
struct Lab {
    dir: std::path::PathBuf,
    queue: std::collections::VecDeque<(String, Vec3, Vec3)>,
    /// Frames left before the current shot (letting chunks remesh and TAA settle).
    wait: u32,
    current: Option<String>,
    since_poll: f32,
    /// Consecutive frames without chunk meshing.
    idle: u32,
}

/// Frames rendered at a new camera position before its screenshot (at least, and then until
/// the terrain chunks are meshed, up to `LAB_MAX_FRAMES`).
const LAB_SETTLE_FRAMES: u32 = 60;
const LAB_MAX_FRAMES: u32 = 900;
/// Frames without any chunk meshing before a lab screenshot.
const LAB_IDLE_FRAMES: u32 = 30;
/// The lab camera stays at least this high above the ground under it (an offset close to the
/// ground could put it inside a slope).
const LAB_CLEARANCE: f32 = 0.8;

fn lab_shots(
    mut commands: Commands,
    mut lab: ResMut<Lab>,
    mut camera: Single<(&mut Transform, &mut PanOrbitCamera)>,
    map: Res<Map>,
    time: Res<Time>,
    meshing: Query<(), With<ChunkMeshTask>>,
) {
    lab.since_poll += time.delta_secs();
    if lab.current.is_none() && lab.queue.is_empty() && lab.since_poll > 0.25 {
        lab.since_poll = 0.;
        let request = lab.dir.join("shots.txt");
        if let Ok(text) = std::fs::read_to_string(&request) {
            let _ = std::fs::remove_file(&request);
            let _ = std::fs::remove_file(lab.dir.join("done.txt"));
            for line in text.lines() {
                let mut words = line.split_whitespace();
                let Some(file) = words.next() else {
                    continue;
                };
                let values: Vec<f32> = words.filter_map(|w| w.parse().ok()).collect();
                let target = Vec3::new(values[0], 0., values[1]);
                let target = target.with_y(map.get_height(target));
                let offset = match values[..] {
                    [_, _, dx, dy, dz] => Vec3::new(dx, dy, dz),
                    _ => Vec3::new(0., 30., 40.),
                };
                lab.queue.push_back((file.to_owned(), target, offset));
            }
        }
    }
    if lab.current.is_none() {
        if let Some((file, target, offset)) = lab.queue.pop_front() {
            let mut eye = target + offset;
            eye.y = eye.y.max(map.get_height(eye) + LAB_CLEARANCE);
            let (transform, controller) = &mut *camera;
            **transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
            // The controller places the near clip plane from its anchor depth: keep it in step
            // (a far anchor would clip everything close to the camera).
            controller.last_anchor_depth = -f64::from(eye.distance(target));
            lab.current = Some(file);
            lab.wait = LAB_MAX_FRAMES;
        }
        return;
    }
    // Settled once no chunk has been meshing for a while (a finished mesh built for the old
    // camera position is only rescheduled on the next frame).
    lab.idle = if meshing.is_empty() { lab.idle + 1 } else { 0 };
    let settled = LAB_MAX_FRAMES - lab.wait >= LAB_SETTLE_FRAMES && lab.idle >= LAB_IDLE_FRAMES;
    if lab.wait > 0 && !settled {
        lab.wait -= 1;
        return;
    }
    if !settled {
        warn!("Lab screenshot taken before the terrain was done meshing ({LAB_MAX_FRAMES} frames)");
    }
    let file = lab.current.take().unwrap();
    let path = lab.dir.join(&file);
    info!("Lab screenshot {} ({:.0} fps)", path.display(), 1. / time.delta_secs());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    if lab.queue.is_empty() {
        let _ = std::fs::write(lab.dir.join("done.txt"), &file);
    }
}
