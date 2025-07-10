pub mod build;
pub mod build_asset;
pub mod map;
pub mod shaders;
pub mod sim;
pub mod ui;
pub mod mapgen;

use std::{
    f32::consts::{FRAC_PI_2, PI},
    ops::Range,
};

use bevy::{
    color::palettes, core_pipeline::{
        bloom::Bloom,
        experimental::taa::{TemporalAntiAliasPlugin, TemporalAntiAliasing},
        prepass::DepthPrepass,
    }, input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll}, pbr::{
        light_consts::lux, wireframe::{WireframeConfig, WireframePlugin}, Atmosphere
    }, prelude::*, remote::{http::RemoteHttpPlugin, RemotePlugin}, render::{camera::Exposure, primitives::Aabb}
};
use build::BuildPlugin;
use build_asset::BuildAssetPlugin;
use map::{Map, MapPlugin};
use shaders::ShadersPlugin;
use sim::SimPlugin;
use ui::UiPlugin;

use crate::build::BuildId;

fn main() {
    let mut app = App::new();
    let seed: u128 = 1082;
    app.add_plugins((
        DefaultPlugins.set(ImagePlugin::default_nearest()),
        WireframePlugin::default(),
        TemporalAntiAliasPlugin,
    ))
    .add_plugins(RemotePlugin::default())
    .add_plugins(RemoteHttpPlugin::default())
    .insert_resource(CameraSettings::default())
    .add_systems(Startup, (setup_3d,))
    .add_plugins((
        BuildPlugin,
        UiPlugin,
        MapPlugin { seed },
        ShadersPlugin,
        BuildAssetPlugin,
    ))
    .add_plugins(SimPlugin)
    .add_systems(
        Update,
        (toggle_wireframe, orbit, rotate_light, toggle_bounding_box),
    );

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
    pub pan_speed: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        // Limiting pitch stops some unexpected rotation past 90° up or down.
        let pitch_limit = FRAC_PI_2 - 0.01;
        Self {
            // These values are completely arbitrary, chosen because they seem to produce
            // "sensible" results for this example. Adjust as required.
            orbit_distance: 1.0..100.0,
            pitch_speed: 0.003,
            pitch_range: -pitch_limit..pitch_limit,
            yaw_speed: 0.004,
            zoom_speed: 0.05,
            pan_speed: 3.,
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
        Name::new("Sun"),
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
        Sun,
    ));

    // //ground plane
    // commands.spawn((
    //     Mesh3d(meshes.add(Plane3d::default().mesh().size(500.0, 500.0).subdivisions(100))),
    //     MeshMaterial3d(materials.add(Color::from(bevy::color::palettes::css::SILVER))),
    //     Transform::from_scale(Vec3::splat(44.0)).with_translation(Vec3::new(0.,0., 0.)).with_rotation(Quat::from_axis_angle(Vec3::Z, 0.))
    // ));

    commands.spawn((
        Name::new("3d camera"),
        Camera3d::default(),
        IsDefaultUiCamera,
        CameraTarget {
            pos: Vec3::default(),
            distance: 10.,
        },
        Projection::Perspective(PerspectiveProjection {
            fov: PI / 3.,
            ..Default::default()
        }),
        Camera {
            hdr: true,
            ..default()
        },
        Bloom::NATURAL,
        Exposure::SUNLIGHT,
        AmbientLight {
            color: palettes::css::MIDNIGHT_BLUE.lighter(0.1).into(),
            brightness: 30000.,
            ..default()
        },
        DepthPrepass,
        Msaa::Off,
        TemporalAntiAliasing::default(),
        Transform::from_xyz(20.0, 20., 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        Atmosphere::EARTH,
        DistanceFog {
            color: Color::srgba(0.55, 0.58, 0.72, 0.6),
            directional_light_color: Color::srgba(1.0, 0.95, 0.85, 0.5),
            directional_light_exponent: 50.0,
            falloff: FogFalloff::from_visibility_colors(
                300.0, // distance in world units up to which objects retain visibility (>= 5% contrast)
                Color::srgb(0.796, 0.914, 0.929), // atmospheric extinction color (after light is lost due to absorption by atmospheric particles)
                Color::srgb(0.8, 0.844, 1.0), // atmospheric inscattering color (light gained due to scattering from the sun)
            ),
        }
        //DistanceFog::default()
        //ScreenSpaceAmbientOcclusion::default()
    ));
}

