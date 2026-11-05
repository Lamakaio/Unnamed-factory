//! The sky.
//!
//! - **The air**: Bevy's atmosphere (Hillaire 2020: the sky, the glow seen from orbit, the
//!   haze over the terrain), centred on the planet and scaled to it, its make-up from the
//!   world's (`worldgen::sky::air`: thicker with CO₂, dusty, orange with methane haze, ozone
//!   with oxygen). Thicker than true scale (the relief is exaggerated too), its density
//!   lowered as much, so that light crosses as much air as on Earth.
//! - **Clouds and local haze**: the sky map (`worldgen::sky::SkyField`, from the climate),
//!   redrawn a few times a second in the background as the clouds drift: the texture holds two
//!   snapshots side by side (now, and `REDRAW` seconds on), which the shaders blend by the
//!   clock, so the clouds move smoothly between redraws. One texture for the
//!   cloud layer (`clouds.wesl`), the clouds' shadows and the haze on the ground, the sea
//!   and the plants (their materials sample it), so all of them agree.

use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    light::{
        Atmosphere, NotShadowCaster,
        atmosphere::{Falloff, PhaseFunction, ScatteringMedium, ScatteringTerm},
    },
    pbr::{AtmosphereMode, AtmosphereSettings},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    tasks::{AsyncComputeTaskPool, Task, block_on},
};
use worldgen::sky::{AirKind, HEIGHT, SkyField, WIDTH, air};

use super::PlanetWorld;
use crate::shaders::CloudMaterial;

/// Space's colour.
pub const SPACE: Color = Color::srgb(0.01, 0.012, 0.02);
/// Earth's radius (m): the atmosphere is computed for an Earth-sized planet, then scaled.
const EARTH_RADIUS: f32 = 6.36e6;
/// The atmosphere's thickness (planet radii; Earth's 100 km would be 0.016, below the
/// exaggerated mountains), and Earth's (m) for the density to keep its optical depth.
const THICKNESS: f32 = 0.15;
const EARTH_THICKNESS: f32 = 1e5;
/// The cloud layer's height (planet radii: about 2.5 km in the relief's units).
pub const CLOUD_HEIGHT: f32 = 0.038;
/// How often the clouds are redrawn (seconds: `SKY_REDRAW` in the shaders, which blend
/// between the snapshots by `globals.time`, the same wrapped clock as
/// `Time::elapsed_secs_wrapped`).
const REDRAW: f32 = 0.2;

/// The sky map and its making.
#[derive(Resource)]
pub struct SkyMap {
    pub image: Handle<Image>,
    /// The cloud layer's radius (world units).
    pub cloud_radius: f32,
    field: Arc<SkyField>,
    /// The redraw shown (the snapshots of `slot` and `slot + 1`), the snapshot of `slot + 1`,
    /// and, being made in the background, the next texture (`slot + 1` and `slot + 2`) with
    /// the snapshot of `slot + 2`.
    slot: i64,
    next: Arc<Vec<u8>>,
    task: Option<Task<Redraw>>,
}

/// A redraw made in the background: the texture, and its second snapshot (the first of the
/// next texture).
type Redraw = (Vec<u8>, Arc<Vec<u8>>);

/// Two snapshots side by side (each row: the first's, then the second's).
fn side_by_side(first: &[u8], second: &[u8]) -> Vec<u8> {
    let row = WIDTH * 4;
    first
        .chunks(row)
        .zip(second.chunks(row))
        .flat_map(|(a, b)| a.iter().chain(b).copied())
        .collect()
}

