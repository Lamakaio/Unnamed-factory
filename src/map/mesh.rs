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

use super::{CHUNK_SIZE, GRID_SQUARE_SIZE, MIN_CELL, SCALE_Y, heights::TerrainHeights};

/// Quads per patch side.
const PATCH_RES: usize = 16;
/// Allowed geometric error, in world units per world unit of camera distance.
const ERROR_PER_DISTANCE: f32 = 0.0015;
/// Coarsest cell size allowed for rivers and edits, per world unit of camera distance.
const FEATURE_CELL_PER_DISTANCE: f32 = 0.003;
/// Base terrain detail stops at half the heightmap resolution: there is no data below.
const MIN_BASE_CELL: f32 = GRID_SQUARE_SIZE / 2.;
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
        uvs: Vec::new(),
        indices: Vec::new(),
    };
    builder.visit(origin, CHUNK_SIZE);

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, builder.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, builder.normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, builder.uvs)
    .with_inserted_indices(Indices::U32(builder.indices))
}

struct MeshBuilder<'a> {
    heights: &'a TerrainHeights,
    camera: Vec3,
    origin: Vec2,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
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
            cell = (GRID_SQUARE_SIZE * (8. * tolerance / curvature).sqrt()).max(MIN_BASE_CELL);
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
                self.normals.push(self.heights.normal(p).to_array());
                // Read by the terrain shader: u = biome coordinate from height, v = river flag.
                self.uvs.push([
                    1.3 * sample.height / SCALE_Y - 0.35,
                    1. + 99. * sample.wetness,
                ]);
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
                self.uvs.push(self.uvs[vertex as usize]);
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
