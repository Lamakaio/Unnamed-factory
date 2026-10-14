//! Sampling the planet's surface between its cells: each cube face as a 2D height grid,
//! padded with the neighbouring faces' cells (found by direction), interpolated bicubically
//! in the face's cell coordinates. The game meshes the planet face by face through it.

use bevy_math::{Vec2, Vec3};

use crate::{grid::Grid, heightmap::catmull_rom, planet::Planet, sphere::FACES};

/// Cells of padding around each face (the bicubic stencil reaches two cells out).
const PAD: usize = 3;

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
                        let (x, z) = ((k % side) as i64 - PAD as i64, (k / side) as i64 - PAD as i64);
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
        let (x, z) = ((x + PAD as i64).clamp(0, side - 1), (z + PAD as i64).clamp(0, side - 1));
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

/// The planet's surface for rendering: heights and lake depths between cells.
#[derive(Clone)]
pub struct PlanetSurface {
    pub n: usize,
    /// Radius of the sea level (world units).
    pub radius: f32,
    pub heights: FaceField,
    pub lake_depth: FaceField,
    grid: std::sync::Arc<crate::sphere::SphereGrid>,
}

impl PlanetSurface {
    pub fn new(planet: &Planet) -> Self {
        Self {
            n: planet.grid.n(),
            radius: planet.radius(),
            heights: FaceField::new(planet, &planet.heights),
            lake_depth: FaceField::new(planet, &planet.lake_depth),
            grid: std::sync::Arc::new(crate::sphere::SphereGrid::new(planet.grid.n())),
        }
    }

    /// Height above sea level at continuous cell coordinates of a face.
    pub fn height(&self, face: usize, p: Vec2) -> f32 {
        self.heights.bicubic(face, p)
    }

    /// Unit direction of continuous cell coordinates of a face.
    pub fn direction(&self, face: usize, p: Vec2) -> Vec3 {
        self.grid.direction_at(face, p)
    }

    /// World position of the surface at continuous cell coordinates of a face (the planet's
    /// centre at the origin).
    pub fn position(&self, face: usize, p: Vec2) -> Vec3 {
        self.direction(face, p) * (self.radius + self.height(face, p))
    }

    /// The face and cell coordinates of a direction.
    pub fn face_coords(&self, direction: Vec3) -> (usize, Vec2) {
        self.grid.face_coords(direction)
    }

    /// Number of cells (for checks).
    pub fn cells(&self) -> usize {
        self.grid.len()
    }
}
