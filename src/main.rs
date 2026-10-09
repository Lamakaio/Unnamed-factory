//! Unnamed Factory: a 3D city builder.

// The `AsBindGroup` derive exceeds the default trait-solver depth on recent nightlies.
#![recursion_limit = "256"]

mod build;
mod build_asset;
mod camera;
mod dev_tools;
mod environment;
mod map;
mod mapgen;
mod shaders;
mod sim;
mod ui;

use bevy::prelude::*;

/// World generation seed.
const SEED: u128 = 1082;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()),
            MeshPickingPlugin,
        ))
        .add_plugins((
            dev_tools::DevToolsPlugin,
            environment::EnvironmentPlugin,
            camera::CameraPlugin,
            map::MapPlugin { seed: SEED },
            shaders::ShadersPlugin,
            build_asset::BuildAssetPlugin,
            build::BuildPlugin,
            ui::UiPlugin,
            sim::SimPlugin,
        ))
        .run();
}
