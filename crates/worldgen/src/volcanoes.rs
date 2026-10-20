//! Volcanoes: where plates make them, built on the eroded planet as young constructions.
//!
//! - **Arcs**: above subduction zones. Island arcs (an ocean plate overriding another) and
//!   continental arcs (young ranges along the coast, as the Andes): steep stratovolcanoes with
//!   a summit crater, spaced along the arc.
//! - **Hotspots**: mantle plumes fixed under the moving plates. The plate carries each volcano
//!   away as it grows a new one over the plume, so they come in chains, the youngest over the
//!   plume and older ones further along the plate's motion: broad shield volcanoes with a
//!   caldera, lower and more eroded the older (at sea, the old ones sink: seamounts).
//!
//! Volcanoes are basalt; active ones have fresh lava fields around them (lobes downhill),
//! which the game draws bare and dark.

use bevy_math::Vec3;

use crate::{
    grid::Grid,
    noise::{Noise, hash, smoothstep},
    sphere::SphereGrid,
};

/// The kind of a volcano.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolcanoKind {
    /// Steep cone with a crater (arcs).
    Strato,
    /// Broad dome with a caldera (hotspots).
    Shield,
}

/// A volcano.
#[derive(Clone, Copy, Debug)]
pub struct Volcano {
    pub kind: VolcanoKind,
    /// Summit (unit direction).
    pub direction: Vec3,
    /// Height of the construction above the ground under it, and its radius (world units).
    pub height: f32,
    pub radius: f32,
    /// 0 (active) .. 1 (old, eroded, sunk).
    pub age: f32,
}

impl Volcano {
    pub fn active(&self) -> bool {
        self.age < ACTIVE_AGE
    }
}

/// Volcanoes younger than this are active (lava fields).
const ACTIVE_AGE: f32 = 0.15;
/// Arc volcanoes: spacing along the arc (cells), height (world units) and radius per height.
const ARC_SPACING: f32 = 80.;
const STRATO_HEIGHT: (f32, f32) = (9., 18.);
const STRATO_RADIUS: f32 = 2.6;
/// Hotspots: plumes per million cells, volcanoes per chain, spacing along it (cells), height
/// and radius per height.
const HOTSPOTS_PER_MILLION: f32 = 2.5;
const CHAIN: (usize, usize) = (4, 8);
const CHAIN_SPACING: (f32, f32) = (28., 45.);
const SHIELD_HEIGHT: (f32, f32) = (10., 16.);
const SHIELD_RADIUS: f32 = 5.;
/// How deep old volcanoes sink at sea (world units, at age 1).
const SUBSIDENCE: f32 = 9.;
/// Share of the hotspots under the oceans.
const OCEANIC_HOTSPOTS: f32 = 0.8;

/// Where volcanoes are. `arc(i)` is how much a cell is on an island arc (0..1), `coastal_belt`
/// a young range near the coast; `velocity(direction)` the surface velocity of the plate there
/// (its direction only matters).
pub fn place(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    arc: impl Fn(usize) -> f32,
    coastal_belt: impl Fn(usize) -> f32,
    velocity: impl Fn(Vec3) -> Vec3,
    sea: impl Fn(usize) -> bool,
) -> Vec<Volcano> {
    let seed = seed as u32 ^ 0x701C;
    let random = |a: u32, b: u32| hash(seed, a, b) as f32 / u32::MAX as f32;
    let mut volcanoes: Vec<Volcano> = Vec::new();
    let far_from_all = |volcanoes: &[Volcano], d: Vec3, spacing: f32| {
        volcanoes.iter().all(|v| v.direction.distance(d) * radius > spacing)
    };
    // Arcs: candidate cells in a random order, kept when far enough from the others.
    let mut candidates: Vec<(u32, usize)> = (0..grid.len())
        .filter_map(|i| {
            let weight = arc(i).max(coastal_belt(i));
            (weight > 0.5 && random(i as u32, 1) < 0.004 * weight).then(|| (hash(seed, i as u32, 2), i))
        })
        .collect();
    candidates.sort_unstable();
    for (_, i) in candidates {
        let d = grid.direction(i);
        let spacing = ARC_SPACING * (0.7 + 0.6 * random(i as u32, 3));
        if !far_from_all(&volcanoes, d, spacing) {
            continue;
        }
        let height = STRATO_HEIGHT.0 + (STRATO_HEIGHT.1 - STRATO_HEIGHT.0) * random(i as u32, 4);
        volcanoes.push(Volcano {
            kind: VolcanoKind::Strato,
            direction: d,
            height,
            radius: height * STRATO_RADIUS,
            age: random(i as u32, 5).powf(1.5),
        });
    }
    // Hotspots: chains along the plate's motion, oldest furthest downstream.
    let count = (HOTSPOTS_PER_MILLION * grid.len() as f32 / 1e6).round() as u32;
    for k in 0..count {
        // Mostly under the oceans (as on Earth), now and then under a continent.
        let oceanic = random(k, 13) < OCEANIC_HOTSPOTS;
        let i = (0..32)
            .map(|t| (hash(seed, k, 10 + 1000 * t) as usize) % grid.len())
            .find(|&i| sea(i) == oceanic)
            .unwrap_or((hash(seed, k, 10) as usize) % grid.len());
        let plume = grid.direction(i);
        let motion = velocity(plume).reject_from(plume).normalize_or_zero();
        if motion == Vec3::ZERO {
            continue;
        }
        let length = CHAIN.0 + (hash(seed, k, 11) as usize) % (CHAIN.1 - CHAIN.0 + 1);
        let spacing = CHAIN_SPACING.0 + (CHAIN_SPACING.1 - CHAIN_SPACING.0) * random(k, 12);
        let mut here = plume;
        for step in 0..length {
            // (From new over the plume to old and sunk at the far end.)
            let age = step as f32 / (length - 1) as f32;
            let height = (SHIELD_HEIGHT.0 + (SHIELD_HEIGHT.1 - SHIELD_HEIGHT.0) * random(k, 20 + step as u32))
                * (1. - 0.35 * age);
            if far_from_all(&volcanoes, here, spacing * 0.6) {
                volcanoes.push(Volcano {
                    kind: VolcanoKind::Shield,
                    direction: here,
                    height,
                    radius: height * SHIELD_RADIUS,
                    age,
                });
            }
            // Downstream, with a little wander.
            let side = here.cross(motion).normalize_or_zero();
            let wander = (random(k, 40 + step as u32) - 0.5) * 0.5;
            here = (here + (motion + side * wander) * spacing / radius).normalize();
        }
    }
    volcanoes
}

