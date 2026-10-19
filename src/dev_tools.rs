//! Debug tooling: remote inspector, wireframe and automatic screenshots (clicking the ground
//! and issue reports: see `planet::inspect`).
//!
//! - F3: global wireframe
//!
//! Setting `UF_SCREENSHOT=<path.png>` saves a screenshot `UF_SCREENSHOT_DELAY` seconds (default
//! 20) after the world is ready, and exits. Agents use this to check visuals without a human (the planet's
//! view: `UF_PLANET_CAMERA`).

use bevy::{
    input::common_conditions::input_just_pressed,
    pbr::wireframe::{WireframeConfig, WireframePlugin},
    prelude::*,
    remote::{RemotePlugin, http::RemoteHttpPlugin},
    render::view::screenshot::{Screenshot, save_to_disk},
};

use crate::AppState;

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

fn toggle_wireframe(mut wireframe_config: ResMut<WireframeConfig>) {
    wireframe_config.global = !wireframe_config.global;
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
    state: Res<State<AppState>>,
    mut ready_at: Local<Option<f32>>,
    mut taken: Local<bool>,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    if *state.get() != AppState::InGame {
        return;
    }
    let elapsed = time.elapsed_secs() - *ready_at.get_or_insert(time.elapsed_secs());
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
