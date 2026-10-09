//! Stage D: fine surface detail. The eroded grid is upsampled (bicubic) to a finer grid and
//! gullies are added on slopes: stripes running down the gradient, branching octave after
//! octave (the "erosion noise" of Clay John / Fewes, as refined in runevision's erosion
//! filter). Gullies are scaled by the province ruggedness and faded on flat ground, near rivers
//! and around lakes.

use std::f32::consts::TAU;

use bevy_math::Vec2;

use crate::{
    WorldgenSettings,
    heightmap::Heightmap,
    noise::{hash, smoothstep},
};

/// Build the fine surface. `coarse` is the eroded terrain; `ruggedness` and `calm` (1 where
/// detail must vanish: rivers, lakes) are per coarse cell.
pub fn refine(
    settings: &WorldgenSettings,
    coarse: &Heightmap,
    ruggedness: &Heightmap,
    calm: &Heightmap,
) -> Heightmap {
    let factor = settings.detail_resolution.max(1);
    let size = coarse.size * factor;
    let cell = coarse.cell / factor as f32;
    let mut values = vec![0.; size * size];
    let seed = settings.seed as u32 ^ 0xD37A;

    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let rows_per_thread = size.div_ceil(threads);
    std::thread::scope(|scope| {
        for (chunk_index, chunk) in values.chunks_mut(rows_per_thread * size).enumerate() {
            scope.spawn(move || {
                for (k, value) in chunk.iter_mut().enumerate() {
                    let index = chunk_index * rows_per_thread * size + k;
                    let (x, z) = (index % size, index / size);
                    let p = (Vec2::new(x as f32, z as f32) - (size / 2) as f32) * cell;
                    *value = detailed_height(settings, coarse, ruggedness, calm, p, seed);
                }
            });
        }
    });
    Heightmap { size, cell, values }
}

fn detailed_height(
    settings: &WorldgenSettings,
    coarse: &Heightmap,
    ruggedness: &Heightmap,
    calm: &Heightmap,
    p: Vec2,
    seed: u32,
) -> f32 {
    let height = coarse.bicubic(p);
    if height <= 0. {
        return height;
    }
    let strength = ruggedness.bilinear(p) * (1. - calm.bilinear(p).clamp(0., 1.));
    if strength <= 0.01 {
        return height;
    }
    let e = coarse.cell * 0.5;
    let gradient = Vec2::new(
        coarse.bicubic(p + Vec2::X * e) - coarse.bicubic(p - Vec2::X * e),
        coarse.bicubic(p + Vec2::Y * e) - coarse.bicubic(p - Vec2::Y * e),
    ) / (2. * e);
    // No gullies on flat ground, where water doesn't run fast enough to carve them.
    let slope_mask = smoothstep(0.08, 0.5, gradient.length());
    if slope_mask <= 0. {
        return height;
    }
    height
        + gullies(p, gradient, seed, settings.detail_wavelength, settings.detail_octaves)
            * settings.detail_amplitude
            * strength
            * slope_mask
}

/// Gully offset (about -1..1) at `p` for a slope `gradient`. Each octave's stripes follow the
/// gradient including the previous octaves' gullies, so smaller gullies branch off larger ones.
pub fn gullies(p: Vec2, gradient: Vec2, seed: u32, wavelength: f32, octaves: u32) -> f32 {
    let mut total = 0.;
    let mut slope = gradient;
    let (mut amplitude, mut wavelength, mut weight) = (1., wavelength, 0.);
    for octave in 0..octaves {
        // Stripes vary across the slope, so they run downhill.
        let across = slope.normalize_or_zero().perp();
        let (value, derivative) = stripes(p / wavelength, across, seed.wrapping_add(octave));
        total += value * amplitude;
        weight += amplitude;
        // Feed the gully slope back (world units) so the next octave branches.
        slope += derivative * amplitude * TAU / wavelength;
        amplitude *= 0.5;
        wavelength *= 0.5;
    }
    total / weight
}

/// One octave: blend of cosine stripes around jittered pivots of the 3×3 neighbouring cells.
/// Returns the value and its derivative (in octave units).
fn stripes(p: Vec2, across: Vec2, seed: u32) -> (f32, Vec2) {
    let cell = p.floor();
    let f = p - cell;
    let (mut value, mut derivative, mut total) = (0., Vec2::ZERO, 0.);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let o = Vec2::new(dx as f32, dz as f32);
            let c = cell + o;
            let h = hash(seed, c.x as i32 as u32, c.y as i32 as u32);
            let pivot = Vec2::new((h & 0xFFFF) as f32, (h >> 16) as f32) / 65535.;
            let offset = f - o - pivot;
            let w = (-offset.length_squared() * 2.).exp();
            let phase = offset.dot(across) * TAU;
            value += phase.cos() * w;
            derivative += -phase.sin() * across * w;
            total += w;
        }
    }
    (value / total, derivative / total)
}
