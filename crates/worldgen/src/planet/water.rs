//! The planet's water on the shaped ground: lakes (`lakes`), rivers (`rivers`), floodplains
//! and wetlands, deltas and estuaries (`mouths`), the water settled again on the result, and
//! where fine detail must vanish (`calm`).

use super::Laps;
use crate::{
    grid::Grid,
    noise::{Noise, smoothstep},
    par,
    sphere::SphereGrid,
};

/// Drainage area (cells) from which fine detail fades around rivers.
const CALM_RIVER_AREA: f32 = 150.;
/// areas), below this height (world units).
const WET_SLOPE: f32 = 0.1;
const WET_DRAINAGE: f32 = 40.;
const WET_HEIGHT: f32 = 4.;

/// The planet's water (see `Planet`).
pub(super) struct Water {
    pub drainage: Vec<f32>,
    pub lake_depth: Vec<f32>,
    pub wetland: Vec<f32>,
    pub basins: Vec<f32>,
    pub rivers: Vec<crate::rivers::PlanetRiver>,
    pub calm: Vec<f32>,
}

pub(super) fn shape_water(
    grid: &SphereGrid,
    radius: f32,
    seed: u64,
    noise: Noise,
    shore: &[f32],
    heights: &mut [f32],
    sea: &mut [bool],
    laps: &mut Laps,
) -> Water {
    let cells = grid.len();
    // Lakes (real ones, with basins of their own), filled flats and dry basins below sea level.
    let crate::lakes::Hydrology {
        filled,
        drainage,
        lake_depth,
        base,
        wetland,
        basins,
    } = crate::lakes::shape_lakes(grid, heights, sea);
    laps.lap("  lakes");
    let (mut rivers, valley) =
        crate::rivers::extract(grid, radius, heights, &filled, &base, &lake_depth);
    // Floodplains: flat valley floors where rivers meander.
    laps.lap("  rivers");
    let floodplain = flatten_floodplains(grid, radius, heights, &lake_depth, &valley);
    // Wet ground: floodplains, and low flat ground along the larger streams (poorly drained
    // valley floors and plains).
    // (Slopes of the general lie of the land: on smoothed heights; near streams: their mask
    // spread a few cells.)
    let smooth = grid.blur(heights, 2);
    let streams: Vec<f32> = drainage
        .iter()
        .map(|&d| if d >= WET_DRAINAGE { 1. } else { 0. })
        .collect();
    let near_stream = grid.blur(&streams, 3);
    let wetland: Vec<f32> = par::map(cells, |i| {
        let (sum, count) = grid.neighbours(i).fold((0., 0.), |(sum, count), (j, d)| {
            (sum + (smooth[j] - smooth[i]).abs() / d, count + 1.)
        });
        let slope = sum / count;
        let flat_valley = smoothstep(WET_SLOPE, WET_SLOPE * 0.4, slope)
            * smoothstep(0.03, 0.15, near_stream[i])
            * smoothstep(WET_HEIGHT, WET_HEIGHT * 0.5, heights[i]);
        wetland[i].max(0.9 * floodplain[i]).max(0.8 * flat_valley)
    });
    let wetland = grid.blur(&wetland, 1);
    let mut wetland = wetland;
    laps.lap("  floodplains, wetlands");
    let sea_before_mouths = sea.to_vec();
    // Deltas and estuaries at the river mouths (deltas build land out into the sea).
    crate::mouths::shape_mouths(
        grid,
        radius,
        seed,
        noise.layer(11),
        shore,
        heights,
        sea,
        &mut wetland,
        &mut rivers,
    );
    // The floodplains and mouths reshaped the ground: settle the water again (the routing
    // base: the sea as it is now, and the dry basins' sinks).
    let base: Vec<bool> = (0..cells)
        .map(|i| sea[i] || base[i] && !sea_before_mouths[i])
        .collect();
    laps.lap("  mouths");
    let (drainage, lake_depth, flats) = crate::lakes::settle(grid, heights, &base, &lake_depth);
    laps.lap("  settle");
    for i in flats {
        wetland[i] = 1.;
    }
    // Rivers (cells draining enough, as drawn in the previews) and lakes, widened.
    let calm: Vec<f32> = (0..cells)
        .map(|i| {
            let river = smoothstep(CALM_RIVER_AREA * 0.5, CALM_RIVER_AREA, drainage[i]);
            let lake = smoothstep(0.02, 0.1, lake_depth[i]);
            // (Wetlands are flat: floodplains, closed flats, delta land.)
            river.max(lake).max(wetland[i]).max(basins[i])
        })
        .collect();
    let calm: Vec<f32> = grid
        .blur(&calm, 2)
        .iter()
        .map(|c| (c * 3.).min(1.))
        .collect();
    laps.lap("drainage");

    Water {
        drainage,
        lake_depth,
        wetland,
        basins,
        rivers,
        calm,
    }
}

/// Lake shores within this distance (world units) stay above their lake's level when
/// floodplains are flattened.
const LAKE_RIM: f32 = 3.;

/// Lower the valley floor to just above the water where rivers meander, so meanders have a flat floodplain to wind through; lake shores stay
/// above their lake. Returns how much each cell is floodplain (0..1).
fn flatten_floodplains(
    grid: &SphereGrid,
    radius: f32,
    heights: &mut [f32],
    lake_depth: &[f32],
    valley: &[crate::rivers::ValleyPoint],
) -> Vec<f32> {
    use crate::rivers::for_cells_within;
    let cells = grid.len();
    // Lowest height allowed per cell: the level of any lake nearby. (From the lakes' shore
    // cells: a lake's level is the same all over it.)
    let mut floor = vec![f32::MIN; cells];
    let lake = |i: usize| lake_depth[i] > 0.05;
    let shore = par::map(cells, |i| {
        lake(i) && grid.neighbours(i).any(|(j, _)| !lake(j))
    });
    for i in (0..cells).filter(|&i| shore[i]) {
        let level = heights[i] + lake_depth[i] + 0.05;
        for_cells_within(grid, radius, grid.direction(i), LAKE_RIM, |n, _| {
            floor[n] = floor[n].max(level);
        });
    }
    let mut plain = vec![0f32; cells];
    // (Every other point: they are half a cell apart, the floodplains several cells wide.)
    for point in valley.iter().step_by(2).filter(|p| p.floodplain > 0.) {
        let reach = point.floodplain * 1.5 + 2.;
        let target = point.level + 0.4;
        for_cells_within(grid, radius, point.direction, reach, |i, distance| {
            if lake_depth[i] > 0.05 || heights[i] <= target {
                return;
            }
            let t = smoothstep(reach * 0.4, reach, distance);
            let lowered = target + (heights[i] - target) * t;
            heights[i] = heights[i].min(lowered.max(floor[i]));
            plain[i] = plain[i].max(1. - t);
        });
    }
    plain
}
