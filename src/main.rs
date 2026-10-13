//! Unnamed Factory: a living planet (see `docs/living-world.md`).

// The `AsBindGroup` derive exceeds the default trait-solver depth on recent nightlies.
#![recursion_limit = "256"]

mod camera;
mod dev_tools;
mod environment;
mod map;
mod shaders;
mod ui;

use bevy::prelude::*;

/// Default world generation seed.
const SEED: u128 = 1082;

/// The world seed: `--seed <n>` on the command line, else [`SEED`].
fn seed() -> u128 {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|arg| arg == "--seed")
        .and_then(|i| args.get(i + 1))
        .map(|value| value.parse().expect("--seed takes a number"))
        .unwrap_or(SEED)
}

fn main() {
    let seed = seed();
    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()),
            MeshPickingPlugin,
        ))
        .init_state::<AppState>()
        .add_plugins((
            dev_tools::DevToolsPlugin,
            environment::EnvironmentPlugin,
            camera::CameraPlugin,
            map::MapPlugin { seed },
            shaders::ShadersPlugin,
            ui::UiPlugin,
        ))
        .run();
}

/// Top-level game state.
#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
pub enum AppState {
    /// The world is being generated in the background.
    #[default]
    Loading,
    /// The world is ready: terrain, camera control and debug tools run.
    InGame,
}
