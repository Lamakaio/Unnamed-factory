//! Plate tectonics over deep time (milestone 4 of `docs/living-world.md`): rigid plates
//! turning on the sphere, carrying crust that is created at diverging boundaries, consumed at
//! subduction zones, thickened by collisions, and occasionally torn apart (rifting). A
//! procedural model in the spirit of *Procedural Tectonic Planets* (Cortial et al. 2019) and
//! tectonics.js: not physics, but the processes that shape continents.
//!
//! Each plate keeps its crust in its own frame (a raster of the coarse cube-sphere that turns
//! with the plate), so crust is never resampled from cell to cell and nothing frays over the
//! steps. Each step, every world cell asks each plate what its raster holds there: the plates
//! whose crust covers it compete (subduction, collision), and a cell no plate covers is a gap
//! filled with new oceanic crust (a mid-ocean ridge). Crust remembers the uplift it received
//! and when (young ranges are sharp, old ones worn down) and when it formed (oceanic age).

use bevy_math::{Quat, Vec3};
use rand::{RngExt, SeedableRng, rngs::StdRng};

use crate::{grid::Grid, noise::Noise, par, sphere::SphereGrid};

/// Tectonic history parameters.
#[derive(Clone, Debug)]
pub struct TectonicSettings {
    pub seed: u64,
    /// Cells along a cube face edge of the crust grid.
    pub n: usize,
    /// Plates at the start, and the share of the surface that is continental crust.
    pub plates: usize,
    pub continental_share: f32,
    /// Steps of history, and each step's length (million years, for reports).
    pub steps: usize,
    pub step_myr: f32,
    /// Mean plate speed (radians per step), and a rift every this many steps on average.
    pub speed: f32,
    pub rift_interval: f32,
}

impl Default for TectonicSettings {
    fn default() -> Self {
        Self {
            seed: 1082,
            n: 96,
            plates: 10,
            continental_share: 0.35,
            steps: 50,
            step_myr: 4.,
            // ~1.5° per step: 4 cm/yr over 4 Myr on Earth.
            speed: 0.026,
            rift_interval: 12.,
        }
    }
}

/// A plate: its rotation per step (axis × angle).
#[derive(Clone, Copy, Debug)]
pub struct PlateMotion {
    pub rotation: Vec3,
}

/// What happened, for the history shown to the player.
#[derive(Clone, Debug)]
pub enum Event {
    /// A plate split in two (step, plate, new plate).
    Rift { step: usize, plate: u16, new: u16 },
}

/// The crust at the end of the history, in the world frame.
pub struct Tectonics {
    pub settings: TectonicSettings,
    pub grid: SphereGrid,
    pub plate: Vec<u16>,
    pub plates: Vec<PlateMotion>,
    /// Crust thickness: 0 oceanic, 1 continental, more where collisions thickened it.
    pub crust: Vec<f32>,
    /// Steps since oceanic crust formed (continental crust: 0).
    pub age: Vec<f32>,
    /// Uplift received (collisions, subduction, arcs), and steps since the last.
    pub orogeny: Vec<f32>,
    pub orogeny_age: Vec<f32>,
    pub events: Vec<Event>,
}

/// Uplift per step: continent–continent collision, a continent over subducting ocean (a
/// cordillera), an ocean plate over another (an island arc).
const COLLISION: f32 = 0.35;
const CORDILLERA: f32 = 0.2;
const ARC: f32 = 0.15;
/// Crust gained per step by an island arc (oceanic until 0.5).
const ARC_GROWTH: f32 = 0.04;
/// Crust thickening per collision step.
const THICKENING: f32 = 0.08;
/// Continental collision cells per step, for this many steps in a row, weld two plates.
const WELD_CELLS: usize = 25;
const WELD_STEPS: usize = 3;
/// "Never uplifted": a time long before the history.
const NEVER: f32 = -1000.;

/// A plate's crust, in its own frame.
#[derive(Clone)]
struct Raster {
    /// Accumulated rotation (plate frame to world).
    rotation: Quat,
    /// The plate's own cells (consumed crust stops being part of it).
    owned: Vec<bool>,
    crust: Vec<f32>,
    /// Step the crust formed at, its uplift and the step it last rose.
    born: Vec<f32>,
    orogeny: Vec<f32>,
    uplifted: Vec<f32>,
}

