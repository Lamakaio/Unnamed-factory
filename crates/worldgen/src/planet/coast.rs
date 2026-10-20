//! From erosion units to the world: the heights scaled by the ranges' summits, plateaus cut at
//! their cap, the sea floor (shelf, slope, abyss, ridges, trenches) and the shores.

use super::{
    PlanetSettings,
    shape::{SHELF_BAND, Shaped},
};
use crate::{
    grid::Grid,
    noise::{Noise, smoothstep},
    par,
    sphere::SphereGrid,
    terrain,
};

/// Depth of the continental shelf (world units).
const SHELF_DEPTH: f32 = 1.5;

/// The heights in world units, and what later stages need: world units per erosion unit (for
/// the strata) and the blurred signed distance to the shore (cells, negative at sea).
pub(super) struct Coasts {
    pub heights: Vec<f32>,
    pub strata_scale: f32,
    pub shore: Vec<f32>,
}

pub(super) fn shape_coasts(
    settings: &PlanetSettings,
    grid: &SphereGrid,
    radius: f32,
    noise: Noise,
    shaped: &Shaped,
    mut heights: Vec<f32>,
) -> Coasts {
    let Shaped {
        shape,
        boundaries,
        crust,
        coast,
        sea,
        to_sea,
        ..
    } = shaped;
    let (coast, cells) = (*coast, grid.len());
    // World units: the ranges' summits set the scale; plateaus are cut at their cap level.
    let mut summits: Vec<f32> = (0..cells)
        .filter(|&i| !sea[i] && shape.range[i] > 0.5)
        .map(|i| heights[i])
        .collect();
    if summits.is_empty() {
        summits = heights.clone();
    }
    summits.sort_by(f32::total_cmp);
    let reference = summits[(summits.len() - 1) * 99 / 100].max(1e-6);
    let strata_scale = settings.max_height / reference;
    for i in 0..cells {
        if !sea[i] {
            heights[i] *= settings.max_height / reference;
        }
    }
    terrain::cap_plateaus(shape, &mut heights);
    // The sea floor: a shelf over continental crust, the continental slope where the crust
    // thins, the abyss over oceanic crust, raised along mid-ocean ridges, deepened in trenches.
    let floor = noise.layer(9);
    let shelf_floor: Vec<f32> = (0..cells)
        .map(|i| {
            let b = boundaries[i];
            let slope = smoothstep(coast - SHELF_BAND * 0.6, coast - SHELF_BAND * 1.6, crust[i]);
            let abyss = settings.sea_depth * (1. - 0.45 * b.ridge) * (1. + 0.4 * b.trench);
            -(SHELF_DEPTH + (abyss - SHELF_DEPTH) * slope)
        })
        .collect();
    let shelf_floor = grid.blur(&shelf_floor, 3);
    // Shores: from a blurred signed distance to
    // the shore (so the coast doesn't follow the cells' staircase), land rises gently from
    // the water line and the sea floor slopes down from it.
    let signed: Vec<f32> = (0..cells)
        .map(|i| if sea[i] { -to_sea[i] } else { to_sea[i] })
        .collect();
    let shore = grid.blur(&grid.blur(&signed, 2), 2);
    let variation = noise.layer(10);
    let heights = par::map(cells, |i| {
        let p = grid.direction(i) * radius;
        let d = shore[i];
        if sea[i] {
            let open = smoothstep(0., 6., -d);
            let floor_height =
                (-0.3 + (shelf_floor[i] + 0.3) * open) * (0.9 + 0.1 * floor.fbm3(p / 60., 3));
            floor_height.max(0.15 + 0.35 * (d - 0.5)).min(-0.05)
        } else {
            let slope =
                0.25 + 0.3 * (0.5 + 0.5 * variation.fbm3(p / 60., 3)) + 0.5 * shape.range[i];
            let ramp = 0.15 + slope * (d - 0.5);
            // (Land cells may end up just under water: the shore follows `shore`.)
            terrain::smooth_min(heights[i], ramp, 1.).max(-0.3)
        }
    });
    Coasts {
        heights,
        strata_scale,
        shore,
    }
}
