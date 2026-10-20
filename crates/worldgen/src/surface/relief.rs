//! Fine relief between the cells: roughness and gullies on rugged land (see `detail`), badlands'
//! deeper gullies, dunes on the sand seas, karst towers and sinkholes.

use bevy_math::{IVec3, Vec2, Vec3};

use super::PlanetSurface;
use crate::{
    detail::gullies,
    noise::{Noise, hash, smoothstep},
};

/// Near a face's edge, fine detail blends with the neighbouring face's over this many cells
/// (each face's gullies follow its own cell axes).
const EDGE_BLEND: f32 = 8.;

/// Dunes: wavelength (world units, at `DUNE_LATITUDE`), height (world units) and where along
/// a wavelength the crest is (the slip face beyond it).
const DUNE_WAVELENGTH: f32 = 5.;
const DUNE_LATITUDE: f32 = 0.45;
const DUNE_HEIGHT: f32 = 0.6;
const DUNE_CREST: f32 = 0.78;

/// Fine detail: gully wavelength (cells), octaves and depth (world units).
const DETAIL_WAVELENGTH: f32 = 8.;
const DETAIL_OCTAVES: u32 = 3;
const DETAIL_AMPLITUDE: f32 = 1.2;

/// Karst: features this far apart (world units), towers this high, sinkholes this deep (world
/// units) round this share of the features.
const KARST_SPACING: f32 = 4.5;
const KARST_TOWER: f32 = 2.6;
const KARST_SINKHOLE: f32 = 0.5;
const KARST_SINKHOLES: f32 = 0.35;

/// Badlands' gullies are this much deeper.
const BADLANDS_GULLIES: f32 = 1.5;

impl PlanetSurface {
    /// The cells' heights with fine detail (no river channels).
    pub(super) fn detailed_height(&self, face: usize, p: Vec2, direction: Vec3) -> f32 {
        let height = self.heights.bicubic(face, p);
        if height <= 0. {
            return height;
        }
        let calm = 1. - self.calm.bilinear(face, p).clamp(0., 1.);
        // Dunes on the sand seas.
        let erg = self.erg.bilinear(face, p);
        let height = if erg > 0.01 {
            height + self.dunes(direction) * erg * calm
        } else {
            height
        };
        // Karst: towers or sinkholes.
        let karst = self.karst.bilinear(face, p);
        let height = if karst.abs() > 0.01 {
            height + self.karst_relief(direction, karst) * calm
        } else {
            height
        };
        // Badlands: dense gullies.
        let badlands = self.badlands.bilinear(face, p);
        let strength = self.ruggedness.bilinear(face, p).max(badlands) * calm;
        if strength <= 0.01 {
            return height;
        }
        let noise = Noise::new(self.seed);
        // Small irregular bumps (3D noise: seamless across faces)...
        let world = direction * (2. * self.n as f32 / std::f32::consts::PI);
        let roughness = noise.fbm3(world / 5., 3) * 0.12;
        // ...and gullies on slopes, blended with the neighbouring face's near an edge.
        let edge = [
            p.x + 0.5,
            self.n as f32 - 0.5 - p.x,
            p.y + 0.5,
            self.n as f32 - 0.5 - p.y,
        ];
        let (nearest, distance) =
            edge.iter().enumerate().fold(
                (0, f32::MAX),
                |best, (k, &d)| if d < best.1 { (k, d) } else { best },
            );
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
        height
            + (gully * (1. + BADLANDS_GULLIES * badlands) + roughness) * DETAIL_AMPLITUDE * strength
    }

    /// Karst relief (world units) in a direction: steep limestone towers with rounded tops
    /// (`karst` positive), or round sinkholes (negative), scattered on a jittered lattice
    /// (`KARST_SPACING`).
    fn karst_relief(&self, direction: Vec3, karst: f32) -> f32 {
        let q = direction * self.radius / KARST_SPACING;
        let cell = q.floor().as_ivec3();
        let seed = self.seed ^ 0x6A25;
        let random = |c: IVec3, k: u32| {
            hash(hash(seed ^ k, c.x as u32, c.y as u32), c.z as u32, k) as f32 / u32::MAX as f32
        };
        let mut relief = 0f32;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let c = cell + IVec3::new(dx, dy, dz);
                    let feature = c.as_vec3() + Vec3::new(random(c, 1), random(c, 2), random(c, 3));
                    let d = feature.distance(q);
                    let size = random(c, 4);
                    if karst > 0. {
                        // Towers, of varied height and girth.
                        let radius = 0.28 + 0.14 * size;
                        let tower = smoothstep(radius, radius * 0.55, d).powf(0.6);
                        relief = relief.max(KARST_TOWER * (0.4 + 0.6 * size) * tower);
                    } else if size < KARST_SINKHOLES {
                        let bowl = smoothstep(0.35, 0., d);
                        relief = relief.min(-KARST_SINKHOLE * bowl * bowl);
                    }
                }
            }
        }
        relief * karst.abs()
    }

    /// Dune height (world units) in a direction: transverse dunes, crests running north–south
    /// (across the trade winds blowing from the east: gentle slopes facing east, steep slip
    /// faces west), sinuous and of varied size. A whole number of dunes round the planet keeps
    /// them seamless.
    fn dunes(&self, direction: Vec3) -> f32 {
        let count =
            (std::f32::consts::TAU * self.radius * DUNE_LATITUDE.cos() / DUNE_WAVELENGTH).round();
        let longitude = direction.z.atan2(direction.x);
        let world = direction * self.radius;
        let noise = Noise::new(self.seed ^ 0xD0E5);
        let warp = 1.2 * noise.fbm3(world / 14., 2) + 2. * noise.layer(1).fbm3(world / 40., 2);
        let along = (-longitude / std::f32::consts::TAU * count + warp).rem_euclid(1.);
        let profile = if along < DUNE_CREST {
            smoothstep(0., DUNE_CREST, along)
        } else {
            1. - smoothstep(DUNE_CREST, 1., along)
        };
        let size = 0.65 + 0.35 * noise.layer(2).fbm3(world / 25., 2);
        DUNE_HEIGHT * profile * size
    }

    /// Gullies (about -1..1, already masked by slope and patchiness) at cell coordinates `p` of
    /// `face`, following the slope of the cells' heights.
    fn gullies_on(&self, face: usize, p: Vec2, world: Vec3) -> f32 {
        let e = 0.5;
        let gradient = Vec2::new(
            self.heights.bicubic(face, p + Vec2::X * e)
                - self.heights.bicubic(face, p - Vec2::X * e),
            self.heights.bicubic(face, p + Vec2::Y * e)
                - self.heights.bicubic(face, p - Vec2::Y * e),
        ) / (2. * e);
        let slope_mask = smoothstep(0.12, 0.6, gradient.length());
        if slope_mask <= 0. {
            return 0.;
        }
        let noise = Noise::new(self.seed);
        let patchiness = smoothstep(-0.4, 0.5, noise.layer(1).fbm3(world / 45., 3));
        gullies(p, gradient, self.seed, DETAIL_WAVELENGTH, DETAIL_OCTAVES)
            * slope_mask
            * (0.3 + 0.7 * patchiness)
    }
}
