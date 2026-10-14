//! The planet's camera: it looks at a point of the surface (its "focus") from a distance,
//! with a forward direction along the ground, carried along (parallel transported) when the
//! focus moves, so it never flips over the poles. Far out it looks straight down at the
//! planet; close in it can tilt toward the horizon.
//!
//! Controls: left (or middle) drag moves the focus over the surface (the ground follows the
//! cursor), Q / E turn the view (physical keys: A / E on an AZERTY keyboard), right drag orbits
//! and tilts, the arrow keys move the focus, the wheel zooms.

use bevy::{
    camera_controller::pan_orbit_camera::prelude::component::PanOrbitCamera,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
};

use super::PlanetWorld;
use crate::{
    environment::{Sun, SunTurn},
    ui::PointerOverUi,
};

/// Orbit speed (radians per pixel), pan speed (distance units per pixel per unit of camera
/// distance), keyboard pan speed (camera distances per second) and zoom per wheel line.
const ORBIT_SPEED: f32 = 0.005;
const PAN_SPEED: f32 = 0.0015;
const KEY_PAN_SPEED: f32 = 0.8;
/// Turn speed with Q / E (radians per second).
const KEY_TURN_SPEED: f32 = 1.5;
const ZOOM_PER_LINE: f32 = 0.15;
/// Closest and farthest camera distances (world units; the farthest in planet radii).
const MIN_DISTANCE: f32 = 2.;
const MAX_DISTANCE: f32 = 4.;
/// Lowest tilt above the horizon close in, and the distances (planet radii) over which the
/// view turns from that to straight down.
const MIN_PITCH: f32 = 0.12;
const TOP_DOWN: (f32, f32) = (0.15, 1.2);
/// Clearance kept between the camera and the ground.
const CLEARANCE: f32 = 1.;

/// The planet camera's state.
#[derive(Component)]
pub struct PlanetCamera {
    /// Unit direction of the focus point from the planet's centre.
    pub focus: Vec3,
    /// Tangent direction the camera looks along (at the focus).
    pub forward: Vec3,
    /// Distance from the focus point.
    pub distance: f32,
    /// Angle above the horizon the camera looks down from (π/2: straight down).
    pub pitch: f32,
}

/// Replace the flat map's orbit controller with the planet camera, looking at `focus` from
/// `distance` (straight down unless `pitch` says otherwise).
pub fn take_over(
    commands: &mut Commands,
    camera: Entity,
    focus: Vec3,
    distance: f32,
    pitch: Option<f32>,
) {
    let focus = focus.normalize();
    let forward = (Vec3::Y - focus * focus.y)
        .try_normalize()
        .unwrap_or(Vec3::X.reject_from(focus).normalize());
    commands.entity(camera).remove::<PanOrbitCamera>().insert(PlanetCamera {
        focus,
        forward,
        distance,
        pitch: pitch.unwrap_or(std::f32::consts::FRAC_PI_2),
    });
}

/// Turn the mouse and keyboard into camera motion.
pub fn control(
    camera: Single<&mut PlanetCamera>,
    world: Res<PlanetWorld>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    over_ui: PointerOverUi,
) {
    let mut camera = camera.into_inner();
    let radius = world.surface.radius;
    let over_ui = over_ui.get();
    // Zoom.
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.,
    };
    if lines != 0. && !over_ui {
        camera.distance = (camera.distance * (-lines * ZOOM_PER_LINE).exp())
            .clamp(MIN_DISTANCE, MAX_DISTANCE * radius);
    }
    // Turn with Q / E (key positions: the same keys on any layout).
    let turn = keys.pressed(KeyCode::KeyE) as i32 as f32 - keys.pressed(KeyCode::KeyQ) as i32 as f32;
    if turn != 0. {
        let up = camera.focus;
        camera.forward = Quat::from_axis_angle(up, -turn * KEY_TURN_SPEED * time.delta_secs()) * camera.forward;
    }
    // Orbit.
    if buttons.pressed(MouseButton::Right) && !over_ui {
        let delta = motion.delta * ORBIT_SPEED;
        let up = camera.focus;
        camera.forward = Quat::from_axis_angle(up, -delta.x) * camera.forward;
        camera.pitch += delta.y;
    }
    // Pan: move the focus over the surface (screen right / up).
    let mut pan = Vec2::ZERO;
    if buttons.any_pressed([MouseButton::Left, MouseButton::Middle]) && !over_ui {
        pan += Vec2::new(-motion.delta.x, motion.delta.y) * PAN_SPEED * camera.distance;
    }
    let keys_pan = Vec2::new(
        keys.pressed(KeyCode::ArrowRight) as i32 as f32 - keys.pressed(KeyCode::ArrowLeft) as i32 as f32,
        keys.pressed(KeyCode::ArrowUp) as i32 as f32 - keys.pressed(KeyCode::ArrowDown) as i32 as f32,
    );
    pan += keys_pan * KEY_PAN_SPEED * camera.distance * time.delta_secs();
    if pan != Vec2::ZERO {
        let up = camera.focus;
        let right = camera.forward.cross(up);
        let shift = right * pan.x + camera.forward * pan.y;
        let focus = (up * radius + shift).normalize();
        // Carry the forward direction along: keep it tangent at the new focus.
        let forward = camera.forward.reject_from(focus).normalize_or_zero();
        if forward != Vec3::ZERO {
            camera.forward = forward;
        }
        camera.focus = focus;
    }
    // Far out, the view turns straight down.
    let top_down = smoothstep(TOP_DOWN.0, TOP_DOWN.1, camera.distance / radius);
    let min_pitch = MIN_PITCH + (std::f32::consts::FRAC_PI_2 - 0.01 - MIN_PITCH) * top_down;
    camera.pitch = camera.pitch.clamp(min_pitch, std::f32::consts::FRAC_PI_2 - 0.01);
}

/// Place the camera from its state, above the ground, with clip planes fitting the view.
pub fn apply(
    camera: Single<(&PlanetCamera, &mut Transform, &mut Projection)>,
    world: Res<PlanetWorld>,
) {
    let (state, mut transform, mut projection) = camera.into_inner();
    let surface = &world.surface;
    let up = state.focus;
    let ground = up * (surface.radius + surface.height_at(up).max(0.));
    let back = -state.forward;
    let offset = (back * state.pitch.cos() + up * state.pitch.sin()) * state.distance;
    let mut position = ground + offset;
    // Never below the ground under it.
    let below = position.normalize();
    let floor = surface.radius + surface.height_at(below).max(0.) + CLEARANCE;
    if position.length() < floor {
        position = below * floor;
    }
    *transform = Transform::from_translation(position).looking_at(ground, up);
    if let Projection::Perspective(perspective) = projection.as_mut() {
        let altitude = position.length() - surface.radius;
        perspective.near = (altitude * 0.02).clamp(0.05, 50.);
        perspective.far = position.length() + surface.radius * 1.2;
    }
}

/// Height of the sun above the horizon at the focus.
const SUN_ELEVATION: f32 = 0.6;

/// Until the planet turns, the sun follows the view: a low afternoon sun from the side over
/// the focus (turned with F), so whatever the camera looks at is lit.
pub fn follow_sun(
    camera: Single<&PlanetCamera>,
    mut sun: Single<&mut Transform, With<Sun>>,
    turn: Res<SunTurn>,
) {
    let up = camera.focus;
    let side = Quat::from_axis_angle(up, 2.2 + turn.0) * camera.forward;
    let towards_sun = (up * SUN_ELEVATION.sin() + side * SUN_ELEVATION.cos()).normalize();
    **sun = Transform::IDENTITY.looking_to(-towards_sun, up);
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
