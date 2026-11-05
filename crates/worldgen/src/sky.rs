//! The sky above each place, from the climate and the world's parameters: how cloudy (and
//! with which clouds), how hazy, how dusty; and the clouds themselves at a moment, drifting
//! with the prevailing winds (`SkyField::clouds`). Also what the air is made of, for the
//! renderer's scattering (`air`).
//!
//! Clouds, as climatology describes them (annual means):
//! - where it rains, it is cloudy: rain bands at the thermal equator (the ITCZ), the
//!   westerlies' storm tracks; clear under the subtropical highs and over deserts;
//! - warm and wet: convective clouds (cumulus, thunderstorm towers) in clusters;
//!   cooler and wet: layered frontal clouds, stretched along the wind;
//! - stratocumulus decks over cold subtropical seas (off west coasts, over cold currents).
//!
//! Haze: humid haze where it is warm and wet, sea salt over windy seas; dust raised from dry,
//! windy land (more with the world's dust).
//!
//! An equirectangular map (longitude × latitude), so that the renderer samples it by
//! direction alike everywhere: the cloud layer, cloud shadows on the ground, the haze.

use std::f32::consts::{PI, TAU};

use bevy_math::Vec3;

use crate::{climate::PlanetClimate, noise::Noise, noise::smoothstep, world::World};

/// Size of the map (texels; longitude × latitude).
pub const WIDTH: usize = 1024;
pub const HEIGHT: usize = 512;
/// Annual precipitation (mm) from which it is cloudy at all, and fully.
const CLOUDY_RAIN: (f32, f32) = (150., 2500.);
/// Mean temperatures (°C) over which clouds turn from layered to convective.
const CONVECTIVE: (f32, f32) = (14., 25.);
/// Cover always there (scattered clouds), and added by the storm tracks' wind strength
/// (m/s, from `PlanetClimate::strength`) and by cold subtropical seas.
const BASE_COVER: f32 = 0.08;
const STORMS: (f32, f32) = (7., 12.);
const STORM_COVER: f32 = 0.25;
const STRATUS_COVER: f32 = 0.55;
/// Sea colder than its latitude's mean by this (°C) has stratocumulus decks, in the
/// subtropics (latitudes, degrees).
const STRATUS_COLD: (f32, f32) = (-1., -4.);
const STRATUS_LATITUDES: (f32, f32, f32, f32) = (10., 18., 32., 40.);
/// Dust: raised from land drier than the first annual rain (mm, fully below the second) by
/// winds stronger than these (m/s).
const DUSTY_RAIN: (f32, f32) = (400., 150.);
const DUSTY_WIND: (f32, f32) = (4., 9.);
/// Cloud systems' scale: noise frequency over the unit sphere (about this many systems round
/// a great circle / 2π), and how much more finely convective clouds break up.
const FREQUENCY: f32 = 9.;
const CONVECTIVE_FREQUENCY: f32 = 2.6;
/// How much layered clouds stretch along the wind (east–west): the noise varies faster
/// across it by 1 / (1 − this).
const STRETCH: f32 = 0.55;
/// Drift: radians per second of game time per m/s of eastward prevailing wind (clouds move
/// far faster than the real winds would carry them across a planet this size: to be seen),
/// and the period (s) over which each noise phase is used before it fades into the other
/// (the clouds form and dissolve as they drift; no endless shearing).
const DRIFT: f32 = 4e-4;
const PERIOD: f32 = 60.;
/// Share of the drift that is the latitude's mean wind (the rest local): smooth drift, no
/// shear between neighbouring texels.
const ZONAL_DRIFT: f32 = 0.8;

/// The sky above each texel of the map.
pub struct SkyField {
    /// Cloud cover (0..1: share of the sky), how much of it convective (0..1), humid haze
    /// (0..1), dust (0..1), eastward drift (radians per second).
    cover: Vec<f32>,
    convective: Vec<f32>,
    haze: Vec<f32>,
    dust: Vec<f32>,
    drift: Vec<f32>,
    noise: Noise,
    /// Quantiles of the cloud noise (a table of 64): the threshold giving each cover.
    quantiles: Vec<f32>,
}

