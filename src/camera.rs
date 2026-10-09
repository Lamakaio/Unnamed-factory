//! The player camera: Bevy's [`PanOrbitCamera`] driven by a custom input mapping,
//! plus keyboard panning and a constraint keeping the camera above the terrain.
//!
//! Controls: right drag orbits, middle drag pans, the wheel zooms (all anchored on the point
//! under the cursor), and the arrow keys pan. Left click is left free for building placement.
//!
//! The input mapping is adapted from Bevy's `pan_orbit_camera_custom_input_plugin` example.
//! Bevy plans to replace it with a first-party input-manager solution.

use std::f32::consts::PI;

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    camera::{Exposure, Hdr, RenderTarget},
    camera_controller::pan_orbit_camera::{
        DefaultPanOrbitCameraPlugins, SyncCameraPosition,
        prelude::component::PanOrbitCamera,
    },
    color::palettes,
    core_pipeline::prepass::DepthPrepass,
    input::mouse::{MouseScrollUnit, MouseWheel},
    math::{DVec2, DVec3, I64Vec2},
    pbr::AtmosphereSettings,
    picking::{
        PickingSystems,
        hover::HoverMap,
        pointer::{
            PointerAction, PointerId, PointerInput, PointerInteraction, PointerLocation, PointerMap,
        },
    },
    platform::collections::HashMap,
    post_process::bloom::Bloom,
    prelude::*,
    transform::TransformSystems,
    window::PrimaryWindow,
};

use crate::map::Map;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(DefaultPanOrbitCameraPlugins)
            .add_message::<CameraInputMessage>()
            .init_resource::<CameraPointerMap>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                PreUpdate,
                (
                    start_and_end_motions,
                    CameraInputMessage::receive_messages,
                    CameraInputMessage::send_pointer_inputs,
                )
                    .chain()
                    .after(PickingSystems::Last)
                    .before(SyncCameraPosition),
            )
            .add_systems(Update, keyboard_pan)
            .add_systems(
                PostUpdate,
                keep_above_terrain.before(TransformSystems::Propagate),
            );
    }
}

/// Mouse button that starts an orbit motion.
const ORBIT_BUTTON: MouseButton = MouseButton::Right;
/// Mouse button that starts a pan motion.
const PAN_BUTTON: MouseButton = MouseButton::Middle;
/// Arrow-key pan speed, in camera heights per second.
const KEYBOARD_PAN_SPEED: f32 = 1.5;
/// Minimum clearance between the camera and the terrain.
const MIN_HEIGHT_ABOVE_TERRAIN: f32 = 1.0;
/// Initial camera offset from the point it looks at.
const INITIAL_OFFSET: Vec3 = Vec3::splat(6.0);

fn spawn_camera(mut commands: Commands, mut map: ResMut<Map>) {
    // Make sure the origin chunk exists so the initial target sits on the terrain.
    map.get_chunk_mut(&I64Vec2::ZERO);
    let target = Vec3::new(0.0, map.get_height(Vec3::ZERO), 0.0);

    commands.spawn((
        Name::new("3d camera"),
        Camera3d::default(),
        IsDefaultUiCamera,
        PanOrbitCamera {
            // Anchor depth (view space, so negative) used when nothing is under the cursor.
            last_anchor_depth: -f64::from(INITIAL_OFFSET.length()),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: PI / 3.,
            ..default()
        }),
        Hdr,
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
        Transform::from_translation(target + INITIAL_OFFSET).looking_at(target, Vec3::Y),
        AtmosphereSettings::default(),
        DistanceFog {
            color: Color::srgba(0.55, 0.58, 0.72, 0.6),
            directional_light_color: Color::srgba(1.0, 0.95, 0.85, 0.5),
            directional_light_exponent: 50.0,
            falloff: FogFalloff::from_visibility_colors(
                300.0, // distance in world units up to which objects retain visibility (>= 5% contrast)
                Color::srgb(0.796, 0.914, 0.929), // atmospheric extinction color (after light is lost due to absorption by atmospheric particles)
                Color::srgb(0.8, 0.844, 1.0), // atmospheric inscattering color (light gained due to scattering from the sun)
            ),
        },
    ));
}

/// Pan the camera horizontally with the arrow keys, faster when the camera is higher up.
fn keyboard_pan(
    mut camera: Single<&mut Transform, With<PanOrbitCamera>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    map: Res<Map>,
    time: Res<Time>,
) {
    let mut input = Vec2::ZERO;
    if keyboard.pressed(KeyCode::ArrowUp) {
        input.y += 1.;
    }
    if keyboard.pressed(KeyCode::ArrowDown) {
        input.y -= 1.;
    }
    if keyboard.pressed(KeyCode::ArrowRight) {
        input.x += 1.;
    }
    if keyboard.pressed(KeyCode::ArrowLeft) {
        input.x -= 1.;
    }
    if input == Vec2::ZERO {
        return;
    }

    let forward = camera.forward().with_y(0.).normalize_or_zero();
    let right = camera.right().with_y(0.).normalize_or_zero();
    let height = (camera.translation.y - map.get_height(camera.translation)).max(5.);
    let movement = (forward * input.y + right * input.x).normalize_or_zero()
        * height
        * KEYBOARD_PAN_SPEED
        * time.delta_secs();
    camera.translation += movement;
}