/// What the volcanoes leave besides their shape: volcanic ground (basalt), fresh lava (0..1)
/// and the ruggedness of their slopes (gullies on the older ones).
pub struct Built {
    pub volcanic: Vec<bool>,
    pub lava: Vec<f32>,
    pub ruggedness: Vec<f32>,
}

/// Build the volcanoes on the heights (world units; sea level 0). On land a cone sits on the
/// ground; at sea it rises from the sea floor to its summit above the water (old ones sink:
/// islands worn low, then seamounts). Cells raised above the sea become land.
pub fn build(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    volcanoes: &[Volcano],
    heights: &mut [f32],
    sea: &mut [bool],
) -> Built {
    let cells = grid.len();
    let mut built = Built {
        volcanic: vec![false; cells],
        lava: vec![0.; cells],
        ruggedness: vec![0.; cells],
    };
    let noise = Noise::new(seed as u32 ^ 0x1A7A);
    let mut seen = vec![0u32; cells];
    for (k, v) in volcanoes.iter().enumerate() {
        let start = grid.cell_at(v.direction);
        // (Cells seen for this volcano: stamped with its number.)
        let stamp = k as u32 + 1;
        seen[start] = stamp;
        let mut queue = std::collections::VecDeque::from([start]);
        let reach = v.radius * 1.3;
        let at_sea = sea[start];
        // Old volcanoes are lower and rounder; at sea, the summit's height above the water
        // (sinking with age) over the floor round it.
        let height = v.height * (1. - 0.4 * v.age);
        let floor = heights[start];
        let summit = height - SUBSIDENCE * v.age;
        while let Some(i) = queue.pop_front() {
            let d = grid.direction(i);
            let r = d.distance(v.direction) * radius;
            if r > reach + 1.5 {
                continue;
            }
            for (j, _) in grid.neighbours(i) {
                if seen[j] != stamp {
                    seen[j] = stamp;
                    queue.push_back(j);
                }
            }
            if r > reach {
                continue;
            }
            // A little irregular: lobes round the cone.
            let wobble = 1. + 0.18 * noise.layer(k as u32).fbm3(d * radius / (v.radius * 0.6), 2);
            let x = (r / (v.radius * wobble)).min(1.3);
            let profile = match v.kind {
                // Concave cone: steep near the summit, flaring out.
                VolcanoKind::Strato => (1. - x).max(0.).powf(1.7),
                // Broad dome.
                VolcanoKind::Shield => (1. - x * x).max(0.).powf(1.4),
            };
            // The summit crater (strato) or caldera (shield), shallower on old ones.
            let (crater_radius, crater_depth) = match v.kind {
                VolcanoKind::Strato => (0.1, 0.12),
                VolcanoKind::Shield => (0.16, 0.08),
            };
            let crater = smoothstep(crater_radius, crater_radius * 0.6, x) * crater_depth * (1. - 0.7 * v.age);
            // (The crater by the volcano's own height: at sea, not by its rise from the floor.)
            let built_height = if at_sea {
                floor + (summit - floor) * profile - height * crater
            } else {
                heights[i] + height * (profile - crater)
            };
            if built_height > heights[i] {
                heights[i] = built_height;
                if built_height > 0. {
                    sea[i] = false;
                }
            }
            if x < 1. {
                built.volcanic[i] = true;
                built.ruggedness[i] = built.ruggedness[i].max((0.25 + 0.6 * v.age) * smoothstep(1., 0.6, x));
            }
            // Fresh lava: lobes down from the summit (a noise of the angle round it), on active
            // volcanoes.
            if v.active() && x < 1.1 {
                let side = d.cross(v.direction).normalize_or_zero();
                let lobes = 0.5 + 0.5 * noise.layer(100 + k as u32).fbm3(side * 3. + v.direction * 7., 2);
                let reach_here = 0.35 + 0.75 * lobes;
                let lava = smoothstep(reach_here, reach_here - 0.15, x);
                built.lava[i] = built.lava[i].max(lava);
            }
        }
    }
    built
}