/// The direction of a texel's centre (as longitude = atan2(z, x), latitude = asin(y)).
pub fn texel_direction(x: usize, y: usize) -> Vec3 {
    let longitude = (x as f32 + 0.5) / WIDTH as f32 * TAU - PI;
    let latitude = PI / 2. - (y as f32 + 0.5) / HEIGHT as f32 * PI;
    Vec3::new(
        latitude.cos() * longitude.cos(),
        latitude.sin(),
        latitude.cos() * longitude.sin(),
    )
}

impl SkyField {
    pub fn new(climate: &PlanetClimate, world: &World, seed: u64) -> Self {
        let texels = WIDTH * HEIGHT;
        let wind = climate.wind_field();
        // Annual means per texel: precipitation, temperature (at sea level), land, wind.
        let means: Vec<(f32, f32, f32, Vec3, f32)> = crate::par::map(texels, |i| {
            let d = texel_direction(i % WIDTH, i / WIDTH);
            let (t, p) = climate.months_at(d, 0.);
            let land = climate.land[climate.grid.cell_at(d)];
            let (prevailing, strength) = wind.at(d);
            (
                p.iter().sum(),
                t.iter().sum::<f32>() / 12.,
                land,
                prevailing,
                strength,
            )
        });
        // The mean sea temperature of each row (for the cold seas).
        let row_sea: Vec<f32> = (0..HEIGHT)
            .map(|y| {
                let (sum, count) = (0..WIDTH)
                    .map(|x| means[y * WIDTH + x])
                    .filter(|m| m.2 < 0.5)
                    .fold((0., 0.), |(s, c), m| (s + m.1, c + 1.));
                if count > 0. { sum / count } else { 0. }
            })
            .collect();
        let water = world.ocean.water.max(0.).powf(0.25);
        let dustiness = (world.atmosphere.dust / 0.1).clamp(0., 4.);
        let fields: Vec<[f32; 5]> = crate::par::map(texels, |i| {
            let (rain, t, land, prevailing, strength) = means[i];
            let d = texel_direction(i % WIDTH, i / WIDTH);
            let latitude = d.y.asin().to_degrees().abs();
            let wet = smoothstep(CLOUDY_RAIN.0, CLOUDY_RAIN.1, rain);
            let storms = smoothstep(STORMS.0, STORMS.1, strength) * wet.sqrt();
            let (a, b, c, e) = STRATUS_LATITUDES;
            let subtropics = smoothstep(a, b, latitude) * smoothstep(e, c, latitude);
            let cold = smoothstep(STRATUS_COLD.0, STRATUS_COLD.1, t - row_sea[i / WIDTH]);
            let stratus = cold * subtropics * (1. - land);
            let cover = ((BASE_COVER + 0.72 * wet + STORM_COVER * storms) * water
                + STRATUS_COVER * stratus)
                .clamp(0., 0.95);
            let convective = smoothstep(CONVECTIVE.0, CONVECTIVE.1, t) * (1. - stratus);
            let haze = (0.6 * wet * smoothstep(16., 27., t)
                + 0.25 * (1. - land) * smoothstep(7., 12., strength))
            .min(1.);
            let dust = (land
                * smoothstep(DUSTY_RAIN.0, DUSTY_RAIN.1, rain)
                * smoothstep(DUSTY_WIND.0, DUSTY_WIND.1, strength)
                * dustiness)
                .min(1.);
            // (Eastward wind: the prevailing wind on the local east.)
            let east = Vec3::Y.cross(d).normalize_or_zero();
            [cover, convective, haze, dust, DRIFT * prevailing.dot(east)]
        });
        // (Mostly each row's mean drift.)
        let row_drift: Vec<f32> = (0..HEIGHT)
            .map(|y| (0..WIDTH).map(|x| fields[y * WIDTH + x][4]).sum::<f32>() / WIDTH as f32)
            .collect();
        let drift: Vec<f32> = (0..texels)
            .map(|i| row_drift[i / WIDTH] * ZONAL_DRIFT + fields[i][4] * (1. - ZONAL_DRIFT))
            .collect();
        let noise = Noise::new(seed as u32 ^ 0xC10D);
        // The noise's distribution, for covers to come out as the share of the sky clouded.
        let mut samples: Vec<f32> = (0..4096)
            .map(|k| {
                let d = texel_direction((k * 37) % WIDTH, (k * 101) % HEIGHT);
                noise.fbm3(d * FREQUENCY, 4)
            })
            .collect();
        samples.sort_by(f32::total_cmp);
        let quantiles = (0..64)
            .map(|q| samples[q * (samples.len() - 1) / 63])
            .collect();
        Self {
            cover: fields.iter().map(|f| f[0]).collect(),
            convective: fields.iter().map(|f| f[1]).collect(),
            haze: fields.iter().map(|f| f[2]).collect(),
            dust: fields.iter().map(|f| f[3]).collect(),
            drift,
            noise,
            quantiles,
        }
    }