/// Prevent the camera from going below the terrain.
fn keep_above_terrain(
    mut camera: Single<&mut Transform, (With<PanOrbitCamera>, Changed<Transform>)>,
    map: Res<Map>,
) {
    let min_y = map.get_height(camera.translation) + MIN_HEIGHT_ABOVE_TERRAIN;
    if camera.translation.y < min_y {
        camera.translation.y = min_y;
    }
}

/// The type of mutually exclusive camera motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MotionKind {
    OrbitZoom,
    PanZoom,
    Zoom,
}

/// Maps pointers to the camera they are currently controlling.
#[derive(Debug, Clone, Default, Deref, DerefMut, Resource)]
struct CameraPointerMap(HashMap<PointerId, Entity>);

/// Starts and ends camera motions.
#[derive(Debug, Clone, Message)]
enum CameraInputMessage {
    /// Start moving the camera. Inputs are tracked automatically until [`CameraInputMessage::End`].
    Start {
        kind: MotionKind,
        camera: Entity,
        /// The pointer controlling the camera. The anchor is computed with picking.
        pointer: PointerId,
    },
    /// The user's input ended, e.g. the button was released.
    End { camera: Entity },
}

/// Turn mouse button and wheel input into camera motion start / end messages.
///
/// Motions don't start while the cursor is over UI, so scrolling a list doesn't zoom.
fn start_and_end_motions(
    pointers: Query<(&PointerId, &PointerLocation)>,
    pointer_map: Res<CameraPointerMap>,
    mut controller: MessageWriter<CameraInputMessage>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    cameras: Query<(Entity, &Camera, &RenderTarget, &PanOrbitCamera)>,
    primary_window: Query<Entity, With<PrimaryWindow>>,
    hover_map: Res<HoverMap>,
    ui_nodes: Query<(), With<Node>>,
) {
    let zoom_stop = 0.0;

    if let Some(&camera) = pointer_map.get(&PointerId::Mouse) {
        let camera_query = cameras.get(camera).ok();
        let is_in_zoom_mode = camera_query
            .map(|(.., cam)| cam.current_motion.is_zooming_only())
            .unwrap_or_default();
        let zoom_amount_abs = camera_query
            .and_then(|(.., cam)| {
                cam.current_motion
                    .inputs()
                    .map(|inputs| inputs.zoom_velocity_abs(cam.smoothing.zoom.mul_f32(2.0)))
            })
            .unwrap_or(0.0);
        let should_zoom_end = is_in_zoom_mode && zoom_amount_abs <= zoom_stop;

        if mouse_input.any_just_released([ORBIT_BUTTON, PAN_BUTTON]) || should_zoom_end {
            controller.write(CameraInputMessage::End { camera });
        }
    }

    let over_ui = hover_map
        .get(&PointerId::Mouse)
        .is_some_and(|hits| hits.keys().any(|entity| ui_nodes.contains(*entity)));

    for (&pointer, pointer_location) in pointers
        .iter()
        .filter_map(|(id, loc)| loc.location().map(|loc| (id, loc)))
    {
        if pointer != PointerId::Mouse || over_ui {
            continue;
        }
        let Some((camera, ..)) = cameras.iter().find(|(_, camera, render_target, _)| {
            pointer_location.is_in_viewport(camera, render_target, &primary_window)
        }) else {
            continue; // Pointer must be in viewport to start a motion.
        };

        let kind = if mouse_input.just_pressed(ORBIT_BUTTON) {
            Some(MotionKind::OrbitZoom)
        } else if mouse_input.just_pressed(PAN_BUTTON) {
            Some(MotionKind::PanZoom)
        } else if mouse_wheel.read().map(|mw| mw.y.abs()).sum::<f32>() > 0.0 {
            // Checking for a non-zero y avoids false positives from empty wheel events.
            Some(MotionKind::Zoom)
        } else {
            None
        };
        if let Some(kind) = kind {
            controller.write(CameraInputMessage::Start {
                kind,
                camera,
                pointer,
            });
        }
    }

    // Reading is conditional, so flush the wheel events manually.
    mouse_wheel.clear();
}

impl CameraInputMessage {
    fn camera(&self) -> Entity {
        match self {
            CameraInputMessage::Start { camera, .. } | CameraInputMessage::End { camera } => *camera,
        }
    }

