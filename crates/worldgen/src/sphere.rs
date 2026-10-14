//! The planet's grid: a cube-sphere. Each face of a cube is an `n × n` square grid, projected
//! onto the sphere *equiangularly* (cells are spaced evenly in angle along each face axis, which
//! keeps their sizes within about ±30% of each other, against ±70% for a plain projection).
//! Within a face, cells are neighbours as on the flat map; across face edges and corners,
//! neighbours are found through their direction on the sphere.
//!
//! Equiangular cube-sphere as in Ronchi, Iacono & Paolucci, "The Cubed Sphere" (1996).

use std::f32::consts::FRAC_PI_4;

use bevy_math::{Vec2, Vec3};

use crate::grid::Grid;

/// Cube faces.
pub const FACES: usize = 6;

/// Each face's normal and its `u` (x) and `v` (z) axes, with `u × v = normal`.
const FACE_AXES: [(Vec3, Vec3, Vec3); FACES] = [
    (Vec3::X, Vec3::NEG_Z, Vec3::Y),
    (Vec3::NEG_X, Vec3::Z, Vec3::Y),
    (Vec3::Y, Vec3::X, Vec3::NEG_Z),
    (Vec3::NEG_Y, Vec3::X, Vec3::Z),
    (Vec3::Z, Vec3::X, Vec3::Y),
    (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
];

/// The 8 neighbour offsets within a face.
const OFFSETS: [(i64, i64); 8] = [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)];

/// A cube-sphere grid of `6 × n × n` cells, indexed `(face * n + z) * n + x`.
pub struct SphereGrid {
    n: usize,
    /// Direction of each cell of a face in the face's frame (x along `u`, y along `v`, z along
    /// the normal): the same for every face.
    local: Vec<Vec3>,
    /// Area of each cell of a face, relative to the mean cell area.
    local_area: Vec<f32>,
    /// Distance (in cells) from each cell of a face to its neighbours within the face, per
    /// [`OFFSETS`] entry: the same for every face, so neighbour lookups away from face edges
    /// need no trigonometry.
    local_distance: Vec<[f32; 8]>,
    /// Mean angle between neighbouring cell centres (radians): a distance of one cell.
    spacing: f32,
}

impl SphereGrid {
    pub fn new(n: usize) -> Self {
        let local = (0..n * n)
            .map(|k| local_direction(n, (k % n) as f32, (k / n) as f32))
            .collect();
        // Solid angle of each cell, from its corners (two spherical triangles).
        let mean = 4. * std::f32::consts::PI / (FACES * n * n) as f32;
        let local_area = (0..n * n)
            .map(|k| {
                let (x, z) = ((k % n) as f32, (k / n) as f32);
                let corner = |dx: f32, dz: f32| local_direction(n, x + dx, z + dz);
                let [a, b, c, d] = [
                    corner(-0.5, -0.5),
                    corner(0.5, -0.5),
                    corner(0.5, 0.5),
                    corner(-0.5, 0.5),
                ];
                (solid_angle(a, b, c) + solid_angle(a, c, d)) / mean
            })
            .collect();
        let spacing = std::f32::consts::FRAC_PI_2 / n as f32;
        let local_distance = (0..n * n)
            .map(|k| {
                let (x, z) = ((k % n) as f32, (k / n) as f32);
                let here = local_direction(n, x, z);
                OFFSETS.map(|(dx, dz)| {
                    angle_between(here, local_direction(n, x + dx as f32, z + dz as f32)) / spacing
                })
            })
            .collect();
        Self {
            n,
            local,
            local_area,
            local_distance,
            spacing,
        }
    }

    /// Cells along a face edge.
    pub fn n(&self) -> usize {
        self.n
    }

    /// The index of cell `(x, z)` of `face`.
    pub fn index(&self, face: usize, x: usize, z: usize) -> usize {
        (face * self.n + z) * self.n + x
    }

    /// The face and coordinates of cell `i`.
    pub fn coords(&self, i: usize) -> (usize, usize, usize) {
        let n = self.n;
        (i / (n * n), i % n, (i / n) % n)
    }

    /// The unit direction from the planet's centre to cell `i`'s centre.
    pub fn direction(&self, i: usize) -> Vec3 {
        let (face, x, z) = self.coords(i);
        let (normal, u, v) = FACE_AXES[face];
        let l = self.local[z * self.n + x];
        u * l.x + v * l.y + normal * l.z
    }

    /// Area of cell `i`, relative to the mean cell area.
    pub fn area(&self, i: usize) -> f32 {
        let (_, x, z) = self.coords(i);
        self.local_area[z * self.n + x]
    }

    /// The face a direction points into, and its continuous cell coordinates on it (cell
    /// centres at whole numbers; within `-0.5..n - 0.5` on the face).
    pub fn face_coords(&self, direction: Vec3) -> (usize, Vec2) {
        let a = direction.abs();
        let face = if a.x >= a.y && a.x >= a.z {
            if direction.x >= 0. { 0 } else { 1 }
        } else if a.y >= a.z {
            if direction.y >= 0. { 2 } else { 3 }
        } else if direction.z >= 0. {
            4
        } else {
            5
        };
        let (normal, u, v) = FACE_AXES[face];
        let depth = direction.dot(normal);
        let tangent = Vec2::new(direction.dot(u), direction.dot(v)) / depth;
        let angle = Vec2::new(tangent.x.atan(), tangent.y.atan()) / FRAC_PI_4;
        (face, (angle + 1.) / 2. * self.n as f32 - 0.5)
    }