    /// The noise value above which a share `cover` of the sky is.
    fn threshold(&self, cover: f32) -> f32 {
        let q = (1. - cover).clamp(0., 1.) * 63.;
        let (k, f) = (q.floor() as usize, q.fract());
        let next = (k + 1).min(63);
        self.quantiles[k] + (self.quantiles[next] - self.quantiles[k]) * f
    }

    /// The map at `time` (seconds) as RGBA8 texels, row by row from the north: cloud density
    /// (0..1: above 0.5 is cloud), how convective, humid haze, dust.
    pub fn clouds(&self, time: f32) -> Vec<u8> {
        // Two noise phases, each drifting for a `PERIOD` and fading out as the other fades in.
        let phase = [(time / PERIOD).fract(), (time / PERIOD + 0.5).fract()];
        let weight = 1. - (2. * phase[0] - 1.).abs();
        let texels = WIDTH * HEIGHT;
        let to_u8 = |v: f32| (v.clamp(0., 1.) * 255. + 0.5) as u8;
        let rgba: Vec<[u8; 4]> = crate::par::map(texels, |i| {
            let d = texel_direction(i % WIDTH, i / WIDTH);
            let cover = self.cover[i];
            let convective = self.convective[i];
            let sample = |k: usize| {
                // Carried east by the wind since this phase began (back along the drift).
                let angle = -self.drift[i] * phase[k] * PERIOD;
                let (s, c) = angle.sin_cos();
                let p = Vec3::new(c * d.x - s * d.z, d.y, s * d.x + c * d.z)
                    + Vec3::splat(k as f32 * 1.73);
                // Layered clouds stretched along the wind (east–west: the noise varies faster
                // north–south), convective ones broken up.
                let layered = self
                    .noise
                    .fbm3(Vec3::new(p.x, p.y / (1. - STRETCH), p.z) * FREQUENCY, 4);
                let puffy = self
                    .noise
                    .layer(7)
                    .fbm3(p * FREQUENCY * CONVECTIVE_FREQUENCY, 3);
                layered + (puffy - layered) * convective * 0.7
            };
            let value = sample(0) * weight + sample(1) * (1. - weight);
            // (The blend of two phases is less contrasted than either: widen the threshold.)
            let contrast = 0.75 + 0.25 * (2. * weight - 1.).abs();
            let threshold = self.threshold(cover) * contrast;
            let density = 0.5 + (value - threshold) * 4.;
            [
                to_u8(if cover < 0.01 { 0. } else { density }),
                to_u8(convective),
                to_u8(self.haze[i]),
                to_u8(self.dust[i]),
            ]
        });
        rgba.into_iter().flatten().collect()
    }
}

