//! Continent generation, independent of the game engine.
//!
//! Pipeline ([`generate`]):
//! 1. **Shape** (A): tectonic plates; province interiors (plains, hill country, plateau),
//!    mountain ranges along converging plate boundaries, rift basins along diverging ones,
//!    warped coastline, orographic rain from a prevailing wind.
//! 2. **Erosion** (B): stream-power law with an implicit solver plus hillslope diffusion, with
//!    per-province rock hardness and diffusion; heights are then scaled to world units and
//!    plateaus capped into tablelands.
//! 3. **Lakes**: valley sites dammed at a level, found back with Priority-Flood.
//! 4. **Rivers** (C): cells draining a large area, traced into smoothed, meandering polylines
//!    with a width and depth from their discharge; floodplains are flattened along them.
//! 5. **Climate and biomes** (E): moisture from orographic rain and wet valleys, temperature
//!    from altitude, biome per cell.
//! 6. **Detail** (D): the eroded grid upsampled to a finer surface with gullies on slopes.
//!
//! Coordinates: cell `(x, z)` is at world `((x - size/2) * cell_size, (z - size/2) * cell_size)`
//! relative to the continent center. Heights are world units, sea level is 0.

mod climate;
mod detail;
mod flow;
mod heightmap;
mod lakes;
pub mod noise;
mod rivers;
mod terrain;

use std::time::{Duration, Instant};

use bevy_math::{UVec2, Vec2, Vec3};
use rand::{RngExt, SeedableRng, rngs::StdRng};

pub use heightmap::Heightmap;

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
    /// Number of tectonic plates (provinces).
    pub plate_count: usize,
    /// Relative frequency of plate interiors: `[plains, hill country, plateau]`.
    pub province_weights: [f32; 3],
    /// Width (cells) of the blend between neighbouring provinces.
    pub province_blend: f32,
    /// Half-width (cells) of mountain range belts.
    pub range_width: f32,
    /// Distance (cells) between the parallel ridges of a range.
    pub ridge_spacing: f32,
    /// Height (world units) of plateau tops.
    pub plateau_level: f32,
    /// Height of the highest summit, in world units.
    pub max_height: f32,
    /// Depth of the open sea, in world units.
    pub sea_depth: f32,
    /// How much the land rises from the coast to 100 cells inland, in world units.
    pub inland_rise: f32,
    pub erosion_steps: usize,
    pub erosion_dt: f32,
    /// Stream power erodibility K.
    pub erodibility: f32,
    /// Stream power area exponent m (slope exponent n = 1).
    pub area_exponent: f32,
    /// Base hillslope diffusion D (scaled per province).
    pub diffusion: f32,
    /// Random weight (0..) on slopes when routing flow during erosion, against grid artifacts.
    pub routing_jitter: f32,
    /// Prevailing wind direction (radians); random from the seed when `None`.
    pub wind_angle: Option<f32>,
    /// Fraction of the air's moisture raining per cell on flat land.
    pub base_rain: f32,
    /// Extra rain fraction per unit of rise along the wind.
    pub orographic_rain: f32,
    /// Fraction of the rain over land that evaporates back into the air.
    pub land_evaporation: f32,
    /// Height (world units) where it gets cold enough for permanent snow.
    pub snow_line: f32,
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
    /// Fine surface samples per cell side.
    pub detail_resolution: usize,
    /// Wavelength (world units) of the largest gullies.
    pub detail_wavelength: f32,
    pub detail_octaves: u32,
    /// Gully depth (world units) on the most rugged slopes.
    pub detail_amplitude: f32,
}

impl Default for WorldgenSettings {
    fn default() -> Self {
        Self {
            seed: 1082,
            size: 512,
            cell_size: 1.,
            land_radius: 0.8,
            plate_count: 10,
            province_weights: [0.35, 0.4, 0.25],
            province_blend: 20.,
            range_width: 28.,
            ridge_spacing: 13.,
            plateau_level: 17.,
            max_height: 38.,
            sea_depth: 20.,
            inland_rise: 3.,
            erosion_steps: 60,
            erosion_dt: 2.,
            erodibility: 0.25,
            area_exponent: 0.45,
            diffusion: 0.05,
            routing_jitter: 1.,
            wind_angle: None,
            base_rain: 0.012,
            orographic_rain: 0.1,
            land_evaporation: 0.6,
            snow_line: 34.,
            lake_count: 5,
            lake_min_area: 300.,
            lake_spacing: 60.,
            river_min_area: 400.,
            river_width: 0.5,
            river_depth: 0.4,
            meander_amplitude: 2.5,
            meander_max_slope: 0.06,
            detail_resolution: 4,
            detail_wavelength: 8.,
            detail_octaves: 3,
            detail_amplitude: 1.2,
        }
    }
}