/// The sky map's image (linear RGBA8, two snapshots side by side: the shaders wrap the
/// longitudes in each).
fn sky_image(data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: 2 * WIDTH as u32,
            height: HEIGHT as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// The sky map, before the planet's materials (they sample it).
pub fn init_sky_map(
    mut commands: Commands,
    world: Res<PlanetWorld>,
    mut images: ResMut<Assets<Image>>,
) {
    let field = Arc::new(SkyField::new(
        &world.climate,
        &world.world,
        world.surface.seed() as u64,
    ));
    let (now, next) = (field.clouds(0.), field.clouds(REDRAW));
    let image = images.add(sky_image(side_by_side(&now, &next)));
    commands.insert_resource(SkyMap {
        image,
        cloud_radius: world.surface.radius * (1. + CLOUD_HEIGHT),
        field,
        slot: 0,
        next: Arc::new(next),
        task: None,
    });
}

/// The air, the cloud layer, and the camera's atmosphere settings.
pub fn spawn_sky(
    mut commands: Commands,
    world: Res<PlanetWorld>,
    sky: Res<SkyMap>,
    camera: Single<Entity, With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    mut clouds: ResMut<Assets<CloudMaterial>>,
) {
    let radius = world.surface.radius;
    // (Thicker than Earth's by this: thinner as much.)
    let stretch = THICKNESS * EARTH_RADIUS / EARTH_THICKNESS;
    let terms = air(&world.world).into_iter().map(|term| ScatteringTerm {
        absorption: Vec3::from(term.absorption),
        scattering: Vec3::from(term.scattering),
        falloff: match term.kind {
            AirKind::Gas { scale_height } | AirKind::Particles { scale_height, .. } => {
                Falloff::Exponential {
                    scale: scale_height,
                }
            }
            AirKind::Layer { center, width } => Falloff::Tent { center, width },
        },
        phase: match term.kind {
            AirKind::Gas { .. } => PhaseFunction::Rayleigh,
            AirKind::Particles { asymmetry, .. } => PhaseFunction::Mie { asymmetry },
            AirKind::Layer { .. } => PhaseFunction::Isotropic,
        },
    });
    let medium = ScatteringMedium::new(256, 256, terms)
        .with_label("planet_air")
        .with_density_multiplier(1. / stretch);
    let transform = Transform::from_scale(Vec3::splat(radius / EARTH_RADIUS));
    commands.spawn((
        Name::new("atmosphere"),
        Atmosphere {
            inner_radius: EARTH_RADIUS,
            outer_radius: EARTH_RADIUS * (1. + THICKNESS),
            ground_albedo: Vec3::splat(0.3),
            medium: media.add(medium),
        },
        transform,
        GlobalTransform::from(transform),
    ));
    commands.entity(*camera).insert(AtmosphereSettings {
        // (Planets seen from orbit: integrated along each ray, accurate at any distance.)
        rendering_method: AtmosphereMode::Raymarched,
        aerial_view_lut_max_distance: EARTH_RADIUS * 4.,
        ..default()
    });
    // The cloud layer, yellowed in hazy and dusty skies.
    let haze = world.world.haze();
    let dust = (world.world.atmosphere.dust - 0.1).max(0.);
    let tint = Vec3::ONE
        .lerp(Vec3::new(1.0, 0.82, 0.55), haze)
        .lerp(Vec3::new(1.0, 0.9, 0.75), dust.min(1.));
    commands.spawn((
        Name::new("clouds"),
        Mesh3d(meshes.add(Sphere::new(sky.cloud_radius).mesh().ico(7).unwrap())),
        MeshMaterial3d(clouds.add(CloudMaterial {
            layer: Vec4::new(sky.cloud_radius, 0., 0., 0.),
            tint: LinearRgba::new(tint.x, tint.y, tint.z, 1.),
            sky: sky.image.clone(),
        })),
        NotShadowCaster,
    ));
}

/// Redraw the clouds in the background a few times a second: at each new redraw slot, the
/// texture of this slot and the next goes up (made in the background during the last slot).
pub fn drift_clouds(time: Res<Time>, mut sky: ResMut<SkyMap>, mut images: ResMut<Assets<Image>>) {
    let slot = (time.elapsed_secs_wrapped() / REDRAW).floor() as i64;
    if slot == sky.slot {
        return;
    }
    let field = sky.field.clone();
    let at = |slot: i64| slot as f32 * REDRAW;
    let (data, next) = match sky.task.take() {
        // (Made in the background: wait for it if it isn't done.)
        Some(task) if slot == sky.slot + 1 => block_on(task),
        // (Skipped slots, or the clock wrapped round.)
        _ => {
            let (now, next) = (field.clouds(at(slot)), field.clouds(at(slot + 1)));
            (side_by_side(&now, &next), Arc::new(next))
        }
    };
    if let Some(mut image) = images.get_mut(&sky.image) {
        image.data = Some(data);
    }
    sky.slot = slot;
    sky.next = next.clone();
    let after = at(slot + 2);
    sky.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let last = Arc::new(field.clouds(after));
        (side_by_side(&next, &last), last)
    }));
}