    /// The cell containing a direction.
    pub fn cell_at(&self, direction: Vec3) -> usize {
        let (face, p) = self.face_coords(direction);
        let last = (self.n - 1) as f32;
        let (x, z) = (p.x.round().clamp(0., last), p.y.round().clamp(0., last));
        self.index(face, x as usize, z as usize)
    }

    /// Distance between the centres of cells `i` and `j`, in cells.
    pub fn distance(&self, i: usize, j: usize) -> f32 {
        angle_between(self.direction(i), self.direction(j)) / self.spacing
    }
}

impl Grid for SphereGrid {
    fn len(&self) -> usize {
        FACES * self.n * self.n
    }

    fn neighbours(&self, i: usize) -> impl Iterator<Item = (usize, f32)> + '_ {
        let (face, x, z) = self.coords(i);
        let n = self.n as i64;
        let mut found = [(0usize, 0f32); 8];
        let mut count = 0;
        // Away from face edges: plain grid offsets, distances from the table.
        if x > 0 && z > 0 && x + 1 < self.n && z + 1 < self.n {
            let distances = &self.local_distance[z * self.n + x];
            for (k, (dx, dz)) in OFFSETS.into_iter().enumerate() {
                let j = self.index(face, (x as i64 + dx) as usize, (z as i64 + dz) as usize);
                found[k] = (j, distances[k]);
            }
            return found.into_iter().take(8);
        }
        let here = self.direction(i);
        for (dx, dz) in OFFSETS {
            let (nx, nz) = (x as i64 + dx, z as i64 + dz);
            let j = if nx >= 0 && nz >= 0 && nx < n && nz < n {
                self.index(face, nx as usize, nz as usize)
            } else {
                // Across a face edge: the cell in the neighbour's direction, continuing this
                // face's projection past its edge.
                let (normal, u, v) = FACE_AXES[face];
                let l = local_direction(self.n, nx as f32, nz as f32);
                let j = self.cell_at(u * l.x + v * l.y + normal * l.z);
                // (At cube corners a diagonal neighbour can land on a side neighbour.)
                if j == i || found[..count].iter().any(|&(k, _)| k == j) {
                    continue;
                }
                j
            };
            found[count] = (j, angle_between(here, self.direction(j)) / self.spacing);
            count += 1;
        }
        found.into_iter().take(count)
    }
}

/// The direction of continuous cell coordinates `(x, z)` of a face, in the face's frame.
fn local_direction(n: usize, x: f32, z: f32) -> Vec3 {
    let angle = |c: f32| ((c + 0.5) / n as f32 * 2. - 1.) * FRAC_PI_4;
    Vec3::new(angle(x).tan(), angle(z).tan(), 1.).normalize()
}

/// The angle between two unit vectors (accurate for small angles, unlike `acos`).
fn angle_between(a: Vec3, b: Vec3) -> f32 {
    2. * ((a - b).length() / 2.).min(1.).asin()
}

/// Solid angle of the spherical triangle `a b c` (unit vectors), Van Oosterom & Strackee.
fn solid_angle(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    let numerator = a.dot(b.cross(c)).abs();
    let denominator = 1. + a.dot(b) + b.dot(c) + c.dot(a);
    2. * numerator.atan2(denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_round_trip() {
        let grid = SphereGrid::new(32);
        for i in 0..grid.len() {
            assert_eq!(grid.cell_at(grid.direction(i)), i);
        }
    }

    #[test]
    fn areas_average_to_one() {
        let grid = SphereGrid::new(32);
        let mean = (0..grid.len()).map(|i| grid.area(i)).sum::<f32>() / grid.len() as f32;
        assert!((mean - 1.).abs() < 1e-3, "{mean}");
        let (min, max) = (0..grid.len())
            .map(|i| grid.area(i))
            .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
        assert!(min > 0.6 && max < 1.4, "{min} {max}");
    }

    #[test]
    fn neighbours_are_mutual_and_about_one_cell_away() {
        let grid = SphereGrid::new(16);
        let n = grid.n();
        // Cells at a cube corner: three faces meet, and a diagonal neighbour may not be mutual.
        let at_corner = |i: usize| {
            let (_, x, z) = grid.coords(i);
            (x == 0 || x == n - 1) && (z == 0 || z == n - 1)
        };
        for i in 0..grid.len() {
            let neighbours: Vec<(usize, f32)> = grid.neighbours(i).collect();
            assert!(neighbours.len() >= 7, "cell {i}: {neighbours:?}");
            for (j, distance) in neighbours {
                assert!(distance > 0.5 && distance < 1.8, "cell {i} -> {j}: {distance}");
                assert!(
                    at_corner(i) || at_corner(j) || grid.neighbours(j).any(|(k, _)| k == i),
                    "cell {i} -> {j} is not mutual"
                );
            }
        }
    }

    #[test]
    fn every_cell_drains_to_the_sea() {
        // Random heights over a sea at one pole: after Priority-Flood every cell has a
        // downhill path to it.
        let grid = SphereGrid::new(16);
        let heights: Vec<f32> = (0..grid.len())
            .map(|i| crate::noise::hash(7, i as u32, 0) as f32 / u32::MAX as f32)
            .collect();
        let sea: Vec<bool> = (0..grid.len()).map(|i| grid.direction(i).y < -0.9).collect();
        let filled = crate::flow::priority_flood(&grid, &heights, &sea, 1e-5);
        let graph = crate::flow::FlowGraph::new(&grid, &filled, &sea);
        for i in 0..grid.len() {
            let mut cell = i;
            for _ in 0..grid.len() {
                if graph.receivers[cell] == cell {
                    break;
                }
                cell = graph.receivers[cell];
            }
            assert!(sea[cell], "cell {i} ends at {cell}, not the sea");
        }
    }
}