/// Large-scale landform of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landform {
    Sea,
    Plains,
    Hills,
    Plateau,
    Mountains,
}

/// Biome of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Biome {
    Sea,
    Lake,
    Beach,
    Marsh,
    Meadow,
    Steppe,
    DeciduousForest,
    ConiferForest,
    Alpine,
    Rock,
    Snow,
}

impl Biome {
    /// Ground color (sRGB, 0..1), shared by previews and the game.
    pub fn color(self) -> [f32; 3] {
        match self {
            Biome::Sea => [0.45, 0.42, 0.33],
            Biome::Lake => [0.35, 0.33, 0.25],
            Biome::Beach => [0.86, 0.80, 0.60],
            Biome::Marsh => [0.36, 0.42, 0.22],
            Biome::Meadow => [0.50, 0.68, 0.30],
            Biome::Steppe => [0.72, 0.68, 0.40],
            Biome::DeciduousForest => [0.26, 0.48, 0.18],
            Biome::ConiferForest => [0.17, 0.34, 0.20],
            Biome::Alpine => [0.55, 0.60, 0.45],
            Biome::Rock => [0.48, 0.45, 0.42],
            Biome::Snow => [0.94, 0.95, 0.97],
        }
    }
}

impl Landform {
    /// Debug color (sRGB, 0..1).
    pub fn color(self) -> [f32; 3] {
        match self {
            Landform::Sea => [0.15, 0.30, 0.55],
            Landform::Plains => [0.55, 0.75, 0.35],
            Landform::Hills => [0.35, 0.55, 0.25],
            Landform::Plateau => [0.75, 0.55, 0.30],
            Landform::Mountains => [0.55, 0.50, 0.50],
        }
    }
}

/// A generated continent.
pub struct Continent {
    pub settings: WorldgenSettings,
    /// Eroded terrain heights per cell, row-major (`z * size + x`), world units.
    pub heights: Vec<f32>,
    /// Fine terrain surface (with gullies): what the game renders.
    pub surface: Heightmap,
    /// Upstream drainage area of each cell, in cells.
    pub drainage: Vec<f32>,
    /// Lake of each cell, as an index in `lakes`.
    pub lake_of: Vec<Option<u16>>,
    pub lakes: Vec<Lake>,
    pub rivers: Vec<River>,
    pub landform: Vec<Landform>,
    /// 0 (smooth) .. 1 (rugged ranges).
    pub ruggedness: Vec<f32>,
    /// Relative moisture (mean ~1 on land).
    pub moisture: Vec<f32>,
    /// 0 (snow line) .. 1 (warm lowlands).
    pub temperature: Vec<f32>,
    pub biome: Vec<Biome>,
    /// Prevailing wind direction (radians).
    pub wind_angle: f32,
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

    /// Per-cell values as a [`Heightmap`] (for smooth sampling).
    pub fn cell_map(&self, values: Vec<f32>) -> Heightmap {
        Heightmap {
            size: self.size(),
            cell: self.settings.cell_size,
            values,
        }
    }
}

