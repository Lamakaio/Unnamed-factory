//! Adaptive terrain meshing.
//!
//! Each chunk is a quadtree whose leaves are patches of [`PATCH_RES`]² quads. A node is split
//! while its cell size is larger than what the terrain needs there:
//! - the base terrain's curvature, against an error tolerance growing with camera distance,
//! - rivers and edits overlapping it (which ask for finer cells, also relaxed with distance).
//!
//! Patches of different levels don't share vertices; skirts (vertical strips hanging from patch
//! edges) hide the resulting cracks. Normals come from the height function, so lighting is
//! continuous across levels.

use bevy::{
    asset::RenderAssetUsages,
    math::I64Vec2,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use super::{CHUNK_SIZE, MIN_CELL, heights::TerrainHeights};

/// Quads per patch side.
const PATCH_RES: usize = 16;
/// Allowed geometric error, in world units per world unit of camera distance.
const ERROR_PER_DISTANCE: f32 = 0.0015;
/// Coarsest cell size allowed for rivers and edits, per world unit of camera distance.
const FEATURE_CELL_PER_DISTANCE: f32 = 0.003;
/// Base terrain detail stops at this fraction of the heightmap resolution.
const MIN_BASE_CELL_RATIO: f32 = 0.5;
/// Ground colors (sRGB) of bare rock on steep slopes and of wet river beds.
const ROCK: [f32; 3] = [0.45, 0.42, 0.40];
const WET_BED: [f32; 3] = [0.28, 0.26, 0.20];
/// Slope steepness (1 - normal.y) range where ground turns to rock.
const ROCK_STEEPNESS: (f32, f32) = (0.3, 0.55);
/// Depth (world units) over which the sea floor darkens.
const SEA_FLOOR_DARKENING: f32 = 25.;
/// Skirt depth, in cells.
const SKIRT_CELLS: f32 = 2.;

/// Build the mesh of the chunk at `chunk`, for a camera at `camera`.
///
/// Vertex positions are relative to the chunk origin (`chunk * CHUNK_SIZE`).
pub fn build_chunk_mesh(heights: &TerrainHeights, chunk: I64Vec2, camera: Vec3) -> Mesh {
    let origin = chunk.as_vec2() * CHUNK_SIZE;
    let mut builder = MeshBuilder {
        heights,
        camera,
        origin,
        positions: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        rock: linear(ROCK),
        wet_bed: linear(WET_BED),
        indices: Vec::new(),
    };
    builder.visit(origin, CHUNK_SIZE);

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, builder.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, builder.normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, builder.colors)
    .with_inserted_indices(Indices::U32(builder.indices))
}

struct MeshBuilder<'a> {
    heights: &'a TerrainHeights,
    camera: Vec3,
    origin: Vec2,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    rock: Vec3,
    wet_bed: Vec3,
    indices: Vec<u32>,
}

impl MeshBuilder<'_> {
    /// Visit the quadtree node covering `min..min + size`.
    fn visit(&mut self, min: Vec2, size: f32) {
        let cell = size / PATCH_RES as f32;
        if cell > MIN_CELL && cell > self.required_cell(min, size) {
            let half = size / 2.;
            for offset in [Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE] {
                self.visit(min + offset * half, half);
            }
        } else {
            self.add_patch(min, cell);
        }
    }

    /// Largest cell size giving enough detail for the node `min..min + size`.
    fn required_cell(&self, min: Vec2, size: f32) -> f32 {
        let max = min + size;
        let center = min + size / 2.;
        let horizontal = (self.camera.xz() - center).abs() - size / 2.;
        let vertical = self.camera.y - self.heights.height(center);
        let distance = horizontal.max(Vec2::ZERO).extend(vertical).length().max(1.);

        let mut cell = f32::MAX;
        // Linear interpolation over a cell of size s deviates by about curvature * (s/g)² / 8
        // from a surface with that curvature per grid spacing g.
        let curvature = self.heights.max_curvature(min, max);
        if curvature > 1e-6 {
            let tolerance = distance * ERROR_PER_DISTANCE;
            let g = self.heights.base_cell();
            cell = (g * (8. * tolerance / curvature).sqrt()).max(g * MIN_BASE_CELL_RATIO);
        }
        if let Some(feature_cell) = self.heights.feature_cell(min, max) {
            cell = cell.min(feature_cell.max(distance * FEATURE_CELL_PER_DISTANCE));
        }
        cell
    }

    fn add_patch(&mut self, min: Vec2, cell: f32) {
        const N: usize = PATCH_RES + 1;
        let first = self.positions.len() as u32;
        for z in 0..N {
            for x in 0..N {
                let p = min + Vec2::new(x as f32, z as f32) * cell;
                let sample = self.heights.sample(p);
                let local = p - self.origin;
                self.positions.push([local.x, sample.height, local.y]);
                let normal = self.heights.normal(p);
                self.normals.push(normal.to_array());
                // Biome color, bare rock on steep slopes, dark beds under water.
                let (low, high) = ROCK_STEEPNESS;
                let rock = ((1. - normal.y - low) / (high - low)).clamp(0., 1.);
                let mut color = sample.color.lerp(self.rock, rock * rock * (3. - 2. * rock));
                color = color.lerp(self.wet_bed, sample.wetness * 0.8);
                if sample.height < 0. {
                    color *= (1. + sample.height / SEA_FLOOR_DARKENING).max(0.35);
                }
                self.colors.push(color.extend(1.).to_array());
            }
        }
        let index = |x: usize, z: usize| first + (z * N + x) as u32;
        for z in 0..PATCH_RES {
            for x in 0..PATCH_RES {
                let (a, b, c, d) = (
                    index(x, z),
                    index(x + 1, z),
                    index(x, z + 1),
                    index(x + 1, z + 1),
                );
                // Counter-clockwise seen from above.
                self.indices.extend([a, c, b, b, c, d]);
            }
        }

        // Skirts along the four edges.
        let depth = cell * SKIRT_CELLS;
        let edges: [Vec<u32>; 4] = [
            (0..N).map(|x| index(x, 0)).collect(),
            (0..N).map(|x| index(x, PATCH_RES)).collect(),
            (0..N).map(|z| index(0, z)).collect(),
            (0..N).map(|z| index(PATCH_RES, z)).collect(),
        ];
        for edge in edges {
            let skirt_first = self.positions.len() as u32;
            for &vertex in &edge {
                let [x, y, z] = self.positions[vertex as usize];
                self.positions.push([x, y - depth, z]);
                self.normals.push(self.normals[vertex as usize]);
                self.colors.push(self.colors[vertex as usize]);
            }
            for i in 0..PATCH_RES {
                let (top_a, top_b) = (edge[i], edge[i + 1]);
                let (bottom_a, bottom_b) = (skirt_first + i as u32, skirt_first + i as u32 + 1);
                self.indices
                    .extend([top_a, bottom_a, top_b, top_b, bottom_a, bottom_b]);
            }
        }
    }
}

/// sRGB color to linear.
fn linear([r, g, b]: [f32; 3]) -> Vec3 {
    Vec3::from_array(Color::srgb(r, g, b).to_linear().to_f32_array_no_alpha())
}
