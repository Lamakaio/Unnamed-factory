//! The 3D camera, spawned at startup (it also renders the loading screen). The planet's
//! camera controller (`planet::camera`) takes it over once the planet exists.

use std::f32::consts::PI;

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    camera::{Exposure, Hdr},
    core_pipeline::prepass::DepthPrepass,
    post_process::bloom::Bloom,
    prelude::*,
};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera);
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Name::new("3d camera"),
        Camera3d::default(),
        IsDefaultUiCamera,
        Projection::Perspective(PerspectiveProjection {
            fov: PI / 3.,
            ..default()
        }),
        Hdr,
        Bloom::NATURAL,
        Exposure::SUNLIGHT,
        AmbientLight {
            // Sky light: soft blue, weak enough for the sun to shape the relief.
            color: Color::srgb(0.62, 0.72, 0.95),
            brightness: 9000.,
            ..default()
        },
        DepthPrepass,
        Msaa::Off,
        TemporalAntiAliasing::default(),
        Transform::from_xyz(0., 0., 2000.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
