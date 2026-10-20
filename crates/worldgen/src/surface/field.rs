//! Per-cell fields of the planet sampled between cells: each cube face as a 2D grid, padded
//! with the neighbouring faces' cells (found by direction), interpolated in the face's cell
//! coordinates.

use bevy_math::Vec2;

use crate::{planet::Planet, sphere::FACES};

/// Cells of padding around each face: the bicubic stencil reaches two cells out, and fine
/// detail near a face's edge is also evaluated on the neighbouring face, up to
/// `relief::EDGE_BLEND` cells past its edge.
const PAD: usize = 14;

/// A per-cell field of the planet, sampled continuously face by face.
#[derive(Clone)]
pub struct FaceField {
    n: usize,
    /// Each face's values with [`PAD`] cells of padding, `(n + 2 PAD)²`, row-major.
    faces: Vec<Vec<f32>>,
}

impl FaceField {
    /// `values` per cell of the planet's grid.
    pub fn new(planet: &Planet, values: &[f32]) -> Self {
        let grid = &planet.grid;
        let n = grid.n();
        let side = n + 2 * PAD;
        let faces = (0..FACES)
            .map(|face| {
                (0..side * side)
                    .map(|k| {
                        let (x, z) = (
                            (k % side) as i64 - PAD as i64,
                            (k / side) as i64 - PAD as i64,
                        );
                        if x >= 0 && z >= 0 && (x as usize) < n && (z as usize) < n {
                            values[grid.index(face, x as usize, z as usize)]
                        } else {
                            // Off the face: the cell in that direction, on a neighbouring face.
                            let direction = grid.direction_at(face, Vec2::new(x as f32, z as f32));
                            values[grid.cell_at(direction)]
                        }
                    })
                    .collect()
            })
            .collect();
        Self { n, faces }
    }

    fn at(&self, face: usize, x: i64, z: i64) -> f32 {
        let side = (self.n + 2 * PAD) as i64;
        let (x, z) = (
            (x + PAD as i64).clamp(0, side - 1),
            (z + PAD as i64).clamp(0, side - 1),
        );
        self.faces[face][(z * side + x) as usize]
    }

    /// Bicubic (Catmull-Rom) value at continuous cell coordinates of a face (cell centres at
    /// whole numbers).
    pub fn bicubic(&self, face: usize, p: Vec2) -> f32 {
        let cell = p.floor();
        let t = p - cell;
        let (x, z) = (cell.x as i64, cell.y as i64);
        let row = |dz: i64| {
            catmull_rom(
                [
                    self.at(face, x - 1, z + dz),
                    self.at(face, x, z + dz),
                    self.at(face, x + 1, z + dz),
                    self.at(face, x + 2, z + dz),
                ],
                t.x,
            )
        };
        catmull_rom([row(-1), row(0), row(1), row(2)], t.y)
    }

    /// The nearest cell's value at continuous cell coordinates of a face.
    pub fn nearest(&self, face: usize, p: Vec2) -> f32 {
        self.at(face, p.x.round() as i64, p.y.round() as i64)
    }

    /// Bilinear value at continuous cell coordinates of a face.
    pub fn bilinear(&self, face: usize, p: Vec2) -> f32 {
        let cell = p.floor();
        let t = p - cell;
        let (x, z) = (cell.x as i64, cell.y as i64);
        let bottom = self.at(face, x, z) + (self.at(face, x + 1, z) - self.at(face, x, z)) * t.x;
        let top =
            self.at(face, x, z + 1) + (self.at(face, x + 1, z + 1) - self.at(face, x, z + 1)) * t.x;
        bottom + (top - bottom) * t.y
    }
}

/// Bicubic interpolation (Catmull-Rom) through four values, at `t` between the middle two.
fn catmull_rom([p0, p1, p2, p3]: [f32; 4], t: f32) -> f32 {
    0.5 * (2. * p1
        + (p2 - p0) * t
        + (2. * p0 - 5. * p1 + 4. * p2 - p3) * t * t
        + (3. * (p1 - p2) + p3 - p0) * t * t * t)
}
