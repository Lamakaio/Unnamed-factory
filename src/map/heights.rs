//! The continuous terrain height function: base surface (bicubic) + rivers + edits, and the
//! ground color (biomes).
//!
//! [`TerrainHeights`] is cheap to clone (layers are behind `Arc`s), so mesh building tasks get
//! a snapshot of it.

use std::sync::Arc;

use bevy::prelude::*;
use worldgen::{Continent, Heightmap};

use super::{
    edits::{EditLayer, TerrainEdit},
    rivers::RiverLayer,
};

/// Distance used to compute normals by finite differences. It is the same at every LOD level,
/// so lighting matches across LOD seams.
const NORMAL_EPSILON: f32 = 1. / 16.;

/// Height, wetness and ground color of a terrain point.
pub struct TerrainSample {
    /// World height.
    pub height: f32,
    /// How much the point is covered by a river or lake, from 0 to 1.
    pub wetness: f32,
    /// Biome ground color (linear RGB).
    pub color: Vec3,
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
            rivers: Arc::new(RiverLayer::new(&continent.rivers)),
            edits: default(),
        }
    }

    pub fn sample(&self, p: Vec2) -> TerrainSample {
        let mut height = self.base.surface.bicubic(p);
        let wetness = self
            .rivers
            .carve(p, &mut height)
            .max(self.base.lakes.bilinear(p));
        self.edits.apply(p, &mut height);
        TerrainSample {
            height,
            wetness,
            color: self.base.color(p),
        }
    }

    pub fn height(&self, p: Vec2) -> f32 {
        let mut height = self.base.surface.bicubic(p);
        self.rivers.carve(p, &mut height);
        self.edits.apply(p, &mut height);
        height
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

    /// Spacing of the base surface samples, in world units.
    pub fn base_cell(&self) -> f32 {
        self.base.surface.cell
    }

    /// Maximum curvature of the base terrain over `min..max` (see [`BaseGrid`]).
    pub fn max_curvature(&self, min: Vec2, max: Vec2) -> f32 {
        self.base.max_curvature(min, max)
    }
}

/// The continent's fine surface, lake mask and ground colors, with a max-curvature pyramid used
/// to decide the mesh resolution needed to approximate the surface.
///
/// Outside the continent, values extend the border (open sea).
struct BaseGrid {
    surface: Heightmap,
    /// 1 on lake cells, 0 elsewhere (per continent cell).
    lakes: Heightmap,
    /// Biome ground color channels (linear RGB, per continent cell).
    colors: [Heightmap; 3],
    /// `curvature[level]` has `size >> level` texels per side; each texel is the maximum
    /// absolute second difference (world units) over the surface samples it covers.
    curvature: Vec<Vec<f32>>,
}

impl BaseGrid {
    fn new(continent: &Continent) -> Self {
        let surface = continent.surface.clone();
        let lakes = continent.cell_map(
            continent
                .lake_of
                .iter()
                .map(|lake| if lake.is_some() { 1. } else { 0. })
                .collect(),
        );
        let linear: Vec<[f32; 3]> = continent
            .biome
            .iter()
            .map(|biome| {
                let [r, g, b] = biome.color();
                Color::srgb(r, g, b).to_linear().to_f32_array_no_alpha()
            })
            .collect();
        let colors = [0, 1, 2].map(|k| continent.cell_map(linear.iter().map(|c| c[k]).collect()));

        let size = surface.size;
        let at = |x: usize, z: usize| surface.values[z * size + x];
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
            surface,
            lakes,
            colors,
            curvature,
        }
    }

    fn color(&self, p: Vec2) -> Vec3 {
        Vec3::new(
            self.colors[0].bilinear(p),
            self.colors[1].bilinear(p),
            self.colors[2].bilinear(p),
        )
    }

    fn max_curvature(&self, min: Vec2, max: Vec2) -> f32 {
        let size = self.surface.size as i32;
        let lo = self.surface.grid_position(min).floor().as_ivec2() - 1;
        let hi = self.surface.grid_position(max).ceil().as_ivec2() + 1;
        if hi.x < 0 || hi.y < 0 || lo.x >= size || lo.y >= size {
            return 0.;
        }
        let (lo, hi) = (lo.max(IVec2::ZERO), hi.min(IVec2::splat(size - 1)));
        // Coarsest level where the rectangle spans at most ~2 texels per side.
        let span = (hi - lo).max_element().max(1) as u32;
        let level = (span.ilog2() as usize).min(self.curvature.len() - 1);
        let level_size = self.surface.size >> level;
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
