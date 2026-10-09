//! The continuous terrain height function: base heightmap (bicubic) + rivers + edits.
//!
//! [`TerrainHeights`] is cheap to clone (layers are behind `Arc`s), so mesh building tasks get
//! a snapshot of it.

use std::sync::Arc;

use bevy::prelude::*;

use super::{
    GRID_SQUARE_SIZE, SCALE_Y,
    edits::{EditLayer, TerrainEdit},
    rivers::RiverLayer,
};
use crate::mapgen::Continent;

/// Distance used to compute normals by finite differences. It is the same at every LOD level,
/// so lighting matches across LOD seams.
const NORMAL_EPSILON: f32 = 1. / 16.;

/// Height and wetness of a terrain point.
pub struct TerrainSample {
    /// World height.
    pub height: f32,
    /// How much the point is covered by a river, from 0 to 1.
    pub wetness: f32,
}

#[derive(Clone)]
pub struct TerrainHeights {
    base: Arc<BaseGrid>,
    rivers: Arc<RiverLayer>,
    edits: Arc<EditLayer>,
}

impl TerrainHeights {
    pub fn new(continent: &Continent) -> Self {
        Self {
            base: Arc::new(BaseGrid::new(continent)),
            rivers: Arc::new(RiverLayer::new(&continent.river_paths)),
            edits: default(),
        }
    }

    pub fn sample(&self, p: Vec2) -> TerrainSample {
        let mut height = self.base.height(p);
        let wetness = self.rivers.carve(p, &mut height);
        self.edits.apply(p, &mut height);
        TerrainSample { height, wetness }
    }

    pub fn height(&self, p: Vec2) -> f32 {
        self.sample(p).height
    }

    pub fn normal(&self, p: Vec2) -> Vec3 {
        let dx =
            self.height(p + Vec2::X * NORMAL_EPSILON) - self.height(p - Vec2::X * NORMAL_EPSILON);
        let dz =
            self.height(p + Vec2::Y * NORMAL_EPSILON) - self.height(p - Vec2::Y * NORMAL_EPSILON);
        Vec3::new(-dx, 2. * NORMAL_EPSILON, -dz).normalize()
    }

    pub fn add_edit(&mut self, edit: TerrainEdit) {
        Arc::make_mut(&mut self.edits).push(edit);
    }

    /// Smallest mesh cell size requested by rivers or edits overlapping `min..max`.
    pub fn feature_cell(&self, min: Vec2, max: Vec2) -> Option<f32> {
        match (
            self.rivers.detail_cell(min, max),
            self.edits.detail_cell(min, max),
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Maximum curvature of the base terrain over `min..max` (see [`BaseGrid`]).
    pub fn max_curvature(&self, min: Vec2, max: Vec2) -> f32 {
        self.base.max_curvature(min, max)
    }
}

/// The continent heightmap, in world units, with a max-curvature pyramid used to decide the
/// mesh resolution needed to approximate it.
///
/// Outside the continent, heights extend the border values (flat ocean).
struct BaseGrid {
    size: usize,
    /// Row-major (`z * size + x`) world heights.
    heights: Vec<f32>,
    /// `curvature[level]` has `size >> level` texels per side; each texel is the maximum
    /// absolute second difference (world units) over the grid points it covers.
    curvature: Vec<Vec<f32>>,
}

impl BaseGrid {
    fn new(continent: &Continent) -> Self {
        let size = Continent::CONTINENT_SIZE as usize;
        let mut heights = vec![0.; size * size];
        for z in 0..size {
            for x in 0..size {
                heights[z * size + x] = continent[(x as u32, z as u32)].height * SCALE_Y;
            }
        }

        let at = |x: usize, z: usize| heights[z * size + x];
        let mut level0 = vec![0.; size * size];
        for z in 1..size - 1 {
            for x in 1..size - 1 {
                let h = at(x, z);
                let cx = (at(x - 1, z) + at(x + 1, z) - 2. * h).abs();
                let cz = (at(x, z - 1) + at(x, z + 1) - 2. * h).abs();
                level0[z * size + x] = cx.max(cz);
            }
        }
        let mut curvature = vec![level0];
        while curvature.last().unwrap().len() > 1 {
            let previous = curvature.last().unwrap();
            let previous_size = (previous.len() as f32).sqrt() as usize;
            let next_size = previous_size / 2;
            let mut next = vec![0.; next_size * next_size];
            for z in 0..next_size {
                for x in 0..next_size {
                    next[z * next_size + x] = [(0, 0), (1, 0), (0, 1), (1, 1)]
                        .into_iter()
                        .map(|(dx, dz)| previous[(2 * z + dz) * previous_size + 2 * x + dx])
                        .fold(0., f32::max);
                }
            }
            curvature.push(next);
        }

        Self {
            size,
            heights,
            curvature,
        }
    }

    /// Continuous grid coordinates of a world position.
    fn grid_position(&self, p: Vec2) -> Vec2 {
        p / GRID_SQUARE_SIZE + (self.size / 2) as f32
    }

    fn at(&self, x: i32, z: i32) -> f32 {
        let max = self.size as i32 - 1;
        self.heights[z.clamp(0, max) as usize * self.size + x.clamp(0, max) as usize]
    }

    /// Bicubic (Catmull-Rom) interpolation of the heightmap.
    fn height(&self, p: Vec2) -> f32 {
        let g = self.grid_position(p);
        let cell = g.floor();
        let t = g - cell;
        let (x, z) = (cell.x as i32, cell.y as i32);
        let row = |dz: i32| {
            catmull_rom(
                [
                    self.at(x - 1, z + dz),
                    self.at(x, z + dz),
                    self.at(x + 1, z + dz),
                    self.at(x + 2, z + dz),
                ],
                t.x,
            )
        };
        catmull_rom([row(-1), row(0), row(1), row(2)], t.y)
    }

    fn max_curvature(&self, min: Vec2, max: Vec2) -> f32 {
        let size = self.size as i32;
        let lo = self.grid_position(min).floor().as_ivec2() - 1;
        let hi = self.grid_position(max).ceil().as_ivec2() + 1;
        if hi.x < 0 || hi.y < 0 || lo.x >= size || lo.y >= size {
            return 0.;
        }
        let (lo, hi) = (lo.max(IVec2::ZERO), hi.min(IVec2::splat(size - 1)));
        // Coarsest level where the rectangle spans at most ~2 texels per side.
        let span = (hi - lo).max_element().max(1) as u32;
        let level = (span.ilog2() as usize).min(self.curvature.len() - 1);
        let level_size = self.size >> level;
        let (lo, hi) = (lo >> level as i32, hi >> level as i32);
        let mut result: f32 = 0.;
        for z in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                result = result.max(self.curvature[level][z as usize * level_size + x as usize]);
            }
        }
        result
    }
}

fn catmull_rom([p0, p1, p2, p3]: [f32; 4], t: f32) -> f32 {
    0.5 * (2. * p1
        + (p2 - p0) * t
        + (2. * p0 - 5. * p1 + 4. * p2 - p3) * t * t
        + (3. * (p1 - p2) + p3 - p0) * t * t * t)
}