/// Generate a continent.
pub fn generate(settings: &WorldgenSettings) -> Continent {
    let size = settings.size;
    let noise = noise::Noise::new(settings.seed as u32);
    let mut rng = StdRng::seed_from_u64(settings.seed);
    let mut settings = settings.clone();
    let wind_angle = *settings
        .wind_angle
        .get_or_insert_with(|| rng.random_range(0.0..std::f32::consts::TAU));
    let settings = &settings;
    let mut timings = Vec::new();
    let mut stage = Instant::now();
    let mut lap = |name: &'static str, timings: &mut Vec<(&'static str, Duration)>| {
        timings.push((name, stage.elapsed()));
        stage = Instant::now();
    };

    let shape = terrain::shape(settings, noise, &mut rng);
    lap("shape", &mut timings);

    let mut heights = terrain::erode(settings, &shape);
    // Erosion works in relative units: scale land to world units (drainage is preserved). The
    // mountain ranges set the scale, so each province keeps its own relief.
    let mut summits: Vec<f32> = (0..heights.len())
        .filter(|&i| !shape.base_level[i] && shape.range[i] > 0.5)
        .map(|i| heights[i])
        .collect();
    if summits.is_empty() {
        summits = heights.clone();
    }
    summits.sort_by(f32::total_cmp);
    let reference = summits[(summits.len() - 1) * 99 / 100].max(1e-6);
    for (i, h) in heights.iter_mut().enumerate() {
        if shape.base_level[i] {
            *h = shape.sea_floor[i].min(-0.5);
        } else {
            *h *= settings.max_height / reference;
        }
    }
    terrain::smooth_cells(settings, &shape, &mut heights);
    terrain::shape_coast(settings, &shape, noise, &mut heights);
    terrain::cap_plateaus(&shape, &mut heights);
    terrain::limit_slopes(settings, &shape, &mut heights);
    lap("erosion", &mut timings);

    lakes::carve_lakes(settings, &mut heights, &shape.base_level, &shape.land, &mut rng);
    let (lakes, lake_of) = lakes::find_lakes(size, &heights, &shape.base_level, 20);
    lap("lakes", &mut timings);

    let routing = flow::priority_flood(size, &heights, &shape.base_level, 1e-4);
    let lake_levels: Vec<f32> = lakes.iter().map(|lake| lake.level).collect();
    let (mut rivers, valley) = rivers::extract_rivers(
        settings,
        &heights,
        &routing,
        &shape.base_level,
        &lake_of,
        &lake_levels,
    );
    flatten_floodplains(size, &mut heights, &lake_of, &valley);
    let drainage = flow::FlowGraph::new(size, &routing, &shape.base_level).accumulate(|_| 1.);
    lap("rivers", &mut timings);

    let climate = climate::climate(settings, noise, &heights, &shape.base_level, &lake_of, &drainage);
    let landform: Vec<Landform> = (0..size * size).map(|i| shape.landform(i)).collect();
    let ruggedness: Vec<f32> = (0..size * size).map(|i| shape.ruggedness(i)).collect();
    lap("climate", &mut timings);

    let grid = |values: Vec<f32>| Heightmap {
        size,
        cell: settings.cell_size,
        values,
    };
    let calm = calm_map(settings, &rivers, &lake_of);
    let surface = detail::refine(
        settings,
        &grid(heights.clone()),
        &grid(ruggedness.clone()),
        &grid(calm),
    );
    clamp_rivers_to_terrain(&surface, &mut rivers);
    lap("detail", &mut timings);

    Continent {
        settings: settings.clone(),
        heights,
        surface,
        drainage,
        lake_of,
        lakes,
        rivers,
        landform,
        ruggedness,
        moisture: climate.moisture,
        temperature: climate.temperature,
        biome: climate.biome,
        wind_angle,
        timings,
    }
}

/// 1 where fine detail must vanish (around rivers and lakes), per cell.
fn calm_map(settings: &WorldgenSettings, rivers: &[River], lake_of: &[Option<u16>]) -> Vec<f32> {
    let size = settings.size;
    let mut calm = vec![0f32; size * size];
    let half = (size / 2) as f32;
    for point in rivers.iter().flat_map(|river| &river.points) {
        let center = point.position.truncate().with_y(point.position.z) / settings.cell_size + half;
        let radius = (point.width * 1.1 + 2.) / settings.cell_size + 2.;
        let (min, max) = ((center - radius).floor(), (center + radius).ceil());
        for z in (min.y.max(0.) as usize)..=(max.y.min(size as f32 - 1.) as usize) {
            for x in (min.x.max(0.) as usize)..=(max.x.min(size as f32 - 1.) as usize) {
                let d = center.distance(Vec2::new(x as f32, z as f32));
                let value = 1. - noise::smoothstep(radius - 2., radius, d);
                calm[z * size + x] = calm[z * size + x].max(value);
            }
        }
    }
    for i in 0..size * size {
        if lake_of[i].is_some() {
            let (x, z) = ((i % size) as i64, (i / size) as i64);
            for dz in -2..=2 {
                for dx in -2..=2 {
                    let (nx, nz) = (x + dx, z + dz);
                    if nx >= 0 && nz >= 0 && (nx as usize) < size && (nz as usize) < size {
                        calm[nz as usize * size + nx as usize] = 1.;
                    }
                }
            }
        }
    }
    calm
}

/// Keep every river's water line at or below the surface under it, across the whole channel
/// width (smoothing and meanders can move points over lower ground, and coasts are low), and
/// never flowing uphill.
fn clamp_rivers_to_terrain(surface: &Heightmap, rivers: &mut [River]) {
    for river in rivers {
        let mut previous = f32::MAX;
        for i in 0..river.points.len() {
            let points = &river.points;
            let center = points[i].position.truncate().with_y(points[i].position.z);
            let next = points[(i + 1).min(points.len() - 1)].position;
            let last = points[i.saturating_sub(1)].position;
            let across = (next - last).truncate().with_y(next.z - last.z).normalize_or_zero().perp();
            let terrain = [-0.5, 0., 0.5]
                .map(|side| surface.bilinear(center + across * side * points[i].width))
                .into_iter()
                .fold(f32::MAX, f32::min);
            let point = &mut river.points[i];
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
        let radius = point.floodplain * 1.5 + 2.;
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
                let t = noise::smoothstep(radius * 0.4, radius, distance);
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
            detail_resolution: 2,
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
    fn every_landform_appears() {
        let continent = small();
        for landform in [Landform::Plains, Landform::Hills, Landform::Plateau] {
            assert!(continent.landform.contains(&landform), "{landform:?} missing");
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
