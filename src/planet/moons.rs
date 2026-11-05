//! The planet's moons in the sky (`worldgen::world::World::moons`): toon spheres orbiting
//! slowly, closer and bigger than true scale so they read from the ground and from orbit.

use bevy::prelude::*;

use super::PlanetWorld;
use crate::shaders::{ToonMaterial, ToonShader};

/// Distance from the planet's centre (planet radii) for a moon at the Moon's distance, and
/// per Moon distance beyond (true scale would be 60 radii), at most `MAX_DISTANCE`.
const DISTANCE: (f32, f32) = (5., 4.);
pub const MAX_DISTANCE: f32 = 30.;
/// The Moon's radius (planet radii; true scale 0.27), others by mass^1/3 (same density).
const RADIUS: f32 = 0.27;
/// Angular speed of a moon at the Moon's distance (radians per second; slower further out,
/// as distance^1.5), and the tilt of the orbits from the equator (radians).
const SPEED: f32 = 0.01;
const ORBIT_TILT: f32 = 0.09;

/// A moon's orbit: distance (world units), angular speed, and the orbit's tilt axis.
#[derive(Component)]
pub struct MoonOrbit {
    distance: f32,
    speed: f32,
    angle: f32,
    tilt: Quat,
}

impl MoonOrbit {
    fn position(&self) -> Vec3 {
        self.tilt * Vec3::new(self.angle.cos(), 0., self.angle.sin()) * self.distance
    }
}

pub fn spawn_moons(
    mut commands: Commands,
    world: Res<PlanetWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ToonMaterial>>,
) {
    let planet_radius = world.surface.radius;
    let material = materials.add(ToonMaterial {
        base: StandardMaterial {
            base_color: Color::srgb(0.62, 0.60, 0.57),
            perceptual_roughness: 1.,
            ..default()
        },
        // (No snow.)
        extension: ToonShader {
            tint: LinearRgba::WHITE,
            snow: Vec4::ZERO,
            foliage: crate::shaders::NO_FOLIAGE_TINT,
        },
    });
    for (k, moon) in world.world.moons.iter().enumerate() {
        let distance = planet_radius
            * (DISTANCE.0 + DISTANCE.1 * (moon.distance - 1.)).clamp(2., MAX_DISTANCE);
        let radius = planet_radius * RADIUS * moon.mass.max(0.).cbrt();
        let orbit = MoonOrbit {
            distance,
            speed: SPEED / moon.distance.max(0.1).powf(1.5),
            angle: k as f32 * 2.4,
            tilt: Quat::from_rotation_z(ORBIT_TILT * (1. + k as f32 * 0.5)),
        };
        commands.spawn((
            Name::new(format!("moon {k}")),
            Mesh3d(meshes.add(Sphere::new(radius).mesh().ico(5).unwrap())),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(orbit.position()),
            orbit,
        ));
    }
}

pub fn orbit_moons(time: Res<Time>, mut moons: Query<(&mut MoonOrbit, &mut Transform)>) {
    for (mut orbit, mut transform) in &mut moons {
        orbit.angle += orbit.speed * time.delta_secs();
        transform.translation = orbit.position();
    }
}
