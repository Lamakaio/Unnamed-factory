//! River mouths: the kinds (plain, estuary, delta) and a delta's distributary channels, grown
//! in a plane (see `planet_mouths`, which shapes the planet's mouths; first written for the
//! flat continent).

use bevy_math::{Vec2, Vec3};
use rand::{Rng, RngExt};

use crate::{RiverPoint, noise::Noise};

/// How a river meets the sea.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouth {
    Plain,
    Estuary,
    Delta,
}

/// Spacing of distributary polyline points, in world units (as for rivers).
pub(crate) const SPACING: f32 = 0.5;

pub(crate) struct Channel {
    pub(crate) points: Vec<ChannelPoint>,
    /// Ends in the sea (as opposed to forking, or giving up over land).
    pub(crate) open: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct ChannelPoint {
    pub(crate) position: Vec2,
    pub(crate) width: f32,
    /// Distance from the apex along the channels.
    pub(crate) travelled: f32,
    /// Distance to the channel's tip (infinite where it forks).
    pub(crate) to_tip: f32,
}

/// Grow the distributary tree from the apex.
pub(crate) fn grow_channels(
    rng: &mut impl Rng,
    noise: Noise,
    is_sea: &dyn Fn(Vec2) -> bool,
    apex: RiverPoint,
    direction: Vec2,
    length: f32,
    max_turn: f32,
) -> Vec<Channel> {
    // (start, heading, width, travelled, forks left)
    let forks = if apex.width > 2.5 { 4 } else { 3 };
    let mut stack = vec![(xz(apex.position), direction, apex.width * 0.85, 0f32, forks)];
    let mut channels = Vec::new();
    let mut seed = 0u32;
    while let Some((start, mut heading, width, travelled, forks_left)) = stack.pop() {
        seed += 1;
        let forks = forks_left > 0 && width > 0.7;
        // Where this channel forks, or where it reaches the sea (each tip at its own reach).
        let end = if forks {
            travelled + length * rng.random_range(0.15..0.35f32)
        } else {
            length * rng.random_range(0.75..1.25f32)
        }
        .max(travelled + 2.);
        let wavelength = 10. * width.max(0.6);
        let initial = heading;
        let mut points = Vec::new();
        let mut position = start;
        let mut s = travelled;
        // Tips keep going until they reach the sea (within reason).
        let limit = end + length * 0.6;
        while s <= end || (!forks && !is_sea(position) && s <= limit) {
            points.push(ChannelPoint {
                position,
                width,
                travelled: s,
                to_tip: f32::INFINITY,
            });
            // Wander (meander-like curvature from noise), pulled back toward the channel's
            // initial heading so branches keep fanning out instead of curling back.
            let curvature =
                3. * noise.get(Vec2::new(s / wavelength, seed as f32 * 7.31)) / wavelength;
            let pull = heading.angle_to(initial) * 0.5 / wavelength;
            heading = Vec2::from_angle((curvature + pull) * SPACING)
                .rotate(heading)
                .normalize();
            // Never turn back inland.
            let off = direction.angle_to(heading);
            if off.abs() > max_turn {
                heading = Vec2::from_angle(max_turn * off.signum()).rotate(direction);
            }
            position += heading * SPACING;
            s += SPACING;
        }
        if !forks {
            for point in &mut points {
                point.to_tip = s - point.travelled;
            }
        }
        if forks {
            // Uneven split: one branch takes most of the flow and turns less.
            let share = rng.random_range(0.3..0.5f32);
            let (small, large) = (width * share.sqrt(), width * (1. - share).sqrt());
            let side = if rng.random_bool(0.5) { 1. } else { -1. };
            let small_turn = side * rng.random_range(0.45..0.8f32);
            let large_turn = -side * rng.random_range(0.1..0.35f32);
            let fork_travelled = s;
            stack.push((
                position,
                Vec2::from_angle(small_turn).rotate(heading),
                small,
                fork_travelled,
                forks_left - 1,
            ));
            stack.push((
                position,
                Vec2::from_angle(large_turn).rotate(heading),
                large,
                fork_travelled,
                forks_left - 1,
            ));
        }
        // Tips that never found the sea are dropped.
        let open = !forks && is_sea(position);
        if forks || open {
            channels.push(Channel { points, open });
        }
    }
    channels
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}
