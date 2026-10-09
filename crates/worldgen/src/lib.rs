//! Continent generation, independent of the game engine.
//!
//! Pipeline ([`generate`]):
//! 1. **Shape** (A): warped coastline, uplift (mountain ranges / hills) and rock hardness.
//! 2. **Erosion** (B): stream-power law with an implicit solver plus hillslope diffusion, which
//!    carves dendritic valley networks; heights are then scaled to world units.
//! 3. **Lakes**: valley sites dammed at a level, found back with Priority-Flood.
//! 4. **Rivers** (C): cells draining a large area, traced into smoothed, meandering polylines
//!    with a width and depth from their discharge; floodplains are flattened along them.
//!
//! Coordinates: cell `(x, z)` is at world `((x - size/2) * cell_size, (z - size/2) * cell_size)`
//! relative to the continent center. Heights are world units, sea level is 0.

mod flow;
mod lakes;
pub mod noise;
mod rivers;
mod terrain;

use std::time::{Duration, Instant};

use bevy_math::{UVec2, Vec2, Vec3};
use rand::{SeedableRng, rngs::StdRng};

/// Generation parameters. The defaults give a 512×512 unit continent.
#[derive(Clone, Debug)]
pub struct WorldgenSettings {
    pub seed: u64,
    /// Cells per side.
    pub size: usize,
    /// Cell size, in world units.
    pub cell_size: f32,
    /// Radius of the land mass, as a fraction of the half size.
    pub land_radius: f32,
    /// 0..1: how much of the land is mountain ranges.
    pub mountainousness: f32,
    /// Height of the highest summit, in world units.
    pub max_height: f32,
    /// Depth of the open sea, in world units.
    pub sea_depth: f32,
    pub erosion_steps: usize,
    pub erosion_dt: f32,
    /// Stream power erodibility K.
    pub erodibility: f32,
    /// Stream power area exponent m (slope exponent n = 1).
    pub area_exponent: f32,
    /// Hillslope diffusion D.
    pub diffusion: f32,
    /// Random weight (0..) on slopes when routing flow during erosion, against grid artifacts.
    pub routing_jitter: f32,
    pub lake_count: usize,
    /// Drainage area (cells) a lake site needs.
    pub lake_min_area: f32,
    /// Minimum distance between lakes, in cells.
    pub lake_spacing: f32,
    /// Drainage area (cells) where rivers start.
    pub river_min_area: f32,
    /// Width of a river where it starts, in cells (grows with the square root of discharge).
    pub river_width: f32,
    /// Depth of a river where it starts, in world units.
    pub river_depth: f32,
    /// Meander amplitude, in channel widths.
    pub meander_amplitude: f32,
    /// Valley slope above which rivers don't meander.
    pub meander_max_slope: f32,
}

impl Default for WorldgenSettings {
    fn default() -> Self {
        Self {
            seed: 1082,
            size: 512,
            cell_size: 1.,
            land_radius: 0.8,
            mountainousness: 0.5,
            max_height: 60.,
            sea_depth: 20.,
            erosion_steps: 60,
            erosion_dt: 2.,
            erodibility: 0.25,
            area_exponent: 0.45,
            diffusion: 0.05,
            routing_jitter: 1.,
            lake_count: 5,
            lake_min_area: 300.,
            lake_spacing: 60.,
            river_min_area: 400.,
            river_width: 0.5,
            river_depth: 0.4,
            meander_amplitude: 2.5,
            meander_max_slope: 0.06,
        }
    }
}

/// A generated continent.
pub struct Continent {
    pub settings: WorldgenSettings,
    /// Terrain heights, row-major (`z * size + x`), world units.
    pub heights: Vec<f32>,
    /// Upstream drainage area of each cell, in cells.
    pub drainage: Vec<f32>,
    /// Lake of each cell, as an index in `lakes`.
    pub lake_of: Vec<Option<u16>>,
    pub lakes: Vec<Lake>,
    pub rivers: Vec<River>,
    /// Time spent in each stage.
    pub timings: Vec<(&'static str, Duration)>,
}

pub struct Lake {
    /// Water surface height.
    pub level: f32,
    pub cells: Vec<UVec2>,
}

/// Where a river ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiverEnd {
    Sea,
    Lake(usize),
    /// Joins another river (index in `Continent::rivers`, always before this one).
    Junction(usize),
}

