//! Sun and atmosphere.

use bevy::{
    input::common_conditions::input_pressed,
    light::{
        Atmosphere, CascadeShadowConfigBuilder, DirectionalLightShadowMap,
        atmosphere::ScatteringMedium, light_consts::lux,
    },
    prelude::*,
};

pub struct EnvironmentPlugin;

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DirectionalLightShadowMap { size: 4096 })
            .init_resource::<SunTurn>()
            .add_systems(Startup, spawn_environment)
            .add_systems(Update, rotate_sun.run_if(input_pressed(KeyCode::KeyF)));
    }
}

/// Marker for the directional light acting as the sun.
#[derive(Component)]
pub struct Sun;

/// How far the sun was turned with F (radians), for views that place the sun themselves
/// (the planet's).
#[derive(Resource, Default)]
pub struct SunTurn(pub f32);

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
        // Most shadow map resolution goes to the ground near the camera.
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            first_cascade_far_bound: 25.,
            maximum_distance: 600.,
            ..default()
        }
        .build(),
        // Low afternoon sun from the side of the starting view, so relief casts shadows.
        Transform::from_xyz(0.0, 10.0, 0.0).looking_to(Vec3::new(0.75, -0.55, -0.35), Vec3::Y),
    ));

    // The atmosphere is its own entity; the camera only carries `AtmosphereSettings`.
    commands.spawn((
        Name::new("Atmosphere"),
        Atmosphere::earth(scattering_mediums.add(ScatteringMedium::default())),
    ));
}

fn rotate_sun(mut sun: Single<&mut Transform, With<Sun>>, mut turn: ResMut<SunTurn>, time: Res<Time>) {
    sun.rotate_axis(Dir3::Z, time.delta_secs() * SUN_ROTATION_SPEED);
    turn.0 += time.delta_secs() * SUN_ROTATION_SPEED;
}