/// One component of the air for the renderer's scattering: absorption and scattering per
/// meter (RGB) at the ground on an Earth-sized planet, its scale height (share of the
/// atmosphere's thickness), and how it scatters.
#[derive(Clone, Copy, Debug)]
pub struct AirTerm {
    pub absorption: [f32; 3],
    pub scattering: [f32; 3],
    pub kind: AirKind,
}

#[derive(Clone, Copy, Debug)]
pub enum AirKind {
    /// Molecules (Rayleigh: blue scattered most), thinning with this scale height.
    Gas { scale_height: f32 },
    /// Particles (Mie: forward scattering), thinning with this scale height.
    Particles { scale_height: f32, asymmetry: f32 },
    /// A layer (ozone) round this height with this width (shares of the thickness).
    Layer { center: f32, width: f32 },
}

/// What the world's air is made of, for the renderer: Earth's air (Bruneton and Neyret's
/// coefficients, as Bevy's Earth atmosphere), the gas thicker with a CO₂-rich atmosphere
/// (CO₂ scatters about 2.5× as much as air per molecule), aerosols with the dust, an organic
/// haze absorbing the blues (orange skies, as Titan's) with methane, ozone only once there is
/// oxygen.
pub fn air(world: &World) -> Vec<AirTerm> {
    let atmosphere = &world.atmosphere;
    let co2_bar = atmosphere.co2 / 1e6;
    let gas = 1. + 2.5 * co2_bar;
    let aerosols = 0.3 + 7. * atmosphere.dust;
    let haze = world.haze();
    // (Ozone forms from little oxygen: about Earth's layer from a tenth of today's oxygen.)
    let ozone = smoothstep(0., 0.02, atmosphere.o2);
    let scale = |v: [f32; 3], k: f32| v.map(|c| c * k);
    let mut terms = vec![
        AirTerm {
            absorption: [0.; 3],
            scattering: scale([5.802e-6, 13.558e-6, 33.1e-6], gas),
            kind: AirKind::Gas {
                scale_height: 8. / 60.,
            },
        },
        AirTerm {
            absorption: scale([3.996e-6; 3], aerosols),
            scattering: scale([0.444e-6; 3], aerosols),
            kind: AirKind::Particles {
                scale_height: 1.2 / 60.,
                asymmetry: 0.8,
            },
        },
    ];
    if ozone > 0. {
        terms.push(AirTerm {
            absorption: scale([0.65e-6, 1.881e-6, 0.085e-6], ozone),
            scattering: [0.; 3],
            kind: AirKind::Layer {
                center: 0.75,
                width: 0.3,
            },
        });
    }
    // Desert dust: yellowish, absorbing the blues.
    let dusty = (atmosphere.dust - 0.1).max(0.);
    if dusty > 0. {
        terms.push(AirTerm {
            absorption: scale([1.0e-6, 2.0e-6, 4.5e-6], dusty * 4.),
            scattering: scale([6.0e-6, 5.5e-6, 4.5e-6], dusty * 4.),
            kind: AirKind::Particles {
                scale_height: 2.5 / 60.,
                asymmetry: 0.7,
            },
        });
    }
    if haze > 0. {
        terms.push(AirTerm {
            absorption: scale([1.5e-6, 6.0e-6, 18.0e-6], haze),
            scattering: scale([8.0e-6, 7.0e-6, 5.0e-6], haze),
            kind: AirKind::Particles {
                scale_height: 12. / 60.,
                asymmetry: 0.6,
            },
        });
    }
    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texels_round_trip() {
        // Directions run west to east from −180°, north to south.
        let d = texel_direction(0, 0);
        assert!(d.z.atan2(d.x).to_degrees() < -179.);
        assert!(d.y > 0.99);
        let d = texel_direction(WIDTH / 2, HEIGHT / 2);
        assert!(d.z.atan2(d.x).abs() < 0.01 && d.y.abs() < 0.01);
    }
}
