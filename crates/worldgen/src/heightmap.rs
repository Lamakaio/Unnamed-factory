//! A square grid of heights with bicubic sampling.

use bevy_math::Vec2;

/// Square grid of samples centered on the continent origin: sample `(x, z)` is at world
/// `((x - size/2) * cell, (z - size/2) * cell)`. Outside the grid, border values extend.
#[derive(Clone)]
pub struct Heightmap {
    pub size: usize,
    /// Sample spacing, in world units.
    pub cell: f32,
    /// Row-major (`z * size + x`).
    pub values: Vec<f32>,
}

impl Heightmap {
    /// Continuous grid coordinates of a world position.
    pub fn grid_position(&self, p: Vec2) -> Vec2 {
        p / self.cell + (self.size / 2) as f32
    }

    pub fn at(&self, x: i64, z: i64) -> f32 {
        let max = self.size as i64 - 1;
        self.values[z.clamp(0, max) as usize * self.size + x.clamp(0, max) as usize]
    }

    /// Bicubic (Catmull-Rom) interpolation at a world position.
    pub fn bicubic(&self, p: Vec2) -> f32 {
        let g = self.grid_position(p);
        let cell = g.floor();
        let t = g - cell;
        let (x, z) = (cell.x as i64, cell.y as i64);
        let row = |dz: i64| {
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

    /// Bilinear interpolation at a world position.
    pub fn bilinear(&self, p: Vec2) -> f32 {
        let g = self.grid_position(p);
        let cell = g.floor();
        let t = g - cell;
        let (x, z) = (cell.x as i64, cell.y as i64);
        let bottom = self.at(x, z) + (self.at(x + 1, z) - self.at(x, z)) * t.x;
        let top = self.at(x, z + 1) + (self.at(x + 1, z + 1) - self.at(x, z + 1)) * t.x;
        bottom + (top - bottom) * t.y
    }
}

pub fn catmull_rom([p0, p1, p2, p3]: [f32; 4], t: f32) -> f32 {
    0.5 * (2. * p1
        + (p2 - p0) * t
        + (2. * p0 - 5. * p1 + 4. * p2 - p3) * t * t
        + (3. * (p1 - p2) + p3 - p0) * t * t * t)
}