    /// Start and end moves on the [`PanOrbitCamera`], computing the anchor with picking.
    fn receive_messages(
        mut messages: MessageReader<Self>,
        mut controllers: Query<(&mut PanOrbitCamera, &GlobalTransform)>,
        mut camera_map: ResMut<CameraPointerMap>,
        pointer_map: Res<PointerMap>,
        pointer_interactions: Query<&PointerInteraction>,
        pointer_locations: Query<&PointerLocation>,
        cameras: Query<(&Camera, &Projection)>,
        windows: Query<&Window>,
    ) {
        for message in messages.read() {
            let Ok((mut controller, cam_transform)) = controllers.get_mut(message.camera()) else {
                continue;
            };

            match message {
                CameraInputMessage::Start { kind, pointer, .. } => {
                    if controller.is_actively_controlled() {
                        continue;
                    }
                    let anchor = pointer_map
                        .get_entity(*pointer)
                        .and_then(|entity| pointer_interactions.get(entity).ok())
                        .and_then(|interaction| interaction.get_nearest_hit())
                        // A hit on a window is in viewport coordinates, so it can't be an anchor.
                        .filter(|(entity, _hit)| !windows.contains(*entity))
                        .and_then(|(_, hit)| hit.position)
                        .map(|world_space_hit| {
                            // World space to view (camera) space.
                            cam_transform
                                .to_matrix()
                                .as_dmat4()
                                .inverse()
                                .transform_point3(world_space_hit.into())
                        })
                        .filter(|p| p.is_finite())
                        .or_else(|| {
                            let camera = cameras.get(message.camera()).ok();
                            let pointer_location = pointer_map
                                .get_entity(*pointer)
                                .and_then(|entity| pointer_locations.get(entity).ok())
                                .and_then(|l| l.location());
                            let ((camera, proj), pointer_location) =
                                camera.zip(pointer_location)?;
                            screen_to_view_space(
                                camera,
                                proj,
                                &controller,
                                pointer_location.position,
                            )
                        })
                        .filter(|p| p.is_finite());

                    match kind {
                        MotionKind::OrbitZoom => controller.start_orbit(anchor),
                        MotionKind::PanZoom => controller.start_pan(anchor),
                        MotionKind::Zoom => controller.start_zoom(anchor),
                    }
                    camera_map.insert(*pointer, message.camera());
                }
                CameraInputMessage::End { .. } => {
                    controller.end_move();
                    camera_map.retain(|_, camera| *camera != message.camera());
                }
            }
        }
    }

    /// While a motion is active, forward pointer motion and wheel input to the controller.
    fn send_pointer_inputs(
        camera_map: Res<CameraPointerMap>,
        mut controllers: Query<&mut PanOrbitCamera>,
        mut mouse_wheel: MessageReader<MouseWheel>,
        mut moves: MessageReader<PointerInput>,
    ) {
        let moves_list: Vec<_> = moves.read().collect();
        for (pointer, camera) in camera_map.iter() {
            let Ok(mut controller) = controllers.get_mut(*camera) else {
                continue;
            };

            let screenspace_input = moves_list
                .iter()
                .filter(|m| m.pointer_id == *pointer)
                .filter_map(|m| match m.action {
                    PointerAction::Move { delta } => Some(delta),
                    _ => None,
                })
                .sum();

            let zoom_amount = match pointer {
                PointerId::Mouse => mouse_wheel
                    .read()
                    .map(|mw| {
                        let scroll_multiplier = match mw.unit {
                            MouseScrollUnit::Line => 150.0,
                            MouseScrollUnit::Pixel => 1.0,
                        };
                        mw.y * scroll_multiplier
                    })
                    .sum::<f32>(),
                _ => 0.0,
            };

            controller.send_screenspace_input(screenspace_input);
            controller.send_zoom_input(zoom_amount);
        }
        // Reading is conditional, so flush the wheel events manually.
        mouse_wheel.clear();
    }
}

/// Project a cursor position into view space, at the controller's last anchor depth.
fn screen_to_view_space(
    camera: &Camera,
    proj: &Projection,
    controller: &PanOrbitCamera,
    target_position: Vec2,
) -> Option<DVec3> {
    let mut viewport_position = if let Some(rect) = camera.logical_viewport_rect() {
        target_position.as_dvec2() - rect.min.as_dvec2()
    } else {
        target_position.as_dvec2()
    };
    let target_size = camera.logical_viewport_size()?.as_dvec2();
    // Flip the Y coordinate origin from the top to the bottom.
    viewport_position.y = target_size.y - viewport_position.y;
    let ndc = viewport_position * 2. / target_size - DVec2::ONE;
    let ndc_to_view = proj.get_clip_from_view().as_dmat4().inverse();
    let view_near_plane = ndc_to_view.project_point3(ndc.extend(1.));
    match proj {
        Projection::Perspective(_) | Projection::Custom(_) => {
            // EPSILON because an NDC with Z = 0 returns NaNs.
            let view_far_plane = ndc_to_view.project_point3(ndc.extend(f64::EPSILON));
            let direction = (view_far_plane - view_near_plane).normalize();
            Some((direction / direction.z) * controller.last_anchor_depth())
        }
        Projection::Orthographic(_) => Some(DVec3::new(
            view_near_plane.x,
            view_near_plane.y,
            controller.last_anchor_depth(),
        )),
    }
}