/// A river, as a polyline from its source to its end.
pub struct River {
    pub points: Vec<RiverPoint>,
    pub end: RiverEnd,
}

#[derive(Clone, Copy, Debug)]
pub struct RiverPoint {
    /// World position of the water surface at the channel center.
    pub position: Vec3,
    /// Channel width, world units.
    pub width: f32,
    /// Channel depth below the water surface, world units.
    pub depth: f32,
}

impl Continent {
    pub fn size(&self) -> usize {
        self.settings.size
    }

    /// World position of cell `(x, z)` (relative to the continent center).
    pub fn cell_to_world(&self, x: usize, z: usize) -> Vec2 {
        (Vec2::new(x as f32, z as f32) - (self.size() / 2) as f32) * self.settings.cell_size
    }

    /// A good place to start a town: beside the widest river, in the lowlands.
    pub fn start_point(&self) -> Vec2 {
        self.rivers
            .iter()
            .flat_map(|river| &river.points)
            .filter(|point| (2.0..20.).contains(&point.position.y))
            .max_by(|a, b| a.width.total_cmp(&b.width))
            .map(|point| point.position.truncate().with_y(point.position.z))
            .unwrap_or(Vec2::ZERO)
    }

    /// Cell under a world position, if it is on the continent grid.
    pub fn world_to_cell(&self, p: Vec2) -> Option<(usize, usize)> {
        let g = (p / self.settings.cell_size + (self.size() / 2) as f32).round();
        let size = self.size() as f32;
        (g.x >= 0. && g.y >= 0. && g.x < size && g.y < size).then_some((g.x as usize, g.y as usize))
    }
}

/// Generate a continent.
pub fn generate(settings: &WorldgenSettings) -> Continent {
    let size = settings.size;
    let noise = noise::Noise::new(settings.seed as u32);
    let mut rng = StdRng::seed_from_u64(settings.seed);
    let mut timings = Vec::new();
    let mut stage = Instant::now();
    let mut lap = |name: &'static str, timings: &mut Vec<(&'static str, Duration)>| {
        timings.push((name, stage.elapsed()));
        stage = Instant::now();
    };

    let shape = terrain::shape(settings, noise);
    lap("shape", &mut timings);

    let mut heights = terrain::erode(settings, &shape);
    // Erosion works in relative units: scale land to world units (drainage is preserved).
    let highest = heights.iter().copied().fold(f32::MIN, f32::max).max(1e-6);
    for (i, h) in heights.iter_mut().enumerate() {
        if shape.base_level[i] {
            *h = shape.sea_floor[i].min(-0.5);
        } else {
            *h *= settings.max_height / highest;
        }
    }
    lap("erosion", &mut timings);

    lakes::carve_lakes(
        settings,
        &mut heights,
        &shape.base_level,
        &shape.land,
        &mut rng,
    );
    let (lakes, lake_of) = lakes::find_lakes(size, &heights, &shape.base_level, 20);
    lap("lakes", &mut timings);

    let routing = flow::priority_flood(size, &heights, &shape.base_level, 1e-4);
    let lake_levels: Vec<f32> = lakes.iter().map(|lake| lake.level).collect();
    let (rivers, valley) = rivers::extract_rivers(
        settings,
        &heights,
        &routing,
        &shape.base_level,
        &lake_of,
        &lake_levels,
    );
    flatten_floodplains(size, &mut heights, &lake_of, &valley);
    let mut rivers = rivers;
    clamp_rivers_to_terrain(settings, &heights, &mut rivers);
    let drainage = flow::FlowGraph::new(size, &routing, &shape.base_level).accumulate(|_| 1.);
    lap("rivers", &mut timings);

    Continent {
        settings: settings.clone(),
        heights,
        drainage,
        lake_of,
        lakes,
        rivers,
        timings,
    }
}

