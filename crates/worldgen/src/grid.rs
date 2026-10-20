//! The grid the simulations run on, as far as flow routing, erosion and smoothing need it: a
//! cell's neighbours and their distances. The planet's is [`crate::sphere::SphereGrid`] (the
//! flat map's square grid is gone; the trait keeps those algorithms apart from the sphere's
//! geometry).

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
            values = crate::par::map(self.len(), |i| {
                let (mut sum, mut count) = (values[i], 1.);
                for (j, distance) in self.neighbours(i) {
                    if distance < 1.2 {
                        sum += values[j];
                        count += 1.;
                    }
                }
                sum / count
            });
        }
        values
    }
}
