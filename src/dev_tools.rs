//! Debug tooling: remote inspector, wireframe and automatic screenshots (clicking the ground
//! and issue reports: see `planet::inspect`).
//!
//! - F3: global wireframe
//!
//! Setting `UF_SCREENSHOT=<path.png>` saves a screenshot `UF_SCREENSHOT_DELAY` seconds (default
//! 20) after the world is ready, and exits. Agents use this to check visuals without a human (the planet's
//! view: `UF_PLANET_CAMERA`).
//!
//! `UF_SCREENSHOTS=<view>=<path.png>;<view>=<path.png>;...` takes several in one run (a single
//! world generation): the camera moves to each view in turn (as `UF_PLANET_CAMERA` takes it),
//! waits `UF_SCREENSHOT_DELAY` seconds (default 10: chunks and vegetation load) and saves the
//! shot; the app exits after the last.

use bevy::{
    input::common_conditions::input_just_pressed,
    pbr::wireframe::{WireframeConfig, WireframePlugin},
    prelude::*,
    remote::{RemotePlugin, http::RemoteHttpPlugin},
    render::view::screenshot::{Screenshot, save_to_disk},
};

use crate::{
    AppState,
    planet::camera::{PlanetCamera, View},
};

pub struct DevToolsPlugin;

impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            WireframePlugin::default(),
            RemotePlugin::default(),
            RemoteHttpPlugin::default(),
        ))
        .add_systems(
            Update,
            toggle_wireframe.run_if(input_just_pressed(KeyCode::F3)),
        );
        let mut shots: Vec<(Option<String>, String)> = Vec::new();
        if let Ok(path) = std::env::var("UF_SCREENSHOT") {
            shots.push((None, path));
        }
        if let Ok(list) = std::env::var("UF_SCREENSHOTS") {
            for entry in list.split(';').filter(|entry| !entry.trim().is_empty()) {
                match entry.split_once('=') {
                    Some((view, path)) => {
                        shots.push((Some(view.trim().to_string()), path.trim().to_string()))
                    }
                    None => warn!("UF_SCREENSHOTS entry without a view: {entry}"),
                }
            }
        }
        if !shots.is_empty() {
            let delay = std::env::var("UF_SCREENSHOT_DELAY")
                .ok()
                .and_then(|d| d.parse().ok())
                .unwrap_or(if shots.len() > 1 { 10.0 } else { 20.0 });
            app.insert_resource(AutoScreenshot { shots, delay })
                .add_systems(Update, auto_screenshot);
        }
    }
}

fn toggle_wireframe(mut wireframe_config: ResMut<WireframeConfig>) {
    wireframe_config.global = !wireframe_config.global;
}

/// Screenshot requests from `UF_SCREENSHOT` and `UF_SCREENSHOTS`: (view, path), in order.
#[derive(Resource)]
struct AutoScreenshot {
    shots: Vec<(Option<String>, String)>,
    delay: f32,
}

/// Seconds over which the frame rate is measured before an automatic screenshot.
const FPS_WINDOW: f32 = 3.;

/// Take the requested screenshots in turn: move the camera to the shot's view, wait the delay,
/// save it; exit a second after the last.
fn auto_screenshot(
    mut commands: Commands,
    request: Res<AutoScreenshot>,
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut camera: Query<&mut PlanetCamera>,
    mut started_at: Local<Option<f32>>,
    mut next: Local<usize>,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    if *state.get() != AppState::InGame {
        return;
    }
    let now = time.elapsed_secs();
    let Some(started) = *started_at else {
        // Start the next shot: the camera to its view.
        let Ok(mut camera) = camera.single_mut() else {
            return;
        };
        if let Some(view) = &request.shots[*next].0 {
            let current = View {
                focus: camera.focus,
                distance: camera.distance,
                pitch: Some(camera.pitch),
                heading: None,
            };
            View::parse(view, current).apply(&mut camera);
        }
        *started_at = Some(now);
        *frames = 0;
        return;
    };
    let elapsed = now - started;
    if *next >= request.shots.len() {
        // (A second for the last one to be written.)
        if elapsed >= 1.0 {
            exit.write(AppExit::Success);
        }
        return;
    }
    // Frame rate over the last seconds before the screenshot.
    if elapsed >= request.delay - FPS_WINDOW {
        *frames += 1;
    }
    if elapsed >= request.delay {
        let path = &request.shots[*next].1;
        info!(
            "Saving screenshot to {path} ({:.0} fps over the last {FPS_WINDOW} s)",
            *frames as f32 / FPS_WINDOW
        );
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()));
        *next += 1;
        *started_at = (*next >= request.shots.len()).then_some(now);
    }
}
