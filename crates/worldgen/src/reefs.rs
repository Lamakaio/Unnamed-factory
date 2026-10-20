//! Coral reefs, in warm tropical seas (corals need sea water warm all year: within about
//! `TROPICS` of the equator), built on the shaped coasts and volcanoes:
//!
//! - **Fringing reefs** along the coasts: a reef rim a little offshore, just under the water,
//!   with a shallow lagoon between it and the shore; round volcanic islands old enough to sink a
//!   little, the rim stands further out (**barrier reefs**).
//! - **Atolls**: old volcanoes sunk under the sea keep their reef as a ring at the former
//!   coast: a flat lagoon inside, the reef flat just under the water with sandy islets (motu)
//!   where it breaks the surface, the old cone's slope falling to the deep outside.
//!
//! Returns how much each cell is reef (0..1), for the terrain shader's coral and the islets'
//! coral sand.

use std::collections::VecDeque;

use crate::{
    grid::Grid,
    noise::{Noise, smoothstep},
    sphere::SphereGrid,
    volcanoes::{Volcano, VolcanoKind},
};

/// Reefs grow within this latitude (degrees), fading over the last `TROPICS_FADE`.
const TROPICS: f32 = 26.;
const TROPICS_FADE: f32 = 6.;
/// Fringing reefs: the rim's distance from the shore (cells), its top and the lagoon floor
/// (world units under the sea), and how deep the sea may be for a reef to grow there.
const FRINGE: (f32, f32) = (1.5, 3.5);
const BARRIER: (f32, f32) = (4., 7.);
const REEF_TOP: f32 = -0.12;
const LAGOON_FLOOR: f32 = -0.7;
const REEF_MAX_DEPTH: f32 = 4.;
/// Atolls: on volcanoes sunk this much (age) or more, the ring at this share of the old cone's
/// radius, this wide (cells); the lagoon this deep.
const ATOLL_AGE: f32 = 0.5;
const ATOLL_RING: f32 = 0.3;
const ATOLL_WIDTH: f32 = 2.5;
const ATOLL_LAGOON: f32 = -1.4;
/// Barrier reefs round volcanic islands from this age.
const BARRIER_AGE: f32 = 0.25;

/// How tropical a direction is (0..1).
fn tropical(direction: bevy_math::Vec3) -> f32 {
    let latitude = direction.y.clamp(-1., 1.).asin().to_degrees().abs();
    smoothstep(TROPICS, TROPICS - TROPICS_FADE, latitude)
}

/// Build the reefs on the heights (sea cells; cells raised above the sea become land).
pub fn build(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    volcanoes: &[Volcano],
    heights: &mut [f32],
    sea: &mut [bool],
) -> Vec<f32> {
    let cells = grid.len();
    let noise = Noise::new(seed as u32 ^ 0xC0A1);
    let mut reef = vec![0f32; cells];

    // Atolls first (their lagoons are not coasts to fringe).
    for v in volcanoes {
        let centre = grid.cell_at(v.direction);
        if !sea[centre] || v.kind != VolcanoKind::Shield || v.age < ATOLL_AGE || tropical(v.direction) < 0.5 {
            continue;
        }
        let ring = v.radius * ATOLL_RING;
        let reach = ring + ATOLL_WIDTH * 3.;
        let mut seen = std::collections::HashSet::from([centre]);
        let mut queue = VecDeque::from([centre]);
        while let Some(i) = queue.pop_front() {
            let d = grid.direction(i);
            let r = d.distance(v.direction) * radius;
            if r > reach {
                continue;
            }
            for (j, _) in grid.neighbours(i) {
                if seen.insert(j) {
                    queue.push_back(j);
                }
            }
            let p = d * radius;
            // The ring, a little irregular; islets where it rises above the water.
            let wobble = 1.5 * noise.fbm3(p / 9., 2);
            let x = (r - ring + wobble) / ATOLL_WIDTH;
            let crest = (-x * x).exp();
            let islets = smoothstep(0.25, 0.6, noise.layer(1).fbm3(p / 4., 2));
            let top = REEF_TOP + 0.3 * islets;
            // Inside the ring, the flat lagoon floor; on it, the reef.
            let lagoon = smoothstep(-0.5, -1.5, x);
            let floor = heights[i] * (1. - lagoon) + ATOLL_LAGOON * lagoon;
            heights[i] = floor * (1. - crest) + top * crest;
            reef[i] = reef[i].max(crest);
            sea[i] = heights[i] <= 0.;
        }
    }

    // Fringing and barrier reefs: sea cells by their distance from the land (cells).
    let mut distance = vec![f32::INFINITY; cells];
    let mut queue: VecDeque<usize> = (0..cells).filter(|&i| !sea[i]).collect();
    for &i in &queue {
        distance[i] = 0.;
    }
    while let Some(i) = queue.pop_front() {
        for (j, d) in grid.neighbours(i) {
            let next = distance[i] + d;
            if sea[j] && next < distance[j] && next <= BARRIER.1 + 1. {
                distance[j] = next;
                queue.push_back(j);
            }
        }
    }
    // Volcanic islands old enough for a barrier reef.
    let barrier_near = |d: bevy_math::Vec3| {
        volcanoes.iter().any(|v| {
            v.age >= BARRIER_AGE && v.age < ATOLL_AGE && d.distance(v.direction) * radius < v.radius * 0.9
        })
    };
    // (Each cell on its own: on all cores.)
    let fringed = crate::par::map(cells, |i| {
        if !sea[i] || !distance[i].is_finite() || heights[i] < -REEF_MAX_DEPTH || reef[i] > 0. {
            return None;
        }
        let d = grid.direction(i);
        let warm = tropical(d);
        if warm <= 0. {
            return None;
        }
        let p = d * radius;
        // Not everywhere: reefs come in stretches.
        let stretch = smoothstep(-0.2, 0.2, noise.layer(2).fbm3(p / 60., 2)) * warm;
        if stretch <= 0. {
            return None;
        }
        let (near, far) = if barrier_near(d) { BARRIER } else { FRINGE };
        let rim = (distance[i] - (near + far) / 2.) / ((far - near) / 2.);
        let crest = (-rim * rim * 2.).exp() * stretch;
        let lagoon = smoothstep(0., -1., rim) * stretch;
        let target = REEF_TOP * crest + LAGOON_FLOOR.min(heights[i]) * lagoon * (1. - crest);
        let blend = crest.max(lagoon);
        Some((heights[i] * (1. - blend) + target * blend, crest))
    });
    for (i, fringe) in fringed.into_iter().enumerate() {
        if let Some((height, crest)) = fringe {
            heights[i] = height;
            reef[i] = reef[i].max(crest);
        }
    }
    reef
}
