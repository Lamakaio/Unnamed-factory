//! Unnamed Factory: a living planet (see `docs/living-world.md`).

// The `AsBindGroup` derive exceeds the default trait-solver depth on recent nightlies.
#![recursion_limit = "256"]

mod camera;
mod dev_tools;
mod environment;
mod planet;
mod shaders;
mod ui;

use bevy::prelude::*;

/// Default world generation seed.
const SEED: u128 = 1082;

/// Cells along a cube face edge of the planet (`--n <cells>`), see `planet`.
const PLANET_CELLS: usize = 1024;

/// The value following `flag` on the command line, if any.
fn argument<T: std::str::FromStr>(flag: &str) -> Option<T> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|i| args.get(i + 1))
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|_| panic!("{flag}: bad value {value}"))
        })
}

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
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(ImagePlugin::default_nearest()),
        MeshPickingPlugin,
    ))
    .init_state::<AppState>()
    .add_plugins((
        dev_tools::DevToolsPlugin,
        environment::EnvironmentPlugin,
        camera::CameraPlugin,
        shaders::ShadersPlugin,
        ui::UiPlugin,
    ));
    app.add_plugins(planet::PlanetPlugin {
        seed,
        n: argument("--n").unwrap_or(PLANET_CELLS),
    })
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
