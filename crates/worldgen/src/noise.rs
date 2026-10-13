//! Small, dependency-free 2D and 3D gradient noise and the usual combinators (fBm, ridged,
//! warp). The 3D versions sample the planet's surface by direction, with no seams.

use bevy_math::{Vec2, Vec3};

/// Seeded 2D gradient (Perlin-style) noise.
#[derive(Clone, Copy)]
pub struct Noise {
    seed: u32,
}

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self { seed }
    }

    /// An independent noise derived from this one.
    pub fn layer(self, index: u32) -> Self {
        Self::new(hash(self.seed, index, 0x9E37_79B9))
    }

    fn gradient(self, x: i32, y: i32) -> Vec2 {
        let h = hash(self.seed, x as u32, y as u32);
        let angle = (h as f32 / u32::MAX as f32) * std::f32::consts::TAU;
        Vec2::from_angle(angle)
    }

    /// Gradient noise in about -1..1, with features about one unit apart.
    pub fn get(self, p: Vec2) -> f32 {
        let cell = p.floor();
        let (x, y) = (cell.x as i32, cell.y as i32);
        let f = p - cell;
        let dot = |dx: i32, dy: i32| {
            self.gradient(x + dx, y + dy)
                .dot(f - Vec2::new(dx as f32, dy as f32))
        };
        let u = f * f * f * (f * (f * 6. - 15.) + 10.);
        let bottom = dot(0, 0) + (dot(1, 0) - dot(0, 0)) * u.x;
        let top = dot(0, 1) + (dot(1, 1) - dot(0, 1)) * u.x;
        // Perlin noise in 2D spans about ±0.7; rescale to about ±1.
        (bottom + (top - bottom) * u.y) * 1.4
    }

    /// Fractal Brownian motion in about -1..1.
    pub fn fbm(self, p: Vec2, octaves: u32) -> f32 {
        let (mut sum, mut amplitude, mut total, mut frequency) = (0., 1., 0., 1.);
        for octave in 0..octaves {
            sum += self.layer(octave).get(p * frequency) * amplitude;
            total += amplitude;
            amplitude *= 0.5;
            frequency *= 2.03;
        }
        sum / total
    }

    /// Ridged multifractal in 0..1: sharp crests where the base noise crosses zero.
    pub fn ridged(self, p: Vec2, octaves: u32) -> f32 {
        let (mut sum, mut amplitude, mut total, mut frequency) = (0., 1., 0., 1.);
        let mut weight = 1.;
        for octave in 0..octaves {
            let ridge = 1. - self.layer(octave).get(p * frequency).abs().min(1.);
            let ridge = ridge * ridge * weight;
            weight = (ridge * 2.).clamp(0., 1.);
            sum += ridge * amplitude;
            total += amplitude;
            amplitude *= 0.5;
            frequency *= 2.03;
        }
        sum / total
    }

    /// Domain warp: an offset of up to about `strength` to apply to `p`.
    pub fn warp(self, p: Vec2, octaves: u32, strength: f32) -> Vec2 {
        Vec2::new(
            self.layer(100).fbm(p, octaves),
            self.layer(200).fbm(p, octaves),
        ) * strength
    }
}

impl Noise {
    /// Perlin's 12 cube-edge gradients, picked by hash.
    fn gradient3(self, x: i32, y: i32, z: i32) -> Vec3 {
        const GRADIENTS: [Vec3; 12] = [
            Vec3::new(1., 1., 0.),
            Vec3::new(-1., 1., 0.),
            Vec3::new(1., -1., 0.),
            Vec3::new(-1., -1., 0.),
            Vec3::new(1., 0., 1.),
            Vec3::new(-1., 0., 1.),
            Vec3::new(1., 0., -1.),
            Vec3::new(-1., 0., -1.),
            Vec3::new(0., 1., 1.),
            Vec3::new(0., -1., 1.),
            Vec3::new(0., 1., -1.),
            Vec3::new(0., -1., -1.),
        ];
        let h = hash(hash(self.seed, x as u32, y as u32), z as u32, 0x5BD1_E995);
        GRADIENTS[(h % 12) as usize]
    }

    /// 3D gradient noise in about -1..1, with features about one unit apart.
    pub fn get3(self, p: Vec3) -> f32 {
        let cell = p.floor();
        let (x, y, z) = (cell.x as i32, cell.y as i32, cell.z as i32);
        let f = p - cell;
        let dot = |dx: i32, dy: i32, dz: i32| {
            self.gradient3(x + dx, y + dy, z + dz)
                .dot(f - Vec3::new(dx as f32, dy as f32, dz as f32))
        };
        let u = f * f * f * (f * (f * 6. - 15.) + 10.);
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let face = |dz: i32| {
            let bottom = lerp(dot(0, 0, dz), dot(1, 0, dz), u.x);
            let top = lerp(dot(0, 1, dz), dot(1, 1, dz), u.x);
            lerp(bottom, top, u.y)
        };
        // Perlin noise with edge gradients spans about ±1 in 3D.
        lerp(face(0), face(1), u.z)
    }

    /// 3D fractal Brownian motion in about -1..1.
    pub fn fbm3(self, p: Vec3, octaves: u32) -> f32 {
        let (mut sum, mut amplitude, mut total, mut frequency) = (0., 1., 0., 1.);
        for octave in 0..octaves {
            sum += self.layer(octave).get3(p * frequency) * amplitude;
            total += amplitude;
            amplitude *= 0.5;
            frequency *= 2.03;
        }
        sum / total
    }

    /// 3D ridged multifractal in 0..1.
    pub fn ridged3(self, p: Vec3, octaves: u32) -> f32 {
        let (mut sum, mut amplitude, mut total, mut frequency) = (0., 1., 0., 1.);
        let mut weight = 1.;
        for octave in 0..octaves {
            let ridge = 1. - self.layer(octave).get3(p * frequency).abs().min(1.);
            let ridge = ridge * ridge * weight;
            weight = (ridge * 2.).clamp(0., 1.);
            sum += ridge * amplitude;
            total += amplitude;
            amplitude *= 0.5;
            frequency *= 2.03;
        }
        sum / total
    }

    /// 3D domain warp: an offset of up to about `strength` to apply to `p`.
    pub fn warp3(self, p: Vec3, octaves: u32, strength: f32) -> Vec3 {
        Vec3::new(
            self.layer(100).fbm3(p, octaves),
            self.layer(200).fbm3(p, octaves),
            self.layer(300).fbm3(p, octaves),
        ) * strength
    }
}

/// Integer hash of a seed and two coordinates (MurmurHash3 finalizer constants).
pub fn hash(seed: u32, x: u32, y: u32) -> u32 {
    let mut h = seed ^ x.wrapping_mul(0x27d4_eb2d) ^ y.wrapping_mul(0x1656_67b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

/// Smooth 0 to 1 transition of `x` between `edge0` and `edge1`.
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
