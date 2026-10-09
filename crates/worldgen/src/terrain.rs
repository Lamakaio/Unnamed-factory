//! Stage A (macro shape: coastline, uplift, rock hardness) and stage B (stream-power erosion).

use bevy_math::Vec2;

use crate::{
    WorldgenSettings,
    flow::{FlowGraph, neighbours, priority_flood},
    noise::{Noise, hash, smoothstep},
};

/// Inputs of the erosion simulation, per cell.
pub struct Shape {
    /// 0 in the sea, 1 inland.
    pub land: Vec<f32>,
    /// Tectonic uplift rate (relative, 0..1).
    pub uplift: Vec<f32>,
    /// Erodibility (relative): low values are hard rock that forms plateaus and steep ranges.
    pub erodibility: Vec<f32>,
    /// Sea cells: fixed at sea level (0) during erosion; every river drains to them.
    pub base_level: Vec<bool>,
    /// Initial heights (a gentle land surface, 0 in the sea).
    pub heights: Vec<f32>,
    /// Sea floor depth (world units, negative), applied after erosion.
    pub sea_floor: Vec<f32>,
}

/// Stage A: coastline, mountain ranges and rock hardness from warped noise.
pub fn shape(settings: &WorldgenSettings, noise: Noise) -> Shape {
    let size = settings.size;
    let half = size as f32 / 2.;
    let n = size * size;
    let mut shape = Shape {
        land: vec![0.; n],
        uplift: vec![0.; n],
        erodibility: vec![1.; n],
        base_level: vec![false; n],
        heights: vec![0.; n],
        sea_floor: vec![0.; n],
    };
    let coast = noise.layer(1);
    let ranges = noise.layer(2);
    let regions = noise.layer(3);
    let rock = noise.layer(4);
    let surface = noise.layer(5);

    for i in 0..n {
        let (x, z) = (i % size, i / size);
        // Position relative to the continent center, in cells.
        let p = Vec2::new(x as f32 - half, z as f32 - half);
        let border = x.min(z).min(size - 1 - x).min(size - 1 - z) as f32;

        // Coastline: a warped disc with noisy edges, always sea near the borders.
        let q = p + coast.warp(p / 140., 3, 70.);
        let radial = 1. - q.length() / (half * settings.land_radius);
        let land = radial + coast.fbm(q / 110., 5) * 0.45;
        let land = smoothstep(0., 0.3, land) * smoothstep(4., 24., border);
        shape.land[i] = land;
        shape.base_level[i] = land < 0.02;

        // Mountain ranges where the region noise says so; hills elsewhere.
        let w = p + ranges.warp(p / 200., 3, 60.);
        let mountainous = smoothstep(
            -0.35,
            0.35,
            regions.fbm(w / 260., 3) + settings.mountainousness - 0.5,
        );
        let ridges = ranges.ridged(w / 170., 5);
        let hills = 0.5 + 0.5 * ranges.layer(9).fbm(w / 90., 4);
        // Plains: low, flat regions where rivers meander.
        let plains = smoothstep(
            0.1,
            0.5,
            regions.layer(7).fbm(w / 230., 3) - settings.mountainousness * 0.3,
        );
        let uplift = (0.12 + 0.25 * hills) * (1. - 0.8 * plains) + mountainous * ridges * 0.9;
        shape.uplift[i] = land.powf(1.5) * uplift;

        // Harder rock in patches.
        shape.erodibility[i] = (1. + 0.6 * rock.fbm(w / 80., 4)).clamp(0.35, 1.6);

        // Deeper away from the coast (`radial` keeps decreasing offshore).
        shape.sea_floor[i] = -settings.sea_depth * smoothstep(0., -0.25, radial.min(land));
        shape.heights[i] = if shape.base_level[i] {
            0.
        } else {
            land * (0.5 + surface.fbm(p / 40., 4) * 0.3)
        };
    }
    shape
}

/// Stage B: stream-power-law erosion with an implicit solver (Braun & Willett 2013) and hillslope
/// diffusion. Returns heights in the simulation's relative units (scaled later).
///
/// Every step: depressions are filled (sediment), flow is routed by steepest descent,
/// drainage area accumulated, then `dh/dt = U - K A^m S` is solved implicitly from the outlets
/// upstream, and `D ∇²h` smooths hillslopes.
pub fn erode(settings: &WorldgenSettings, shape: &Shape) -> Vec<f32> {
    let size = settings.size;
    let mut heights = shape.heights.clone();
    let dt = settings.erosion_dt;
    for step in 0..settings.erosion_steps {
        heights = priority_flood(size, &heights, &shape.base_level, 1e-4);
        let seed = hash(settings.seed as u32, step as u32, 0x51ED);
        let jitter = settings.routing_jitter;
        let graph = FlowGraph::with_weights(size, &heights, &shape.base_level, |i, j| {
            1. + jitter * (hash(seed, i as u32, j as u32) as f32 / u32::MAX as f32)
        });
        let area = graph.accumulate(|_| 1.);

        for &i in &graph.order {
            if shape.base_level[i] {
                continue;
            }
            let r = graph.receivers[i];
            let uplifted = heights[i] + shape.uplift[i] * dt;
            if r == i {
                heights[i] = uplifted;
                continue;
            }
            let f = settings.erodibility
                * shape.erodibility[i]
                * dt
                * area[i].powf(settings.area_exponent)
                / graph.receiver_distance[i];
            heights[i] = (uplifted + f * heights[r]) / (1. + f);
        }

        // Hillslope diffusion (explicit; stable while diffusion * dt <= 0.25).
        let k = (settings.diffusion * dt).min(0.24);
        let previous = heights.clone();
        for i in 0..heights.len() {
            if shape.base_level[i] {
                continue;
            }
            let mut sum = 0.;
            let mut count = 0.;
            for (j, distance) in neighbours(size, i) {
                if distance <= 1. {
                    sum += previous[j];
                    count += 1.;
                }
            }
            heights[i] += k * (sum - count * previous[i]);
        }
    }
    priority_flood(size, &heights, &shape.base_level, 1e-4)
}
