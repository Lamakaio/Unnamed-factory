//! Grids of cells the simulations run on: the flat square map ([`PlaneGrid`]) and the planet
//! ([`crate::sphere::SphereGrid`]). Flow routing, erosion and smoothing only need a cell's
//! neighbours and their distances, so they are written against [`Grid`].

use crate::flow;

/// Cells with neighbours.
pub trait Grid: Sync {
    /// Number of cells; cells are indexed `0..len()`.
    fn len(&self) -> usize;

    /// The neighbours of cell `i` (up to 8), with their distance in cells (about 1 for
    /// side neighbours, √2 for diagonal ones).
    fn neighbours(&self, i: usize) -> impl Iterator<Item = (usize, f32)> + '_;

    /// Cells on the border of the map, where flow leaves it (none on a planet).
    fn is_border(&self, _i: usize) -> bool {
        false
    }

    /// Box blur of `values`, `radius` cells wide (by default, `radius` passes of averaging
    /// each cell with its side neighbours).
    fn blur(&self, values: &[f32], radius: usize) -> Vec<f32> {
        let mut values = values.to_vec();
        for _ in 0..radius {
            values = (0..self.len())
                .map(|i| {
                    let (mut sum, mut count) = (values[i], 1.);
                    for (j, distance) in self.neighbours(i) {
                        if distance < 1.2 {
                            sum += values[j];
                            count += 1.;
                        }
                    }
                    sum / count
                })
                .collect();
        }
        values
    }
}

/// The flat square map: `size × size` cells, indexed `z * size + x`, open on its border.
#[derive(Clone, Copy)]
pub struct PlaneGrid {
    pub size: usize,
}

impl PlaneGrid {
    pub fn new(size: usize) -> Self {
        Self { size }
    }
}

impl Grid for PlaneGrid {
    fn len(&self) -> usize {
        self.size * self.size
    }

    fn neighbours(&self, i: usize) -> impl Iterator<Item = (usize, f32)> + '_ {
        flow::neighbours(self.size, i)
    }

    fn is_border(&self, i: usize) -> bool {
        let (x, z) = (i % self.size, i / self.size);
        x == 0 || z == 0 || x == self.size - 1 || z == self.size - 1
    }

    fn blur(&self, values: &[f32], radius: usize) -> Vec<f32> {
        crate::terrain::blur(self.size, values, radius)
    }
}
