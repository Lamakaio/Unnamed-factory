//! The planet's camera: it looks at a point of the surface (its "focus") from a distance,
//! with a forward direction along the ground, carried along (parallel transported) when the
//! focus moves, so it never flips over the poles. Far out it looks straight down at the
//! planet; close in it can tilt toward the horizon.
//!
//! Controls: left (or middle) drag moves the focus over the surface (the ground follows the
//! cursor), Q / E turn the view (physical keys: A / E on an AZERTY keyboard), right drag orbits
//! and tilts, the arrow keys move the focus, the wheel zooms.

use bevy::{
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

/// A view as `UF_PLANET_CAMERA` gives it (see [`View::parse`]).
#[derive(Clone, Copy)]
pub struct View {
    pub focus: Vec3,
    pub distance: f32,
    pub pitch: Option<f32>,
    pub heading: Option<f32>,
}

impl View {
    /// Parse `longitude,latitude[,distance[,pitch[,heading]]]` (degrees, world units); what is
    /// left out comes from `default`.
    pub fn parse(view: &str, default: View) -> View {
        let values: Vec<f32> = view.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let mut out = default;
        if let [longitude, latitude, ..] = values[..] {
            let (longitude, latitude) = (longitude.to_radians(), latitude.to_radians());
            out.focus = Vec3::new(
                latitude.cos() * longitude.cos(),
                latitude.sin(),
                latitude.cos() * longitude.sin(),
            );
        }
        if let [_, _, d, ..] = values[..] {
            out.distance = d;
        }
        if let [_, _, _, p, ..] = values[..] {
            out.pitch = Some(p.to_radians());
        }
        if let [_, _, _, _, h] = values[..] {
            out.heading = Some(h.to_radians());
        }
        out
    }

    /// Point the planet camera this way.
    pub fn apply(&self, camera: &mut PlanetCamera) {
        let focus = self.focus.normalize();
        let (east, north) = east_north(focus);
        let heading = self.heading.unwrap_or(0.);
        camera.focus = focus;
        camera.forward = north * heading.cos() + east * heading.sin();
        camera.distance = self.distance;
        camera.pitch = self.pitch.unwrap_or(std::f32::consts::FRAC_PI_2);
    }
}

/// Take the camera over with the planet camera, looking at `focus` from
/// `distance` (straight down unless `pitch` says otherwise), facing `heading` (radians east
/// of north; north by default).
pub fn take_over(
    commands: &mut Commands,
    camera: Entity,
    focus: Vec3,
    distance: f32,
    pitch: Option<f32>,
    heading: Option<f32>,
) {
    let focus = focus.normalize();
    let (east, north) = east_north(focus);
    let heading = heading.unwrap_or(0.);
    let forward = north * heading.cos() + east * heading.sin();
    commands.entity(camera).insert(PlanetCamera {
        focus,
        forward,
        distance,
        pitch: pitch.unwrap_or(std::f32::consts::FRAC_PI_2),
    });
}

/// East and north unit tangents at a unit direction (north falls back near the poles).
pub fn east_north(direction: Vec3) -> (Vec3, Vec3) {
    let north = (Vec3::Y - direction * direction.y)
        .try_normalize()
        .unwrap_or(Vec3::X.reject_from(direction).normalize());
    (direction.cross(north), north)
}

/// The view as `UF_PLANET_CAMERA` takes it: longitude, latitude, distance, pitch, heading
/// (degrees, world units).
pub fn describe(camera: &PlanetCamera) -> String {
    let f = camera.focus;
    let (east, north) = east_north(f);
    let heading = camera.forward.dot(east).atan2(camera.forward.dot(north));
    format!(
        "{:.2},{:.2},{:.1},{:.1},{:.1}",
        f.z.atan2(f.x).to_degrees(),
        f.y.clamp(-1., 1.).asin().to_degrees(),
        camera.distance,
        camera.pitch.to_degrees(),
        heading.to_degrees()
    )
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
    inspector: Res<super::inspect::Inspector>,
) {
    let mut camera = camera.into_inner();
    // (Keys typed into an issue report don't move the camera.)
    let pressed = |key: KeyCode| !inspector.typing() && keys.pressed(key);
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
    let turn = pressed(KeyCode::KeyE) as i32 as f32 - pressed(KeyCode::KeyQ) as i32 as f32;
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
        pressed(KeyCode::ArrowRight) as i32 as f32 - pressed(KeyCode::ArrowLeft) as i32 as f32,
        pressed(KeyCode::ArrowUp) as i32 as f32 - pressed(KeyCode::ArrowDown) as i32 as f32,
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