/// Toggle wireframe on pressing space, for debugging purposes
fn toggle_wireframe(
    mut wireframe_config: ResMut<WireframeConfig>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        wireframe_config.global = !wireframe_config.global;
    }
}
#[derive(Default)]
struct BoundingBoxConfig(pub bool);

fn toggle_bounding_box(
    mut bb_config: Local<BoundingBoxConfig>,
    keyboard: Res<ButtonInput<KeyCode>>,
    aabb_query: Query<(&Aabb, &GlobalTransform), With<BuildId>>,
    mut gizmos: Gizmos,
) {
    if keyboard.just_pressed(KeyCode::F2) {
        bb_config.0 = !bb_config.0;
    }
    if bb_config.0 {
        for (aabb, transform) in aabb_query {
            gizmos.cuboid(
                Transform::from_translation(
                    Vec3::from(aabb.center) * transform.scale() + transform.translation(),
                )
                .with_scale(
                    transform
                        .rotation()
                        .mul_vec3(Vec3::from(aabb.half_extents) * transform.scale() * 2.),
                ),
                bevy::color::palettes::css::ORANGE_RED,
            );
        }
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

#[derive(Component)]
pub struct CameraTarget {
    pos: Vec3,
    distance: f32,
}

/// Orbiting camera handling
fn orbit(
    mut camera: Single<(&mut Transform, &mut CameraTarget), With<Camera>>,
    camera_settings: Res<CameraSettings>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    map: Res<Map>,
    time: Res<Time>,
) {
    let (camera_transform, camera_target) = &mut *camera;
    if mouse_buttons.pressed(MouseButton::Right) {
        let delta = mouse_motion.delta;

        // Mouse motion is one of the few inputs that should not be multiplied by delta time,
        // as we are already receiving the full movement since the last frame was rendered. Multiplying
        // by delta time here would make the movement slower that it should be.
        let delta_pitch = -delta.y * camera_settings.pitch_speed;
        let delta_yaw = -delta.x * camera_settings.yaw_speed;

        // Obtain the existing pitch, yaw, and roll values from the transform.
        let (yaw, pitch, roll) = camera_transform.rotation.to_euler(EulerRot::YXZ);

        // Establish the new yaw and pitch, preventing the pitch value from exceeding our limits.
        let pitch = (pitch + delta_pitch).clamp(
            camera_settings.pitch_range.start,
            camera_settings.pitch_range.end,
        );
        let yaw = yaw + delta_yaw;
        camera_transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
    }

    // Adjust the translation to maintain the correct orientation toward the orbit target at the desired orbit distance.

    let mut movement = Vec3::default();
    // Move the target if needed
    if keyboard_input.pressed(KeyCode::ArrowDown) {
        movement += Vec3::Z;
    }
    if keyboard_input.pressed(KeyCode::ArrowUp) {
        movement -= Vec3::Z;
    }
    if keyboard_input.pressed(KeyCode::ArrowLeft) {
        movement -= Vec3::X;
    }
    if keyboard_input.pressed(KeyCode::ArrowRight) {
        movement += Vec3::X;
    }
    movement *= time.delta_secs() * camera_settings.pan_speed * camera_target.distance;

    camera_target.pos += camera_transform.rotation.mul_vec3(movement);

    let height =  map.get_height(camera_target.pos);
    camera_target.pos.y = height;

    let delta_scroll = -mouse_scroll.delta.y;
    camera_target.distance += delta_scroll * camera_settings.zoom_speed * camera_target.distance;
    camera_target.distance = camera_target.distance.clamp(
        camera_settings.orbit_distance.start,
        camera_settings.orbit_distance.end,
    );
    camera_transform.translation =
        camera_target.pos - camera_transform.forward() * camera_target.distance;

    camera_transform.translation.y = camera_transform
        .translation
        .y
        .max(map.get_height(camera_transform.translation) + 1.)
}
