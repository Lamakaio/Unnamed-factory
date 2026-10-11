//! Lakes, read from the eroded terrain: its basins (rift floors, glacial troughs, kettle
//! hollows, see `terrain`) filled to their spill point with Priority-Flood. Basins too small or
//! too shallow to hold a lake are filled in (sediment). Where a basin's inflow can't make up for
//! what its surface evaporates (dry all around, little inflow, and a deep basin or one near sea
//! level), the lake has no outlet (endorheic): it shrinks to the size its inflow sustains,
//! leaving a salt flat on the dry bed around it.

use bevy_math::UVec2;

use crate::{
    Lake,
    flow::{FlowGraph, neighbours, priority_flood},
};

/// Flooding depth from which a cell belongs to a lake: the shallow fringe is part of it,
/// otherwise a shallow basin falls apart into several lakes at the same level.
const MIN_FLOOD: f32 = 0.01;
/// Depth a lake must reach somewhere (world units).
const MIN_DEPTH: f32 = 0.4;
/// Water a lake cell evaporates, in cells of mean rain, where it rains `DRY_RAIN` (more where
/// drier, none where it rains twice that): lakes whose inflow is less are endorheic.
const EVAPORATION: f32 = 1.5;
const DRY_RAIN: f32 = 0.6;
/// Endorheic lakes also need an inflow below this (rain units), and a basin at least this deep
/// below its spill point, or a spill point at most this high (world units).
const MAX_ENDORHEIC_INFLOW: f32 = 300.;
const ENDORHEIC_BARRIER: f32 = 3.;
const ENDORHEIC_SEA_LEVEL: f32 = 1.;
/// Cells around a lake that must all be dry for it to lose its outlet.
const DRY_RADIUS: i64 = 4;
/// Smallest endorheic lake kept (cells); below, the basin is only a salt flat.
const MIN_ENDORHEIC_CELLS: usize = 8;

/// The lakes of `heights` (world units), each cell's lake, and the salt flats (dry beds of
/// endorheic lakes). Fills basins that don't hold a lake into `heights`. `rain` is per cell
/// (mean ~1 on land).
pub fn find_lakes(
    size: usize,
    heights: &mut [f32],
    base_level: &[bool],
    rain: &[f32],
    min_cells: usize,
) -> (Vec<Lake>, Vec<Option<u16>>, Vec<bool>) {
    let water = priority_flood(size, heights, base_level, 0.);
    let flooded: Vec<bool> = (0..heights.len())
        .map(|i| !base_level[i] && water[i] - heights[i] > MIN_FLOOD)
        .collect();

    // Connected flooded regions; those too small or shallow are filled.
    let mut regions: Vec<Vec<usize>> = Vec::new();
    let mut seen = vec![false; heights.len()];
    for start in 0..heights.len() {
        if !flooded[start] || seen[start] {
            continue;
        }
        let mut cells = vec![start];
        seen[start] = true;
        let mut k = 0;
        while k < cells.len() {
            let i = cells[k];
            k += 1;
            for (n, _) in neighbours(size, i) {
                if flooded[n] && !seen[n] {
                    seen[n] = true;
                    cells.push(n);
                }
            }
        }
        let deepest = cells.iter().map(|&i| water[i] - heights[i]).fold(0., f32::max);
        if cells.len() < min_cells || deepest < MIN_DEPTH {
            for &i in &cells {
                heights[i] = water[i];
            }
        } else {
            regions.push(cells);
        }
    }

    // Inflow of each basin: the drainage reaching its lowest outflow (in rain units).
    let routing = priority_flood(size, heights, base_level, 1e-4);
    let graph = FlowGraph::new(size, &routing, base_level);
    let area = graph.accumulate(|i| rain[i]);

    let mut lake_of = vec![None; heights.len()];
    let mut salt = vec![false; heights.len()];
    let mut lakes = Vec::new();
    for cells in regions {
        let spill = water[cells[0]];
        let inflow = cells.iter().map(|&i| area[i]).fold(0., f32::max);
        // Only where it is dry all around: the wettest cell near the lake decides.
        let mut wettest: f32 = 0.;
        for &i in &cells {
            let (x, z) = ((i % size) as i64, (i / size) as i64);
            for dz in -DRY_RADIUS..=DRY_RADIUS {
                for dx in -DRY_RADIUS..=DRY_RADIUS {
                    let (nx, nz) = (x + dx, z + dz);
                    if nx >= 0 && nz >= 0 && nx < size as i64 && nz < size as i64 {
                        let n = nz as usize * size + nx as usize;
                        if !base_level[n] {
                            wettest = wettest.max(rain[n]);
                        }
                    }
                }
            }
        }
        let evaporation = EVAPORATION * (2. - wettest / DRY_RAIN).max(0.);
        let sustained = (inflow / evaporation.max(1e-6)).min(usize::MAX as f32 / 2.) as usize;
        // Closed basins also need little inflow, and either a high barrier to their outlet (a
        // deep basin) or to lie near sea level; elsewhere the water finds its way out.
        let bed = cells.iter().map(|&i| heights[i]).fold(f32::MAX, f32::min);
        let closed = inflow < MAX_ENDORHEIC_INFLOW
            && (spill - bed >= ENDORHEIC_BARRIER || spill <= ENDORHEIC_SEA_LEVEL);
        let (level, lake_cells, endorheic) = if sustained >= cells.len() || !closed {
            (spill, cells, false)
        } else {
            // Endorheic: the water stands as high as it covers the cells it can sustain.
            let mut beds: Vec<f32> = cells.iter().map(|&i| heights[i]).collect();
            beds.sort_by(f32::total_cmp);
            if sustained < MIN_ENDORHEIC_CELLS {
                for &i in &cells {
                    salt[i] = true;
                }
                continue;
            }
            let level = beds[sustained - 1] + MIN_FLOOD * 2.;
            let (wet, dry): (Vec<usize>, Vec<usize>) =
                cells.iter().partition(|&&i| heights[i] < level - MIN_FLOOD);
            for i in dry {
                salt[i] = true;
            }
            (level, wet, true)
        };
        if lake_cells.is_empty() {
            continue;
        }
        let id = lakes.len() as u16;
        for &i in &lake_cells {
            lake_of[i] = Some(id);
        }
        lakes.push(Lake {
            level,
            cells: lake_cells
                .iter()
                .map(|&i| UVec2::new((i % size) as u32, (i / size) as u32))
                .collect(),
            endorheic,
        });
    }
    (lakes, lake_of, salt)
}
