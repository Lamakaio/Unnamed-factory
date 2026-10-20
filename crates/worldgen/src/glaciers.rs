//! Glaciers and ice caps, from the climate: permanent ice where the warmest month stays below
//! `ICE_WARMEST` at the cell's height, gathering in valleys and on flat ground (ridges and
//! steep slopes stay bare), and glacier tongues flowing from it down the valleys
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
/// Valleys fill at most this deep (world units): the ice stays a flat floor in its valley.
const MAX_FILL: f32 = 1.5;
/// Ice gathers where the ground is this much below its mean over `VALLEY_RADIUS` blur passes
/// (valleys, cirques) or flatter than `FLAT_SLOPE`, never steeper than `MAX_SLOPE` (world
/// units per cell).
const VALLEY_RADIUS: usize = 5;
const VALLEY_DEPTH: f32 = 0.4;
const FLAT_SLOPE: f32 = 0.12;
const MAX_SLOPE: f32 = 0.8;
/// Where the warmest month stays below this (°C), the ice covers all but steep ground.
const SHEET_WARMEST: f32 = -3.;
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
    // Ice gathers in valleys (below the ground smoothed round them) and on flat ground (ice
    // sheets, plateaus); ridges, peaks and steep slopes stay bare rock.
    let slope: Vec<f32> = crate::par::map(cells, |i| {
        grid.neighbours(i).map(|(j, d)| (planet.heights[j] - planet.heights[i]).abs() / d).fold(0., f32::max)
    });
    let around = grid.blur(&planet.heights, VALLEY_RADIUS);
    // (Where it is coldest, ice sheets bury everything but the steepest ground.)
    let gathers = |i: usize| {
        slope[i] < MAX_SLOPE
            && (around[i] - planet.heights[i] > VALLEY_DEPTH || slope[i] < FLAT_SLOPE || warmest[i] < SHEET_WARMEST)
    };
    // (No ice on lakes: they stay lakes.)
    let lake = |i: usize| planet.lake_depth[i] > 0.02;
    let mut ice: Vec<bool> = (0..cells).map(|i| warmest[i] < ICE_WARMEST && gathers(i) && !lake(i)).collect();
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
            // (Tongues flow in valleys: not straight down a cone or a ridge.)
            let valley = around[next] - planet.heights[next] > VALLEY_DEPTH * 0.5;
            if planet.sea[next] || lake(next) || !valley || warmest[next] > TONGUE_WARMEST || slope[next] > MAX_SLOPE {
                break;
            }
            ice[next] = true;
            here = next;
        }
    }
    // Widen the tongues a little (glaciers fill their valley's floor).
    let widened: Vec<bool> = (0..cells)
        .map(|i| {
            ice[i]
                || !planet.sea[i]
                    && !lake(i)
                    && slope[i] < MAX_SLOPE
                    && grid.neighbours(i).filter(|&(j, d)| d < 1.2 && ice[j]).count() >= 2
        })
        .collect();
    // Close the small holes (no ice-free speckles in the sheets and valley floors).
    let mut ice = widened;
    for _ in 0..2 {
        let closed: Vec<bool> = (0..cells)
            .map(|i| ice[i] || !planet.sea[i] && !lake(i) && grid.neighbours(i).filter(|&(j, _)| ice[j]).count() >= 5)
            .collect();
        ice = closed;
    }
    let ice = ice;
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
            let fill = (smooth[i] - planet.heights[i]).clamp(0., MAX_FILL);
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
