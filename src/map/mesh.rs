//! Adaptive terrain meshing.
//!
//! Each chunk is a quadtree whose leaves are patches of [`PATCH_RES`]² quads. Its nodes at
//! depth [`PART_DEPTH`] are meshed separately (`TerrainPart`), so a rebuild near the camera
//! only redoes a small part; where the tree stops above that depth, the part at the corner
//! of the leaf meshes it and the others are empty. A node is split
//! while its cell size is larger than what the terrain needs there:
//! - the base terrain's curvature, against an error tolerance growing with camera distance,
//! - rivers and edits overlapping it (which ask for finer cells, also relaxed with distance).
//!
//! Patches of different levels don't share vertices: the edge vertices of a patch next to a
//! coarser one are snapped onto the coarser edge, and skirts (vertical strips hanging from
//! patch edges) hide any remaining crack (e.g. against a chunk meshed for another camera
//! position). Lighting is per pixel (see the terrain shader): the UVs carry the slope that
//! rivers and edits add to the base surface.

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
/// Skirt depth, in cells.
const SKIRT_CELLS: f32 = 2.;

/// Depth of the quadtree nodes meshed separately, and their number per chunk side.
pub const PART_DEPTH: u32 = 2;
pub const PARTS: u32 = 1 << PART_DEPTH;

/// Build the mesh of part `index` (x, z, at depth [`PART_DEPTH`]) of the chunk at `chunk`, for a
/// camera at `camera`; `None` if the part has no geometry of its own.
///
/// Vertex positions are relative to the chunk origin (`chunk * CHUNK_SIZE`).
pub fn build_part_mesh(
    heights: &TerrainHeights,
    chunk: I64Vec2,
    index: UVec2,
    camera: Vec3,
) -> Option<Mesh> {
    let origin = chunk.as_vec2() * CHUNK_SIZE;
    let mut builder = MeshBuilder {
        heights,
        camera,
        origin,
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
    };
    // Walk down to the part's node; a leaf above it is meshed by the part at its corner.
    let part_size = CHUNK_SIZE / PARTS as f32;
    let part_min = origin + index.as_vec2() * part_size;
    let (mut min, mut size) = (origin, CHUNK_SIZE);
    while size > part_size * 1.5 {
        if !builder.splits(min, size) {
            if part_min.distance(min) > 1e-3 {
                return None;
            }
            builder.add_patch(min, size);
            return Some(builder.build());
        }
        size /= 2.;
        min += ((part_min - min) / size + 1e-3).floor().clamp(Vec2::ZERO, Vec2::ONE) * size;
    }
    builder.visit(min, size);
    Some(builder.build())
}

impl MeshBuilder<'_> {
    fn build(self) -> Mesh {
        Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
    .with_inserted_indices(Indices::U32(self.indices))
    }
}

struct MeshBuilder<'a> {
    heights: &'a TerrainHeights,
    camera: Vec3,
    origin: Vec2,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    /// Slope added by rivers and edits.
    uvs: Vec<[f32; 2]>,
    /// Red: river and lake wetness; green: grass clearance (see
    /// `TerrainHeights::grass_clearance`), for the terrain shader.
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder<'_> {
    /// Visit the quadtree node covering `min..min + size`.
    fn visit(&mut self, min: Vec2, size: f32) {
        if self.splits(min, size) {
            let half = size / 2.;
            for offset in [Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE] {
                self.visit(min + offset * half, half);
            }
        } else {
            self.add_patch(min, size);
        }
    }

    fn splits(&self, min: Vec2, size: f32) -> bool {
        let cell = size / PATCH_RES as f32;
        cell > MIN_CELL && cell > self.required_cell(min, size)
    }

    /// The leaf node (min, size) containing `p`, in whichever chunk it is.
    fn leaf_at(&self, p: Vec2) -> (Vec2, f32) {
        let mut min = (p / CHUNK_SIZE).floor() * CHUNK_SIZE;
        let mut size = CHUNK_SIZE;
        while self.splits(min, size) {
            size /= 2.;
            min += ((p - min) / size).floor().clamp(Vec2::ZERO, Vec2::ONE) * size;
        }
        (min, size)
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

    fn add_patch(&mut self, min: Vec2, size: f32) {
        const N: usize = PATCH_RES + 1;
        let cell = size / PATCH_RES as f32;
        let first = self.positions.len() as u32;
        for z in 0..N {
            for x in 0..N {
                let p = min + Vec2::new(x as f32, z as f32) * cell;
                let sample = self.heights.sample(p);
                let local = p - self.origin;
                self.positions.push([local.x, sample.height, local.y]);
                self.normals.push(self.heights.normal(p).to_array());
                self.uvs.push(self.heights.feature_gradient(p).to_array());
                self.colors
                    .push([sample.wetness, self.heights.grass_clearance(p), 0., 1.]);
            }
        }
        let index = |x: usize, z: usize| first + (z * N + x) as u32;

        // Snap edges onto coarser neighbours: their edge is a straight line between their own
        // vertices, which are also ours.
        let edges: [(Vec2, Vec2, Vec2, fn(usize) -> (usize, usize)); 4] = [
            (min, Vec2::X, -Vec2::Y, |k| (k, 0)),
            (min + Vec2::Y * size, Vec2::X, Vec2::Y, |k| (k, PATCH_RES)),
            (min, Vec2::Y, -Vec2::X, |k| (0, k)),
            (min + Vec2::X * size, Vec2::Y, Vec2::X, |k| (PATCH_RES, k)),
        ];
        for (start, along, outward, vertex) in edges {
            let (neighbour, neighbour_size) =
                self.leaf_at(start + along * size / 2. + outward * cell / 2.);
            let neighbour_cell = neighbour_size / PATCH_RES as f32;
            if neighbour_cell < cell * 1.5 {
                continue;
            }
            for k in 0..N {
                let p = start + along * k as f32 * cell;
                let t = (p - neighbour).dot(along) / neighbour_cell;
                let f = t - t.floor();
                if f < 1e-3 || f > 1. - 1e-3 {
                    continue;
                }
                let a = p - along * f * neighbour_cell;
                let b = a + along * neighbour_cell;
                let height = self.heights.height(a).lerp(self.heights.height(b), f);
                let (x, z) = vertex(k);
                self.positions[index(x, z) as usize][1] = height;
            }
        }

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
