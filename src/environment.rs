//! Sun and atmosphere.

use std::f32::consts::PI;

use bevy::{
    input::common_conditions::input_pressed,
    light::{Atmosphere, atmosphere::ScatteringMedium, light_consts::lux},
    prelude::*,
};

pub struct EnvironmentPlugin;

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_environment)
            .add_systems(Update, rotate_sun.run_if(input_pressed(KeyCode::KeyF)));
    }
}

/// Marker for the directional light acting as the sun.
#[derive(Component)]
struct Sun;

/// Sun rotation speed while F is held, in radians per second.
const SUN_ROTATION_SPEED: f32 = 1.;

fn spawn_environment(
    mut commands: Commands,
    mut scattering_mediums: ResMut<Assets<ScatteringMedium>>,
) {
    commands.spawn((
        Name::new("Sun"),
        Sun,
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: lux::RAW_SUNLIGHT,
            shadow_depth_bias: 0.05,
            ..default()
        },
        Transform::from_xyz(0.0, 10.0, 0.0).with_rotation(Quat::from_rotation_x(-PI / 4.)),
    ));

    // The atmosphere is its own entity; the camera only carries `AtmosphereSettings`.
    commands.spawn((
        Name::new("Atmosphere"),
        Atmosphere::earth(scattering_mediums.add(ScatteringMedium::default())),
    ));
}

fn rotate_sun(mut sun: Single<&mut Transform, With<Sun>>, time: Res<Time>) {
    sun.rotate_axis(Dir3::Z, time.delta_secs() * SUN_ROTATION_SPEED);
}
