//! Impact craters: how many the planet keeps depends on how hard it was hit (the impact rate,
//! far higher early on: `World::impact_flux`) and how fast erosion wears them away. Young,
//! slowly eroding planets are cratered like the Moon; Earth keeps a few.
//!
//! Shapes after the classic morphology (Melosh, *Impact Cratering*, 1989): a bowl in a raised
//! rim, an ejecta blanket thinning outward; the bigger ones (complex craters) shallower for
//! their size with a flat floor and a central peak. Older craters are shallower, their rims
//! worn down and their floors filled. Sizes follow a power law (many small, few big).

use bevy_math::Vec3;

use crate::{
    grid::Grid,
    noise::{Noise, hash, smoothstep},
    sphere::SphereGrid,
    world::World,
};

/// A crater.
#[derive(Clone, Copy, Debug)]
pub struct Crater {
    /// Centre (unit direction), radius (world units).
    pub direction: Vec3,
    pub radius: f32,
    /// How fresh it is (1 just made .. 0 worn away).
    pub freshness: f32,
}

/// Craters per million cells per Gyr at today's impact rate (calibrated so that Earth keeps a
/// handful).
const CRATERS_PER_GYR: f32 = 2.;
/// At most this many craters per million cells (a saturated surface).
const SATURATION: f32 = 160.;
/// How long a crater lasts at Earth's erosion rate (Gyr: freshness e-folds over it).
const LIFETIME: f32 = 0.15;
/// Radius range (world units; about a cell each) and the cumulative size law's exponent
/// (N(> r) ∝ r^-2).
const RADIUS: (f32, f32) = (1.5, 22.);
const SIZE_EXPONENT: f32 = 2.;
/// Depth (world units) for a radius: `DEPTH × radius^0.5` (shallower for their size the
/// bigger they are); rim height and ejecta reach (shares of the depth, of the radius).
const DEPTH: f32 = 0.9;
const RIM: f32 = 0.3;
const EJECTA: f32 = 2.4;
/// Craters bigger than this (world units) are complex: a flat floor, a central peak.
const COMPLEX: f32 = 4.;

/// Where the craters are, their sizes and how fresh.
pub fn place(grid: &SphereGrid, seed: u64, world: &World) -> Vec<Crater> {
    let seed = seed as u32 ^ 0xC2A7;
    let random = |a: u32, b: u32| hash(seed, a, b) as f32 / u32::MAX as f32;
    // Impacts over the time craters last, in bins of age (how long ago).
    let lifetime = LIFETIME / world.ground.erosion.max(0.05);
    let window = (4. * lifetime).min(world.age);
    const BINS: usize = 32;
    let per_bin: Vec<f32> = (0..BINS)
        .map(|b| {
            let ago = (b as f32 + 0.5) / BINS as f32 * window;
            world.impact_flux(world.age - ago) * window / BINS as f32
        })
        .collect();
    let millions = grid.len() as f32 / 1e6;
    let total: f32 = per_bin.iter().sum::<f32>() * CRATERS_PER_GYR * millions;
    let count = total.min(SATURATION * millions).round() as u32;
    let sum: f32 = per_bin.iter().sum();
    let mut craters: Vec<Crater> = (0..count)
        .map(|k| {
            // An age drawn from the bins' impacts.
            let mut roll = random(k, 1) * sum;
            let bin = per_bin
                .iter()
                .position(|&n| {
                    roll -= n;
                    roll < 0.
                })
                .unwrap_or(BINS - 1);
            let ago = (bin as f32 + random(k, 2)) / BINS as f32 * window;
            let z = random(k, 3) * 2. - 1.;
            let angle = random(k, 4) * std::f32::consts::TAU;
            let ring = (1. - z * z).sqrt();
            Crater {
                direction: Vec3::new(ring * angle.cos(), z, ring * angle.sin()),
                radius: (RADIUS.0 * random(k, 5).max(1e-4).powf(-1. / SIZE_EXPONENT)).min(RADIUS.1),
                freshness: (-ago / lifetime).exp(),
            }
        })
        .filter(|c| c.freshness > 0.03)
        .collect();
    // (Oldest first: younger craters cut into older ones.)
    craters.sort_by(|a, b| a.freshness.total_cmp(&b.freshness));
    craters
}

/// Dig the craters into the heights (world units; sea level 0). Cells raised above the sea
/// become land.
pub fn build(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    craters: &[Crater],
    heights: &mut [f32],
    sea: &mut [bool],
) {
    let noise = Noise::new(seed as u32 ^ 0xC2A8);
    let mut seen = vec![0u32; grid.len()];
    for (k, crater) in craters.iter().enumerate() {
        let stamp = k as u32 + 1;
        let reach = crater.radius * EJECTA;
        let complex = smoothstep(COMPLEX * 0.7, COMPLEX * 1.3, crater.radius);
        // Worn: shallower, the rim lower, the floor filled.
        let fresh = crater.freshness;
        let depth = DEPTH * crater.radius.sqrt() * (0.25 + 0.75 * fresh);
        let rim = depth * RIM * fresh.sqrt();
        let floor = 0.15 + 0.4 * complex;
        let start = grid.cell_at(crater.direction);
        seen[start] = stamp;
        let mut queue = std::collections::VecDeque::from([start]);
        let layer = noise.layer(k as u32);
        while let Some(i) = queue.pop_front() {
            let d = grid.direction(i);
            let distance = d.distance(crater.direction) * radius;
            if distance > reach + 1.5 {
                continue;
            }
            for (j, _) in grid.neighbours(i) {
                if seen[j] != stamp {
                    seen[j] = stamp;
                    queue.push_back(j);
                }
            }
            // (A little irregular.)
            let r = distance / crater.radius
                * (1. + 0.08 * layer.fbm3(d * radius / (crater.radius * 0.6), 2));
            if r > EJECTA {
                continue;
            }
            let shape = if r < 1. {
                // The bowl: a flat floor rising to the rim.
                let wall = smoothstep(floor, 1., r);
                -depth * (1. - wall) + rim * wall.powi(3)
                    // The central peak of complex craters.
                    + complex * depth * 0.45 * fresh * smoothstep(0.18, 0., r)
            } else {
                // The ejecta blanket, thinning outward.
                rim * r.powi(-3) * smoothstep(EJECTA, EJECTA * 0.7, r)
            };
            heights[i] += shape;
            if heights[i] > 0. {
                sea[i] = false;
            }
        }
    }
}
