//! Glaciers and ice caps, from the climate: permanent ice where the warmest month stays below
//! `ICE_WARMEST` at the cell's height, and glacier tongues flowing from it down the valleys
//! (the steepest way down) while the summers stay cool enough (`TONGUE_WARMEST`), up to
//! `TONGUE_LENGTH`. The ice is thickest where it fills valleys (its surface smoothed over the
//! ground), and ice sheets dome up away from their edges.

use std::collections::VecDeque;

use crate::{grid::Grid, planet::Planet, planet_climate::PlanetClimate};

/// Permanent ice where the warmest month is below this (°C at the cell's height); glacier
/// tongues reach down while it is below `TONGUE_WARMEST`, for at most `TONGUE_LENGTH` cells.
const ICE_WARMEST: f32 = 0.5;
const TONGUE_WARMEST: f32 = 8.;
const TONGUE_LENGTH: usize = 45;
/// Ice thickness (world units): at least `ICE_MIN`, plus the valleys filled up to the ground
/// smoothed over `SMOOTHING` blur passes, plus `DOME` per cell from the ice's edge (at most
/// `DOME_MAX`).
const ICE_MIN: f32 = 0.35;
const SMOOTHING: usize = 4;
const DOME: f32 = 0.06;
const DOME_MAX: f32 = 2.5;

/// The ice thickness over each cell (world units, 0 without ice).
pub fn glaciers(planet: &Planet, climate: &PlanetClimate) -> Vec<f32> {
    let grid = &planet.grid;
    let cells = grid.len();
    let warmest: Vec<f32> = crate::par::map(cells, |i| {
        if planet.sea[i] {
            return f32::MAX;
        }
        let (t, _) = climate.months_at(grid.direction(i), planet.heights[i]);
        t.iter().copied().fold(f32::MIN, f32::max)
    });
    let mut ice: Vec<bool> = (0..cells).map(|i| warmest[i] < ICE_WARMEST).collect();
    // Tongues: down from the ice's edge, the steepest way.
    let edge: Vec<usize> =
        (0..cells).filter(|&i| ice[i] && grid.neighbours(i).any(|(j, _)| !ice[j] && !planet.sea[j])).collect();
    for start in edge {
        let mut here = start;
        for _ in 0..TONGUE_LENGTH {
            let Some(next) = grid
                .neighbours(here)
                .map(|(j, _)| j)
                .filter(|&j| planet.heights[j] < planet.heights[here])
                .min_by(|&a, &b| planet.heights[a].total_cmp(&planet.heights[b]))
            else {
                break;
            };
            if planet.sea[next] || warmest[next] > TONGUE_WARMEST {
                break;
            }
            ice[next] = true;
            here = next;
        }
    }
    // Widen the tongues a little (glaciers fill their valley's floor).
    let widened: Vec<bool> = (0..cells)
        .map(|i| ice[i] || !planet.sea[i] && grid.neighbours(i).filter(|&(j, d)| d < 1.2 && ice[j]).count() >= 2)
        .collect();
    let ice = widened;
    // Thickness: the valleys filled up to the smoothed ground, domes away from the edge.
    let smooth = grid.blur(&planet.heights, SMOOTHING);
    let mut from_edge = vec![f32::INFINITY; cells];
    let mut queue: VecDeque<usize> =
        (0..cells).filter(|&i| ice[i] && grid.neighbours(i).any(|(j, _)| !ice[j])).collect();
    for &i in &queue {
        from_edge[i] = 0.;
    }
    while let Some(i) = queue.pop_front() {
        for (j, d) in grid.neighbours(i) {
            let next = from_edge[i] + d;
            if ice[j] && next < from_edge[j] && next * DOME < DOME_MAX {
                from_edge[j] = next;
                queue.push_back(j);
            }
        }
    }
    let thickness: Vec<f32> = (0..cells)
        .map(|i| {
            if !ice[i] {
                return 0.;
            }
            let fill = (smooth[i] - planet.heights[i]).max(0.);
            let dome = (from_edge[i].min(DOME_MAX / DOME)) * DOME;
            ICE_MIN + fill + dome
        })
        .collect();
    // (Softened: no step at the edge; none at sea.)
    let mut thickness = grid.blur(&thickness, 1);
    for (t, &sea) in thickness.iter_mut().zip(&planet.sea) {
        if sea {
            *t = 0.;
        }
    }
    thickness
}