/// Keep every river's water line at or below the terrain under it (smoothing and meanders can
/// move points over lower ground), and never flowing uphill.
fn clamp_rivers_to_terrain(settings: &WorldgenSettings, heights: &[f32], rivers: &mut [River]) {
    let size = settings.size;
    let at = |x: i64, z: i64| {
        let max = size as i64 - 1;
        heights[z.clamp(0, max) as usize * size + x.clamp(0, max) as usize]
    };
    for river in rivers {
        let mut previous = f32::MAX;
        for point in &mut river.points {
            let g = point.position.truncate().with_y(point.position.z) / settings.cell_size
                + (size / 2) as f32;
            let (cell, t) = (g.floor(), g - g.floor());
            let (x, z) = (cell.x as i64, cell.y as i64);
            let bottom = at(x, z) + (at(x + 1, z) - at(x, z)) * t.x;
            let top = at(x, z + 1) + (at(x + 1, z + 1) - at(x, z + 1)) * t.x;
            let terrain = bottom + (top - bottom) * t.y;
            point.position.y = point.position.y.min(terrain).min(previous);
            previous = point.position.y;
        }
    }
}

/// Lower the valley floor to just above the water where rivers meander, so meanders have a
/// flat floodplain to wind through.
fn flatten_floodplains(
    size: usize,
    heights: &mut [f32],
    lake_of: &[Option<u16>],
    valley: &[rivers::ValleyPoint],
) {
    for point in valley.iter().filter(|p| p.floodplain > 0.) {
        let radius = point.floodplain * 1.3 + 1.;
        let target = point.water_level + 0.4;
        let (min, max) = (
            (point.position - radius).floor().max(Vec2::ZERO),
            (point.position + radius)
                .ceil()
                .min(Vec2::splat(size as f32 - 1.)),
        );
        for z in min.y as usize..=max.y as usize {
            for x in min.x as usize..=max.x as usize {
                let i = z * size + x;
                if lake_of[i].is_some() || heights[i] <= target {
                    continue;
                }
                let distance = point.position.distance(Vec2::new(x as f32, z as f32));
                let t = noise::smoothstep(radius * 0.6, radius, distance);
                heights[i] = heights[i].min(target + (heights[i] - target) * t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Continent {
        generate(&WorldgenSettings {
            size: 192,
            erosion_steps: 20,
            lake_spacing: 30.,
            river_min_area: 150.,
            ..WorldgenSettings::default()
        })
    }

    #[test]
    fn rivers_flow_downhill_and_join_earlier_rivers() {
        let continent = small();
        assert!(!continent.rivers.is_empty());
        for (index, river) in continent.rivers.iter().enumerate() {
            for pair in river.points.windows(2) {
                assert!(
                    pair[1].position.y <= pair[0].position.y + 1e-4,
                    "river {index} flows uphill"
                );
            }
            match river.end {
                RiverEnd::Junction(parent) => assert!(parent < index),
                RiverEnd::Lake(lake) => assert!(lake < continent.lakes.len()),
                RiverEnd::Sea => {}
            }
        }
    }

    #[test]
    fn priority_flood_drains_everything() {
        let size = 64;
        let noise = noise::Noise::new(3);
        let heights: Vec<f32> = (0..size * size)
            .map(|i| noise.fbm(Vec2::new((i % size) as f32, (i / size) as f32) / 10., 4) * 10.)
            .collect();
        let base_level: Vec<bool> = (0..size * size).map(|i| i % size == 0).collect();
        let filled = flow::priority_flood(size, &heights, &base_level, 1e-3);
        let graph = flow::FlowGraph::new(size, &filled, &base_level);
        // Every cell reaches an outlet on the border.
        assert_eq!(graph.order.len(), size * size);
        for i in 0..size * size {
            let mut cell = i;
            while graph.receivers[cell] != cell {
                cell = graph.receivers[cell];
            }
            let (x, z) = (cell % size, cell / size);
            assert!(base_level[cell] || x == 0 || z == 0 || x == size - 1 || z == size - 1);
        }
    }
}
