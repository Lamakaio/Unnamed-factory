//! A river's water line, from the levels along its path (first written for the flat map, used
//! by the planet's water, see `water`): on steep stretches the river runs in steps (pools
//! between falls of varied height and slant, as in mountain streams, and sloping chutes), and
//! the profile carries the flow speed and whitewater for the shader. Also the ribbon geometry
//! constants shared by the water meshes (offsets, domes on steep stretches).

use bevy::prelude::*;
use worldgen::RiverPoint;

/// River ribbons extend this far past the channel edges, under the banks.
pub(super) const RIVER_OVERLAP: f32 = 0.25;

/// Rivers float this much above their water line, to avoid fighting with lakes and the sea.
pub(super) const RIVER_OFFSET: f32 = 0.02;

/// Rivers steeper than this (water line slope) run in steps: pools between small falls.
const CASCADE_SLOPE: f32 = 0.18;

/// Step height of cascades, relative to the channel depth (the pools stay above the bed),
/// varied by up to ±45% per step.
const CASCADE_STEP: f32 = 0.45;

/// Pools keep this fraction of the natural slope (from their lowest point, so the water never
/// stands above the natural water line, which the banks hold), varied per pool; the falls take
/// the rest.
const POOL_TILT: (f32, f32) = (0.15, 0.6);

/// Share of cascade stretches left as sloping chutes (1 to 3 points) between the pools.
const CHUTE_SHARE: f32 = 0.35;

/// Horizontal extent of a fall, relative to its drop (from nearly vertical to a slanted
/// chute, varied per fall).
const FALL_RUN: (f32, f32) = (0.2, 1.4);

/// Steep stretches (water line slope from the first to the second value) get volume: an oval
/// cross-section (see [`Row::dome`]), the water bulging up in the middle and its edges sunk
/// into the banks, which the ribbon cannot follow down steep slopes.
pub(super) const DOME_SLOPE: (f32, f32) = (0.2, 0.6);

/// Height of the oval's top above the water line, and its edges' depth below it, relative to
/// the ribbon's half-width.
pub(super) const DOME_SHAPE: (f32, f32) = (0.3, 1.);

/// Domed stretches are also lowered by this much (relative to the half-width, like
/// [`DOME_SHAPE`]): on the steepest slopes the terrain mesh lags under the carved channel.
pub(super) const DOME_DROP: f32 = 0.35;

/// Segments across a domed cross-section.
pub(super) const DOME_SEGMENTS: usize = 8;

/// Water line and flow of a river, per point.
pub(super) struct ProfilePoint {
    pub(crate) level: f32,
    pub(crate) speed: f32,
    pub(crate) foam: f32,
    /// Height of the fall right after this point (0 if none), and its horizontal extent
    /// relative to the drop.
    pub(crate) drop: f32,
    pub(crate) fall_run: f32,
}

/// A deterministic pseudo-random value in 0..1 for a river point (and a salt).
fn jitter(point: &RiverPoint, salt: u32) -> f32 {
    let mut h = point.position.x.to_bits()
        ^ point.position.z.to_bits().rotate_left(16)
        ^ salt.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 15)).wrapping_mul(0x85eb_ca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

/// The water line of a river and its flow. Steep stretches run in irregular steps: pools (see
/// [`POOL_TILT`]) between falls of varied height and slant, and sloping chutes.
pub(super) fn river_profile(points: &[RiverPoint]) -> Vec<ProfilePoint> {
    let n = points.len();
    // Slope of the natural water line, smoothed over a few points.
    let slope: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(2), (i + 2).min(n - 1));
            let run: f32 = points[a..=b]
                .windows(2)
                .map(|w| (w[1].position - w[0].position).xz().length())
                .sum();
            ((points[a].position.y - points[b].position.y) / run.max(1e-3)).max(0.)
        })
        .collect();
    let mut levels: Vec<f32> = points.iter().map(|p| p.position.y).collect();
    let mut pool_ends = Vec::new();
    let mut i = 0;
    while i < n {
        if slope[i] < CASCADE_SLOPE {
            i += 1;
            continue;
        }
        if jitter(&points[i], 1) < CHUTE_SHARE {
            i += 1 + (jitter(&points[i], 2) * 3.) as usize;
            continue;
        }
        let step = CASCADE_STEP * points[i].depth * (0.55 + 0.9 * jitter(&points[i], 3));
        let tilt = POOL_TILT.0 + (POOL_TILT.1 - POOL_TILT.0) * jitter(&points[i], 4);
        let top = points[i].position.y;
        let mut end = i;
        while end + 1 < n
            && slope[end + 1] >= CASCADE_SLOPE
            && top - points[end + 1].position.y <= step
        {
            end += 1;
        }
        // Too steep for pools (a single point): a continuous cascade, left as a ramp.
        if end == i {
            i += 1;
            continue;
        }
        let bottom = points[end].position.y;
        for (level, point) in levels[i..=end].iter_mut().zip(&points[i..=end]) {
            *level = bottom + tilt * (point.position.y - bottom);
        }
        if end + 1 < n {
            pool_ends.push(end);
        }
        i = end + 1;
    }
    let mut drops = vec![0.; n];
    for end in pool_ends {
        drops[end] = (levels[end] - levels[end + 1]).max(0.);
    }
    // Speed from the slope (narrow streams faster); whitewater on rapids and below falls.
    let mut since_fall = f32::MAX;
    let mut plunge = 0f32;
    (0..n)
        .map(|i| {
            if i > 0 {
                since_fall += (points[i].position - points[i - 1].position).xz().length();
                if drops[i - 1] > 0. {
                    since_fall = 0.;
                    plunge = (drops[i - 1] / 0.25).min(0.9);
                }
            }
            let narrow = (1.3 - 0.1 * points[i].width).clamp(0.6, 1.2);
            let rapids = smoothstep(0.1, 0.6, slope[i]) * 0.35;
            let below_fall = plunge * (-since_fall / (0.12 + points[i].width * 0.2)).exp();
            ProfilePoint {
                level: levels[i],
                speed: (0.35 + 2.5 * slope[i].sqrt()).min(2.5) * narrow,
                foam: rapids.max(below_fall),
                drop: drops[i],
                fall_run: FALL_RUN.0 + (FALL_RUN.1 - FALL_RUN.0) * jitter(&points[i], 5),
            }
        })
        .collect()
}

/// Where the fall after point `i` reaches the next pool, as a fraction of the way to point
/// `i + 1`.
pub(super) fn fall_split(points: &[RiverPoint], profile: &[ProfilePoint], i: usize) -> f32 {
    let run = (points[i + 1].position - points[i].position).xz().length();
    (profile[i].fall_run * profile[i].drop / run.max(1e-3)).min(0.6)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