pub fn run(settings: &TectonicSettings) -> Tectonics {
    let grid = SphereGrid::new(settings.n);
    let cells = grid.len();
    let mut rng = StdRng::seed_from_u64(settings.seed ^ 0x7EC7);
    let noise = Noise::new(settings.seed as u32 ^ 0x7EC7);
    let directions: Vec<Vec3> = (0..cells).map(|i| grid.direction(i)).collect();

    // Initial plates: warped Voronoi regions, each turning about a random axis.
    let centers: Vec<Vec3> = (0..settings.plates).map(|_| random_direction(&mut rng)).collect();
    let warp = noise.layer(1);
    let mut plate: Vec<u16> = par::map(cells, |i| {
        let d = directions[i];
        let w = (d + warp.warp3(d * 2., 3, 0.25)).normalize();
        centers
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.dot(w).total_cmp(&b.1.dot(w)))
            .map_or(0, |(k, _)| k as u16)
    });
    let mut plates: Vec<PlateMotion> = (0..settings.plates)
        .map(|_| PlateMotion {
            rotation: random_direction(&mut rng) * settings.speed * rng.random_range(0.5..1.5),
        })
        .collect();
    // Initial continents: noise, cut at the continental share.
    let continents = noise.layer(2);
    let value: Vec<f32> = par::map(cells, |i| {
        let d = directions[i];
        continents.fbm3((d + continents.warp3(d * 2.2, 3, 0.15)) * 2.0, 5)
    });
    let mut sorted = value.clone();
    let k = ((1. - settings.continental_share) * (cells - 1) as f32) as usize;
    let cut = *sorted.select_nth_unstable_by(k, f32::total_cmp).1;
    let crust0: Vec<f32> = value.iter().map(|&v| if v >= cut { 1. } else { 0. }).collect();
    let born0: Vec<f32> = (0..cells).map(|_| -rng.random_range(0.0..30.0f32)).collect();
    let mut rasters: Vec<Raster> = (0..settings.plates)
        .map(|p| Raster {
            rotation: Quat::IDENTITY,
            owned: (0..cells).map(|i| plate[i] as usize == p).collect(),
            crust: crust0.clone(),
            born: born0.clone(),
            orogeny: vec![0.; cells],
            uplifted: vec![NEVER; cells],
        })
        .collect();
    let mut events = Vec::new();
    // Plate pairs colliding (steps in a row), and plates welded to another (moving with it).
    let mut welding: std::collections::HashMap<(u16, u16), usize> = Default::default();
    let mut welded: Vec<(u16, u16)> = Vec::new();

    for step in 0..settings.steps {
        let now = step as f32;
        // Rifting: the plate with the most continental crust splits along a great circle
        // through its middle, the halves drifting apart.
        if rng.random_range(0.0..1.0f32) < 1. / settings.rift_interval && plates.len() < 30 {
            let mut land = vec![0f32; plates.len()];
            let mut middle = vec![Vec3::ZERO; plates.len()];
            for i in 0..cells {
                let p = plate[i] as usize;
                let r = grid.cell_at(rasters[p].rotation.inverse() * directions[i]);
                land[p] += rasters[p].crust[r].min(1.);
                middle[p] += directions[i];
            }
            let p = (0..plates.len()).max_by(|&a, &b| land[a].total_cmp(&land[b])).unwrap();
            let center = middle[p].normalize_or_zero();
            if center != Vec3::ZERO {
                let normal = random_direction(&mut rng).reject_from(center).normalize();
                let new = plates.len();
                let mut split = rasters[p].clone();
                // (Sides are decided in the world frame; cells stay in the plate's frame.)
                for r in 0..cells {
                    let beyond = (rasters[p].rotation * directions[r]).dot(normal) > 0.;
                    split.owned[r] &= beyond;
                    rasters[p].owned[r] &= !beyond;
                }
                rasters.push(split);
                let away = center.cross(normal).normalize();
                let speed = settings.speed * rng.random_range(0.8..1.4);
                let base = plates[p].rotation * 0.5;
                plates[p].rotation = away * speed * 0.5 + base;
                plates.push(PlateMotion { rotation: -away * speed * 0.5 + base });
                events.push(Event::Rift { step, plate: p as u16, new: new as u16 });
            }
        }
        // Motions wander slowly (welded plates follow their partner); plates turn.
        for motion in plates.iter_mut() {
            let nudge = random_direction(&mut rng) * settings.speed * 0.15;
            let speed = motion.rotation.length().max(1e-6);
            motion.rotation = (motion.rotation + nudge).normalize() * speed;
        }
        for &(small, big) in &welded {
            plates[small as usize].rotation = plates[big as usize].rotation;
        }
        for (motion, raster) in plates.iter().zip(&mut rasters) {
            raster.rotation = (Quat::from_scaled_axis(motion.rotation) * raster.rotation).normalize();
        }

        // Each world cell: the plates whose crust covers it (and the cell of their raster).
        let inverse: Vec<Quat> = rasters.iter().map(|r| r.rotation.inverse()).collect();
        let claims: Vec<Vec<(u16, usize)>> = par::map(cells, |i| {
            inverse
                .iter()
                .enumerate()
                .filter_map(|(p, q)| {
                    let r = grid.cell_at(*q * directions[i]);
                    rasters[p].owned[r].then_some((p as u16, r))
                })
                .collect()
        });
        let mut owner = vec![u16::MAX; cells];
        // Continental collision cells between each pair of plates this step.
        let mut collisions: std::collections::HashMap<(u16, u16), usize> = Default::default();
        for i in 0..cells {
            let claims = &claims[i];
            if claims.is_empty() {
                continue;
            }
            // The winner: continental crust floats on oceanic; between oceanic crusts, the
            // younger (lighter) one.
            let key = |&(p, r): &(u16, usize)| (rasters[p as usize].crust[r], rasters[p as usize].born[r]);
            let &(winner, wr) = claims
                .iter()
                .max_by(|a, b| {
                    let (ka, kb) = (key(a), key(b));
                    ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
                })
                .unwrap();
            owner[i] = winner;
            let here = rasters[winner as usize].crust[wr] > 0.5;
            let (mut uplift, mut thickening) = (0f32, 0f32);
            for &(loser, lr) in claims.iter().filter(|c| c.0 != winner) {
                // Only where the plates converge: their boundaries also overlap by a cell
                // through rounding, which must neither consume nor raise anything. The loser
                // converges if it moves toward the winner's side of the boundary (from the
                // last step's plates around the cell).
                let d = directions[i];
                let across: Vec3 = grid
                    .neighbours(i)
                    .map(|(j, _)| {
                        let toward = directions[j] - d;
                        if plate[j] == winner {
                            toward
                        } else if plate[j] == loser {
                            -toward
                        } else {
                            Vec3::ZERO
                        }
                    })
                    .sum();
                let relative = plates[loser as usize].rotation.cross(d)
                    - plates[winner as usize].rotation.cross(d);
                if relative.dot(across.normalize_or_zero()) < settings.speed * 0.2 {
                    continue;
                }
                let there = rasters[loser as usize].crust[lr] > 0.5;
                if here && there {
                    *collisions.entry((winner.min(loser), winner.max(loser))).or_default() += 1;
                }
                uplift = uplift.max(match (here, there) {
                    (true, true) => {
                        thickening = THICKENING;
                        COLLISION
                    }
                    (true, false) => CORDILLERA,
                    (false, false) => {
                        // Arcs accrete: over many steps they become continental crust.
                        thickening = ARC_GROWTH;
                        ARC
                    }
                    (false, true) => 0.,
                });
                // Oceanic crust is consumed (subducted); continental crust doesn't sink: it
                // stacks under the winner (and stays part of its plate).
                if !there {
                    rasters[loser as usize].owned[lr] = false;
                }
            }
            if uplift > 0. {
                let raster = &mut rasters[winner as usize];
                raster.orogeny[wr] += uplift;
                raster.uplifted[wr] = now;
                raster.crust[wr] += thickening;
            }
        }
        // Continents that keep colliding weld together: their plates move as one.
        for (&(a, b), &count) in &collisions {
            let streak = welding.entry((a, b)).or_insert(0);
            if count >= WELD_CELLS {
                *streak += 1;
            }
            if *streak >= WELD_STEPS {
                let (big, small) = if rasters[a as usize].owned.iter().filter(|&&o| o).count()
                    >= rasters[b as usize].owned.iter().filter(|&&o| o).count()
                {
                    (a, b)
                } else {
                    (b, a)
                };
                plates[small as usize].rotation = plates[big as usize].rotation;
                welded.push((small, big));
            }
        }
        for &(small, big) in &welded {
            plates[small as usize].rotation = plates[big as usize].rotation;
        }
        welding.retain(|key, _| collisions.contains_key(key));
        // Gaps: new oceanic crust, joining a neighbouring plate (ridges grow from both sides),
        // written into that plate's frame.
        let mut frontier: Vec<usize> = (0..cells).filter(|&i| owner[i] != u16::MAX).collect();
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for &i in &frontier {
                for (j, _) in grid.neighbours(i) {
                    if owner[j] == u16::MAX {
                        let p = owner[i];
                        owner[j] = p;
                        let raster = &mut rasters[p as usize];
                        let r = grid.cell_at(inverse[p as usize] * directions[j]);
                        raster.owned[r] = true;
                        raster.crust[r] = 0.;
                        raster.born[r] = now;
                        raster.orogeny[r] = 0.;
                        raster.uplifted[r] = NEVER;
                        next.push(j);
                    }
                }
            }
            frontier = next;
        }
        plate = owner;
    }

    // The crust in the world frame.
    let end = settings.steps as f32;
    let sample = |i: usize| {
        let raster = &rasters[plate[i] as usize];
        (grid.cell_at(raster.rotation.inverse() * directions[i]), raster)
    };
    let crust = (0..cells).map(|i| sample(i).1.crust[sample(i).0]).collect();
    let age = (0..cells)
        .map(|i| {
            let (r, raster) = sample(i);
            if raster.crust[r] > 0.5 { 0. } else { end - raster.born[r] }
        })
        .collect();
    let orogeny = (0..cells).map(|i| sample(i).1.orogeny[sample(i).0]).collect();
    let orogeny_age = (0..cells).map(|i| end - sample(i).1.uplifted[sample(i).0]).collect();
    Tectonics {
        settings: settings.clone(),
        grid,
        plate,
        plates,
        crust,
        age,
        orogeny,
        orogeny_age,
        events,
    }
}

/// A uniformly random unit vector.
fn random_direction(rng: &mut StdRng) -> Vec3 {
    loop {
        let v = Vec3::new(
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
        );
        let length = v.length();
        if length > 0.1 && length <= 1. {
            return v / length;
        }
    }
}
