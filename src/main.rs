//! This example illustrates scrolling in Bevy UI.

pub mod parts;
pub mod ui;
use std::{f32::consts::FRAC_PI_2, ops::Range};

use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    pbr::wireframe::{WireframeConfig, WireframePlugin},
    prelude::*, remote::{http::RemoteHttpPlugin, RemotePlugin},
};
use parts::PartsPlugin;
use ui::UFGUiPlugin;

fn main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(ImagePlugin::default_nearest()),
        WireframePlugin,
    ))
    .add_plugins(RemotePlugin::default())
    .add_plugins(RemoteHttpPlugin::default())
    .insert_resource(CameraSettings::default())
    .add_systems(Startup, setup_3d)
    .add_plugins((PartsPlugin, UFGUiPlugin))
    .add_systems(Update, (toggle_wireframe, orbit));

    app.run();
}

#[derive(Debug, Resource)]
struct CameraSettings {
    pub orbit_distance: Range<f32>,
    pub pitch_speed: f32,
    // Clamp pitch to this range
    pub pitch_range: Range<f32>,
    pub yaw_speed: f32,
    pub zoom_speed: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        // Limiting pitch stops some unexpected rotation past 90° up or down.
        let pitch_limit = FRAC_PI_2 - 0.01;
        Self {
            // These values are completely arbitrary, chosen because they seem to produce
            // "sensible" results for this example. Adjust as required.
            orbit_distance: 2.0..50.0,
            pitch_speed: 0.003,
            pitch_range: -pitch_limit..pitch_limit,
            yaw_speed: 0.004,
            zoom_speed: 0.05,
        }
    }
}

fn setup_3d(
    mut commands: Commands,
    //mut meshes: ResMut<Assets<Mesh>>,
    //mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            intensity: 10_000_000.,
            range: 100.0,
            shadow_depth_bias: 0.2,
            ..default()
        },
        Transform::from_xyz(8.0, 16.0, 8.0),
    ));

    // ground plane
    // commands.spawn((
    //     Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0).subdivisions(10))),
    //     MeshMaterial3d(materials.add(Color::from(bevy::color::palettes::css::SILVER))),
    // ));

    commands.spawn((
        Camera3d::default(),
        IsDefaultUiCamera,
        Transform::from_xyz(0.0, 7., 14.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn toggle_wireframe(
    mut wireframe_config: ResMut<WireframeConfig>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::Space) {
        wireframe_config.global = !wireframe_config.global;
    }
}

fn orbit(
    mut camera: Single<&mut Transform, With<Camera>>,
    camera_settings: Res<CameraSettings>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    mouse_motion: Res<AccumulatedMouseMotion>,
) {
    if mouse_buttons.pressed(MouseButton::Right) {
        let delta = mouse_motion.delta;

        // Mouse motion is one of the few inputs that should not be multiplied by delta time,
        // as we are already receiving the full movement since the last frame was rendered. Multiplying
        // by delta time here would make the movement slower that it should be.
        let delta_pitch = -delta.y * camera_settings.pitch_speed;
        let delta_yaw = -delta.x * camera_settings.yaw_speed;

        // Obtain the existing pitch, yaw, and roll values from the transform.
        let (yaw, pitch, roll) = camera.rotation.to_euler(EulerRot::YXZ);

        // Establish the new yaw and pitch, preventing the pitch value from exceeding our limits.
        let pitch = (pitch + delta_pitch).clamp(
            camera_settings.pitch_range.start,
            camera_settings.pitch_range.end,
        );
        let yaw = yaw + delta_yaw;
        camera.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
    }

    // Adjust the translation to maintain the correct orientation toward the orbit target at the desired orbit distance.
    // Here, it's a static target, but this could easily be customized.
    let target = Vec3::ZERO;

    let current_distance = camera.translation.distance(target);
    let delta_scroll = mouse_scroll.delta.y;
    let distance =
        (current_distance + delta_scroll * camera_settings.zoom_speed * current_distance).clamp(
            camera_settings.orbit_distance.start,
            camera_settings.orbit_distance.end,
        );
    camera.translation = target - camera.forward() * distance;
}
