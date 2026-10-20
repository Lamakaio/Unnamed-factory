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

    /// The unit direction of continuous cell coordinates `p` of `face` (cell centres at whole
    /// numbers; coordinates past the face's edges continue its projection).
    pub fn direction_at(&self, face: usize, p: Vec2) -> Vec3 {
        let (normal, u, v) = FACE_AXES[face];
        let l = local_direction(self.n, p.x, p.y);
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

    /// Continuous cell coordinates of a direction in `face`'s projection, extended past its
    /// edges (for directions on neighbouring faces, near the edge); the direction must be in
    /// the face's hemisphere.
    pub fn coords_on_face(&self, face: usize, direction: Vec3) -> Vec2 {
        let (normal, u, v) = FACE_AXES[face];
        let depth = direction.dot(normal).max(1e-6);
        let tangent = Vec2::new(direction.dot(u), direction.dot(v)) / depth;
        let angle = Vec2::new(tangent.x.atan(), tangent.y.atan()) / FRAC_PI_4;
        (angle + 1.) / 2. * self.n as f32 - 0.5
    }

    /// A per-cell field at a direction: bilinear between the four nearest cells, across the
    /// face's edges (corners beyond it are the neighbouring face's cells there: clamping at the
    /// edge would leave a seam along the cube's edges).
    pub fn bilinear(&self, values: &[f32], direction: Vec3) -> f32 {
        let (face, p) = self.face_coords(direction);
        let n = self.n as i64;
        let (x0, z0) = (p.x.floor(), p.y.floor());
        let (fx, fz) = (p.x - x0, p.y - z0);
        let at = |x: f32, z: f32| {
            let (xi, zi) = (x as i64, z as i64);
            if (0..n).contains(&xi) && (0..n).contains(&zi) {
                values[self.index(face, xi as usize, zi as usize)]
            } else {
                values[self.cell_at(self.direction_at(face, Vec2::new(x, z)))]
            }
        };
        let top = at(x0, z0) + (at(x0 + 1., z0) - at(x0, z0)) * fx;
        let bottom = at(x0, z0 + 1.) + (at(x0 + 1., z0 + 1.) - at(x0, z0 + 1.)) * fx;
        top + (bottom - top) * fz
    }

    /// `f(cell, angle)` for every cell whose centre is within `angle` (radians) of `center`.
    /// Within a face (the common case) a window of its cells is scanned; a disc crossing a
    /// face's edge is walked breadth first.
    pub fn for_cells_within(&self, center: Vec3, angle: f32, mut f: impl FnMut(usize, f32)) {
        let center = center.normalize();
        let (face, p) = self.face_coords(center);
        // (Equiangular cells are never more than about √2 times smaller than the mean.)
        let half = (angle / self.spacing * 1.5).ceil() as i64 + 1;
        let (x0, z0) = (p.x.round() as i64 - half, p.y.round() as i64 - half);
        let (x1, z1) = (p.x.round() as i64 + half, p.y.round() as i64 + half);
        let n = self.n as i64;
        if x0 >= 0 && z0 >= 0 && x1 < n && z1 < n {
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let i = self.index(face, x as usize, z as usize);
                    let a = angle_between(self.direction(i), center);
                    if a <= angle {
                        f(i, a);
                    }
                }
            }
            return;
        }
        let start = self.cell_at(center);
        let mut seen = std::collections::HashSet::from([start]);
        let mut queue = std::collections::VecDeque::from([start]);
        while let Some(i) = queue.pop_front() {
            let a = angle_between(self.direction(i), center);
            if a <= angle {
                f(i, a);
            }
            if a > angle + 1.5 * self.spacing {
                continue;
            }
            for (j, _) in self.neighbours(i) {
                if seen.insert(j) {
                    queue.push_back(j);
                }
            }
        }
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
    fn every_cell_drains_through_depressions() {
        // The same, routing through the pits without filling them: every cell reaches the
        // sea, and the water surface is the filled one.
        let grid = SphereGrid::new(16);
        let heights: Vec<f32> = (0..grid.len())
            .map(|i| crate::noise::hash(9, i as u32, 0) as f32 / u32::MAX as f32)
            .collect();
        let sea: Vec<bool> = (0..grid.len()).map(|i| grid.direction(i).y < -0.9).collect();
        let (graph, water) = crate::flow::FlowGraph::with_depressions(&grid, &heights, &sea, |_, _| 1.);
        let filled = crate::flow::priority_flood(&grid, &heights, &sea, 0.);
        for i in 0..grid.len() {
            assert!((water[i] - filled[i]).abs() < 1e-6, "cell {i}: {} vs {}", water[i], filled[i]);
            let mut cell = i;
            for _ in 0..grid.len() {
                if graph.receivers[cell] == cell {
                    break;
                }
                cell = graph.receivers[cell];
            }
            assert!(sea[cell], "cell {i} ends at {cell}, not the sea");
        }
        assert_eq!(graph.order.len(), grid.len());
    }

    #[test]
    fn parallel_flood_matches_priority_flood() {
        // The parallel depression filling gives Priority-Flood's surface: exactly without
        // epsilon, within a few epsilon with it (the flats' slope may start a cell further);
        // every cell drains either way.
        let grid = SphereGrid::new(24);
        let heights: Vec<f32> = (0..grid.len())
            .map(|i| crate::noise::hash(11, i as u32, 0) as f32 / u32::MAX as f32)
            .collect();
        let sea: Vec<bool> = (0..grid.len()).map(|i| grid.direction(i).y < -0.8).collect();
        for epsilon in [0., 1e-4] {
            let fast = crate::flow::priority_flood(&grid, &heights, &sea, epsilon);
            let reference = crate::flow::priority_flood_reference(&grid, &heights, &sea, epsilon);
            for i in 0..grid.len() {
                let off = (fast[i] - reference[i]).abs();
                assert!(off < 3. * epsilon + 1e-6, "epsilon {epsilon}, cell {i}: {} vs {}", fast[i], reference[i]);
            }
            if epsilon > 0. {
                let graph = crate::flow::FlowGraph::new(&grid, &fast, &sea);
                for i in 0..grid.len() {
                    let mut cell = i;
                    while graph.receivers[cell] != cell {
                        cell = graph.receivers[cell];
                    }
                    assert!(sea[cell], "cell {i} ends at {cell}, not the sea");
                }
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
