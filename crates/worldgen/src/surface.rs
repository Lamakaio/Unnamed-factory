//! Sampling the planet's surface between its cells: each cube face as a 2D height grid,
//! padded with the neighbouring faces' cells (found by direction), interpolated bicubically
//! in the face's cell coordinates. The game meshes the planet face by face through it.

use bevy_math::{Vec2, Vec3};

use crate::{
    WorldgenSettings,
    detail::gullies,
    grid::Grid,
    heightmap::catmull_rom,
    noise::{Noise, smoothstep},
    planet::Planet,
    sphere::{FACES, SphereGrid},
};

/// Cells of padding around each face: the bicubic stencil reaches two cells out, and fine
/// detail near a face's edge is also evaluated on the neighbouring face, up to
/// [`EDGE_BLEND`] cells past its edge.
const PAD: usize = 14;
/// Near a face's edge, fine detail blends with the neighbouring face's over this many cells
/// (each face's gullies follow its own cell axes).
const EDGE_BLEND: f32 = 8.;

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

/// The planet's surface for rendering: heights (with fine detail: roughness and gullies, as
/// on the flat map's `detail.rs`) and lake depths between cells.
#[derive(Clone)]
pub struct PlanetSurface {
    pub n: usize,
    /// Radius of the sea level (world units).
    pub radius: f32,
    pub heights: FaceField,
    pub lake_depth: FaceField,
    pub ruggedness: FaceField,
    pub calm: FaceField,
    grid: std::sync::Arc<SphereGrid>,
    seed: u32,
    /// Gully wavelength (cells), octaves and depth (world units), as on the flat map.
    detail_wavelength: f32,
    detail_octaves: u32,
    detail_amplitude: f32,
}

impl PlanetSurface {
    pub fn new(planet: &Planet) -> Self {
        let flat = WorldgenSettings::default();
        Self {
            n: planet.grid.n(),
            radius: planet.radius(),
            heights: FaceField::new(planet, &planet.heights),
            lake_depth: FaceField::new(planet, &planet.lake_depth),
            ruggedness: FaceField::new(planet, &planet.ruggedness),
            calm: FaceField::new(planet, &planet.calm),
            grid: std::sync::Arc::new(SphereGrid::new(planet.grid.n())),
            seed: planet.settings.seed as u32 ^ 0xD37A,
            detail_wavelength: flat.detail_wavelength,
            detail_octaves: flat.detail_octaves,
            detail_amplitude: flat.detail_amplitude,
        }
    }

    /// Height above sea level at continuous cell coordinates of a face: the cells' heights
    /// (bicubic), with fine detail on rugged land.
    pub fn height(&self, face: usize, p: Vec2) -> f32 {
        let height = self.heights.bicubic(face, p);
        if height <= 0. {
            return height;
        }
        let strength = self.ruggedness.bilinear(face, p)
            * (1. - self.calm.bilinear(face, p).clamp(0., 1.));
        if strength <= 0.01 {
            return height;
        }
        let direction = self.direction(face, p);
        let noise = Noise::new(self.seed);
        // Small irregular bumps (3D noise: seamless across faces)...
        let world = direction * (2. * self.n as f32 / std::f32::consts::PI);
        let roughness = noise.fbm3(world / 5., 3) * 0.12;
        // ...and gullies on slopes, blended with the neighbouring face's near an edge.
        let edge = [p.x + 0.5, self.n as f32 - 0.5 - p.x, p.y + 0.5, self.n as f32 - 0.5 - p.y];
        let (nearest, distance) = edge
            .iter()
            .enumerate()
            .fold((0, f32::MAX), |best, (k, &d)| if d < best.1 { (k, d) } else { best });
        let mut gully = self.gullies_on(face, p, world);
        if distance < EDGE_BLEND {
            // The neighbouring face across the nearest edge: the direction just past it.
            let outward = [-Vec2::X, Vec2::X, -Vec2::Y, Vec2::Y][nearest];
            let beyond = self.direction(face, p + outward * (distance + 1.));
            let (other, _) = self.face_coords(beyond);
            if other != face {
                let q = self.grid.coords_on_face(other, direction);
                let own = 0.5 + 0.5 * smoothstep(0., EDGE_BLEND, distance);
                gully = gully * own + self.gullies_on(other, q, world) * (1. - own);
            }
        }
        height + (gully + roughness) * self.detail_amplitude * strength
    }

    /// Gullies (about -1..1, already masked by slope and patchiness) at cell coordinates `p` of
    /// `face`, following the slope of the cells' heights.
    fn gullies_on(&self, face: usize, p: Vec2, world: Vec3) -> f32 {
        let e = 0.5;
        let gradient = Vec2::new(
            self.heights.bicubic(face, p + Vec2::X * e) - self.heights.bicubic(face, p - Vec2::X * e),
            self.heights.bicubic(face, p + Vec2::Y * e) - self.heights.bicubic(face, p - Vec2::Y * e),
        ) / (2. * e);
        let slope_mask = smoothstep(0.12, 0.6, gradient.length());
        if slope_mask <= 0. {
            return 0.;
        }
        let noise = Noise::new(self.seed);
        let patchiness = smoothstep(-0.4, 0.5, noise.layer(1).fbm3(world / 45., 3));
        gullies(p, gradient, self.seed, self.detail_wavelength, self.detail_octaves)
            * slope_mask
            * (0.3 + 0.7 * patchiness)
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

    /// Height above sea level in a direction from the planet's centre.
    pub fn height_at(&self, direction: Vec3) -> f32 {
        let (face, p) = self.face_coords(direction);
        self.height(face, p)
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
