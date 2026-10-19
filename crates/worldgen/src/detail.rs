//! Fine surface detail: gullies on slopes, stripes running down the gradient, branching octave
//! after octave (the "erosion noise" of Clay John / Fewes, as refined in runevision's erosion
//! filter). The planet's surface (`surface.rs`) adds them on rugged land.

use std::f32::consts::TAU;

use bevy_math::Vec2;

use crate::noise::hash;

/// Gully offset (about -1..1) at `p` for a slope `gradient`. Each octave's stripes follow the
/// gradient including the previous octaves' gullies, so smaller gullies branch off larger ones.
pub(crate) fn gullies(p: Vec2, gradient: Vec2, seed: u32, wavelength: f32, octaves: u32) -> f32 {
    let mut total = 0.;
    let mut slope = gradient;
    let (mut amplitude, mut wavelength, mut weight) = (1., wavelength, 0.);
    for octave in 0..octaves {
        // Each octave's lattice is rotated, so the cells don't line up into a visible grid.
        let rotation = Vec2::from_angle(octave as f32 * 2.4 + 0.6);
        // Stripes vary across the slope, so they run downhill.
        let across = rotation.rotate(slope.normalize_or_zero().perp());
        let (value, derivative) = stripes(
            rotation.rotate(p) / wavelength,
            across,
            seed.wrapping_add(octave),
        );
        total += value * amplitude;
        weight += amplitude;
        // Feed the gully slope back (world units) so the next octave branches.
        let derivative = Vec2::new(rotation.x, -rotation.y).rotate(derivative);
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
            // Random weight per pivot, so some gullies dominate their neighbours.
            let strength = 0.4 + 0.6 * (hash(h, 7, 11) as f32 / u32::MAX as f32);
            let w = (-offset.length_squared() * 2.).exp() * strength;
            let phase = offset.dot(across) * TAU;
            value += phase.cos() * w;
            derivative += -phase.sin() * across * w;
            total += w;
        }
    }
    (value / total, derivative / total)
}
