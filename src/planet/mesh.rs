//! Planet chunk meshing: each chunk is a quadtree of patches of [`PATCH_RES`]² quads in its
//! face's cell coordinates, split while a patch's cells are coarser than the camera distance
//! allows. Vertices sit on the sphere at the surface height; skirts (strips hanging toward
//! the centre from patch edges) hide the cracks between patches of different sizes.

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use worldgen::surface::PlanetSurface;

use super::{CHUNK_CELLS, PlanetChunk};

/// Quads per patch side.
const PATCH_RES: usize = 16;
/// Allowed cell size (in cells of the planet's grid) per world unit of camera distance, and
/// the finest cell size.
const CELL_PER_DISTANCE: f32 = 0.004;
const MIN_CELL: f32 = 0.5;
/// Skirt depth, in patch cells.
const SKIRT_CELLS: f32 = 2.;
/// Step for normals, in cells.
const NORMAL_STEP: f32 = 0.25;

/// Distance from the camera to the nearest point of a chunk (at sea level, roughly).
pub fn distance_to_chunk(surface: &PlanetSurface, chunk: &PlanetChunk, camera: Vec3) -> f32 {
    let half = CHUNK_CELLS as f32 / 2.;
    let center = chunk.center();
    // The chunk's nearest point to the camera's direction, clamped to the chunk.
    let (face, p) = surface.face_coords(camera.normalize_or_zero());
    let nearest = if face == chunk.face {
        p.clamp(center - half, center + half)
    } else {
        center
    };
    let point = surface.direction(chunk.face, nearest) * surface.radius;
    let corner_slack = if face == chunk.face { 0. } else { half * 1.5 };
    (camera.distance(point) - corner_slack).max(1.)
}

/// The mesh of a chunk for a camera at `camera`, relative to `origin`.
pub fn build_chunk_mesh(
    surface: &PlanetSurface,
    chunk: &PlanetChunk,
    origin: Vec3,
    camera: Vec3,
) -> Mesh {
    let mut builder = Builder {
        surface,
        face: chunk.face,
        origin,
        camera,
        positions: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
    };
    builder.visit(chunk.min - 0.5, CHUNK_CELLS as f32);
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, builder.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, builder.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, builder.colors)
        .with_inserted_indices(Indices::U32(builder.indices))
}

struct Builder<'a> {
    surface: &'a PlanetSurface,
    face: usize,
    origin: Vec3,
    camera: Vec3,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Builder<'_> {
    /// The quadtree node covering `min..min + size` (cell coordinates).
    fn visit(&mut self, min: Vec2, size: f32) {
        let cell = size / PATCH_RES as f32;
        let center = min + size / 2.;
        let point = self.surface.position(self.face, center);
        // (Distance to the node, not its centre: minus its half diagonal.)
        let distance = (self.camera.distance(point) - size * 0.75).max(1.);
        let wanted = (distance * CELL_PER_DISTANCE).max(MIN_CELL);
        if cell > wanted && cell > MIN_CELL {
            let half = size / 2.;
            for offset in [Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE] {
                self.visit(min + offset * half, half);
            }
        } else {
            self.add_patch(min, size);
        }
    }

    fn add_patch(&mut self, min: Vec2, size: f32) {
        const N: usize = PATCH_RES + 1;
        let cell = size / PATCH_RES as f32;
        let first = self.positions.len() as u32;
        let face = self.face;
        for z in 0..N {
            for x in 0..N {
                let p = min + Vec2::new(x as f32, z as f32) * cell;
                let position = self.surface.position(face, p);
                self.positions.push((position - self.origin).to_array());
                // Normal from the surface's tangents, pointing out.
                let du = self.surface.position(face, p + Vec2::X * NORMAL_STEP)
                    - self.surface.position(face, p - Vec2::X * NORMAL_STEP);
                let dv = self.surface.position(face, p + Vec2::Y * NORMAL_STEP)
                    - self.surface.position(face, p - Vec2::Y * NORMAL_STEP);
                let mut normal = du.cross(dv).normalize_or_zero();
                if normal.dot(position) < 0. {
                    normal = -normal;
                }
                self.normals.push(normal.to_array());
                let height = self.surface.height(face, p);
                let lake = self.surface.lake_depth.bilinear(face, p);
                self.colors.push(relief_colour(height, lake));
            }
        }
        let index = |x: usize, z: usize| first + (z * N + x) as u32;
        // Triangles wound counter-clockwise seen from outside (the face's `u × v` points out).
        let outward = {
            let [a, b, c] = [index(0, 0), index(1, 0), index(0, 1)]
                .map(|i| Vec3::from(self.positions[i as usize]));
            (b - a).cross(c - a).dot(a + self.origin) > 0.
        };
        for z in 0..PATCH_RES {
            for x in 0..PATCH_RES {
                let (a, b, c, d) = (index(x, z), index(x + 1, z), index(x, z + 1), index(x + 1, z + 1));
                if outward {
                    self.indices.extend([a, b, c, b, d, c]);
                } else {
                    self.indices.extend([a, c, b, b, c, d]);
                }
            }
        }
        // Skirts: each edge's vertices again, lowered toward the centre.
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
                let world = Vec3::from(self.positions[vertex as usize]) + self.origin;
                let lowered = world - world.normalize() * depth;
                self.positions.push((lowered - self.origin).to_array());
                self.normals.push(self.normals[vertex as usize]);
                self.colors.push(self.colors[vertex as usize]);
            }
            for i in 0..PATCH_RES {
                let (top_a, top_b) = (edge[i], edge[i + 1]);
                let (bottom_a, bottom_b) = (skirt_first + i as u32, skirt_first + i as u32 + 1);
                // (Both windings: skirts are seen from either side.)
                self.indices.extend([
                    top_a, bottom_a, top_b, top_b, bottom_a, bottom_b, top_a, top_b, bottom_a,
                    top_b, bottom_b, bottom_a,
                ]);
            }
        }
    }
}

/// Ground colour by height (as the planet previews, until biomes come with the climate).
fn relief_colour(height: f32, lake: f32) -> [f32; 4] {
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
        let t = t.clamp(0., 1.);
        [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
    };
    let colour = if lake > 0.05 {
        mix([0.25, 0.5, 0.78], [0.12, 0.3, 0.6], lake / 4.)
    } else if height < 0. {
        mix([0.62, 0.58, 0.45], [0.2, 0.22, 0.25], -height / 10.)
    } else {
        let stops = [
            (0., [0.36, 0.56, 0.27]),
            (6., [0.52, 0.62, 0.32]),
            (14., [0.58, 0.50, 0.33]),
            (24., [0.55, 0.48, 0.42]),
            (32., [0.70, 0.68, 0.66]),
            (38., [0.95, 0.95, 0.97]),
        ];
        let mut colour = stops[stops.len() - 1].1;
        for w in stops.windows(2) {
            if height < w[1].0 {
                colour = mix(w[0].1, w[1].1, (height - w[0].0) / (w[1].0 - w[0].0));
                break;
            }
        }
        colour
    };
    // (Vertex colours are linear.)
    let linear = Color::srgb(colour[0], colour[1], colour[2]).to_linear();
    [linear.red, linear.green, linear.blue, 1.]
}
