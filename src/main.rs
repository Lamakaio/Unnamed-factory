pub mod map;
pub mod maptext;
pub mod parts;
pub mod ui;
pub mod sim;
use std::{
    f32::consts::{FRAC_PI_2, PI},
    ops::Range,
};

use bevy::{
    core_pipeline::{
        auto_exposure::{AutoExposure, AutoExposurePlugin}, bloom::Bloom, experimental::taa::{TemporalAntiAliasPlugin, TemporalAntiAliasing}, prepass::DepthPrepass, tonemapping::Tonemapping
    },
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    pbr::{
        light_consts::lux, wireframe::{WireframeConfig, WireframePlugin}, Atmosphere, ExtendedMaterial
    },
    prelude::*,
    remote::{http::RemoteHttpPlugin, RemotePlugin},
    render::camera::Exposure,
};
use map::MapPlugin;
use maptext::TerrainShader;
use parts::BuildPlugin;
use sim::SimPlugin;
use ui::UiPlugin;

fn main() {
    let mut app = App::new();
    let seed: u128 = 1082;
    app.add_plugins((
        DefaultPlugins.set(ImagePlugin::default_nearest()),
        WireframePlugin::default(),
    ))
    //.add_plugins(RemotePlugin::default())
    //.add_plugins(RemoteHttpPlugin::default())
    .add_plugins(AutoExposurePlugin)
    .add_plugins(MaterialPlugin::<
        ExtendedMaterial<StandardMaterial, TerrainShader>,
    >::default())
    .insert_resource(CameraSettings::default())
    .add_systems(Startup, (setup_3d,))
    //.add_plugins((BuildPlugin, UiPlugin, MapPlugin { seed }))
    .add_plugins(SimPlugin)
    .add_systems(Update, (toggle_wireframe, orbit, rotate_light));

    app.run();
}

/// Settings for the orientable camera
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
            orbit_distance: 1.0..20.0,
            pitch_speed: 0.003,
            pitch_range: -pitch_limit..pitch_limit,
            yaw_speed: 0.004,
            zoom_speed: 0.05,
        }
    }
}

#[derive(Component)]
struct Sun;

/// Setup the 3D environnement. Mostly a placeholder.
fn setup_3d(
    mut commands: Commands,
    //mut materials: ResMut<Assets<StandardMaterial>>, mut meshes: ResMut<Assets<Mesh>>
) {
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: lux::RAW_SUNLIGHT,
            shadow_depth_bias: 0.05,
            ..default()
        },
        Transform {
            translation: Vec3::new(0.0, 10.0, 0.0),
            rotation: Quat::from_rotation_x(-PI / 4.),
            ..default()
        },
        Sun
    ));

    commands.spawn((
        DirectionalLight {
            shadows_enabled: false,
            illuminance: lux::FULL_MOON_NIGHT,
            ..default()
        },
        Transform {
            translation: Vec3::new(0.0, 10.0, 0.0),
            rotation: Quat::from_rotation_x(-PI / 4.),
            ..default()
        },
    ));

    // //ground plane
    // commands.spawn((
    //     Mesh3d(meshes.add(Plane3d::default().mesh().size(500.0, 500.0).subdivisions(100))),
    //     MeshMaterial3d(materials.add(Color::from(bevy::color::palettes::css::SILVER))),
    //     Transform::from_scale(Vec3::splat(44.0)).with_translation(Vec3::new(0.,0., 0.)).with_rotation(Quat::from_axis_angle(Vec3::Z, 0.))
    // ));

    commands.spawn((
        Camera3d::default(),
        IsDefaultUiCamera,
        // Projection::Perspective(PerspectiveProjection {fov: PI/3., ..Default::default()}),
        // Camera {
        //     hdr: true,
        //     ..default()
        // },
        // Bloom::NATURAL,
        // Exposure::SUNLIGHT,
        // DepthPrepass,
        // //Msaa::Off,
        // //TemporalAntiAliasing::default(),
        // Transform::from_xyz(20.0, 5., 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        // Atmosphere::EARTH,
    ));
}

/// Toggle wireframe on pressing space, for debugging purposes
fn toggle_wireframe(
    mut wireframe_config: ResMut<WireframeConfig>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::Space) {
        wireframe_config.global = !wireframe_config.global;
    }
}

fn rotate_light(
    mut light: Query<&mut Transform, With<Sun>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) -> Result {
    let rotation_speed = 1.;
    let mut light_transform = light.single_mut()?;
    if keyboard_input.pressed(KeyCode::KeyF) {
        light_transform.rotate_axis(Dir3::Z, time.delta_secs() * rotation_speed);
    }

    Ok(())
}

/// Orbiting camera handling
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
    let target = Vec3::new(0., 10., 0.);

    let current_distance = camera.translation.distance(target);
    let delta_scroll = mouse_scroll.delta.y;
    let distance =
        (current_distance + delta_scroll * camera_settings.zoom_speed * current_distance).clamp(
            camera_settings.orbit_distance.start,
            camera_settings.orbit_distance.end,
        );
    camera.translation = target - camera.forward() * distance;
}
