//! Bicubic interpolation (Catmull-Rom), for the planet's surface fields.

pub(crate) fn catmull_rom([p0, p1, p2, p3]: [f32; 4], t: f32) -> f32 {
    0.5 * (2. * p1
        + (p2 - p0) * t
        + (2. * p0 - 5. * p1 + 4. * p2 - p3) * t * t
        + (3. * (p1 - p2) + p3 - p0) * t * t * t)
}
