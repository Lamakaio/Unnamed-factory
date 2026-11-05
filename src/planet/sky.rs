//! The sky (`worldgen::world::World::sky_color`: the air's blue, haze, dust, in the star's
//! light): from orbit a toon glow round the planet (`atmosphere.wesl`); near the ground the
//! sky's colour behind the terrain instead of space (the clear colour, by the camera's
//! altitude).

use bevy::{light::NotShadowCaster, prelude::*};

use super::PlanetWorld;
use crate::shaders::AtmosphereMaterial;

/// Space's colour.
pub const SPACE: Color = Color::srgb(0.01, 0.012, 0.02);
/// The atmosphere shell's radius (planet radii) and glow strength.
const SHELL: f32 = 1.035;
const GLOW: f32 = 0.6;
/// The sky shows behind the terrain below this altitude (planet radii), fully below the
/// second.
const SKY_ALTITUDE: (f32, f32) = (0.45, 0.08);

pub fn spawn_atmosphere(
    mut commands: Commands,
    world: Res<PlanetWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<AtmosphereMaterial>>,
) {
    let [r, g, b] = world.world.sky_color();
    commands.spawn((
        Name::new("atmosphere"),
        Mesh3d(
            meshes.add(
                Sphere::new(world.surface.radius * SHELL)
                    .mesh()
                    .ico(6)
                    .unwrap(),
            ),
        ),
        MeshMaterial3d(materials.add(AtmosphereMaterial {
            sky: LinearRgba::new(r, g, b, GLOW),
        })),
        NotShadowCaster,
    ));
}

/// The sky's colour behind the terrain, by the camera's altitude.
pub fn sky_behind(
    world: Res<PlanetWorld>,
    camera: Single<&Transform, With<Camera3d>>,
    mut clear: ResMut<ClearColor>,
) {
    let altitude = camera.translation.length() / world.surface.radius - 1.;
    let near = SKY_ALTITUDE.0 - altitude;
    let amount = (near / (SKY_ALTITUDE.0 - SKY_ALTITUDE.1)).clamp(0., 1.);
    let amount = amount * amount * (3. - 2. * amount);
    let [r, g, b] = world.world.sky_color();
    let color = SPACE.mix(&Color::linear_rgb(r, g, b), amount);
    if clear.0 != color {
        clear.0 = color;
    }
}
