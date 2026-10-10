//! Sea wave field, baked per continent cell into a texture for the water shader
//! (`assets/shaders/water.wesl`):
//!
//! - R: wave travel time to the shore. Waves slow down in shallow water (speed ~ √depth), so the
//!   crests drawn along its isolines bend around headlands into bays and bunch up near the
//!   shore, as real swell does (refraction and shoaling). Measured in world units at deep-water
//!   speed, capped at [`MAX_TRAVEL`].
//! - G: water depth (world units, capped).
//! - B: exposure, 0..1: how much the waves reaching here come with the prevailing wind
//!   (windward coasts get big breakers, leeward ones small ripples).
//! - A: distance to the shore (world units, capped).
//!
//! Travel times come from a Dijkstra search on the cell grid (with knight moves, so fronts are
//! round rather than octagonal), like the fast marching solution of the eikonal equation used
//! for wave fronts (J. Sethian, *Level Set Methods and Fast Marching Methods*; see CREDITS.md).

use std::{cmp::Ordering, collections::BinaryHeap};

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use half::f16;
use worldgen::Continent;

/// Waves reach full (deep-water) speed in water this deep (world units).
const DEEP: f32 = 2.;
/// Slowest wave speed, relative to deep water (keeps shallow flats from stalling the fronts).
const MIN_SPEED: f32 = 0.2;
/// Travel times beyond this are stored as this (the shader fades shore waves out before it).
const MAX_TRAVEL: f32 = 60.;
const MAX_DEPTH: f32 = 30.;
const MAX_DISTANCE: f32 = 60.;
/// Openness (share of sea cells within `OPEN_RADIUS`) from which swell reaches a cell: a
/// straight coast is about 0.5 open, a narrow channel much less.
const OPEN_RADIUS: usize = 6;
const OPEN_WATER: (f32, f32) = (0.3, 0.55);

/// The sea field texture and the mapping from world xz to its coordinates
/// (`uv = xz * zw + xy`).
pub fn sea_texture(continent: &Continent) -> (Image, Vec4) {
    let size = continent.size();
    let cell = continent.settings.cell_size;
    let depth: Vec<f32> = continent.heights.iter().map(|h| (-h).max(0.)).collect();
    let sea: Vec<bool> = continent.heights.iter().map(|&h| h <= 0.).collect();
    let speed: Vec<f32> = depth
        .iter()
        .map(|d| (d / DEEP).sqrt().clamp(MIN_SPEED, 1.))
        .collect();

    // Shore cells: sea next to land. Land cells count as shore too (time 0).
    let mut travel = vec![f32::MAX; size * size];
    let mut heap = BinaryHeap::new();
    for z in 0..size {
        for x in 0..size {
            let i = z * size + x;
            let shore = !sea[i]
                || neighbours(size, x, z, &STEPS[..4]).any(|(j, _)| !sea[j]);
            if shore {
                travel[i] = 0.;
                heap.push(Visit(0., i));
            }
        }
    }
    while let Some(Visit(time, i)) = heap.pop() {
        if time > travel[i] {
            continue;
        }
        let (x, z) = (i % size, i / size);
        for (j, length) in neighbours(size, x, z, &STEPS) {
            if !sea[j] {
                continue;
            }
            let t = time + length * cell * 2. / (speed[i] + speed[j]);
            if t < travel[j] {
                travel[j] = t;
                heap.push(Visit(t, j));
            }
        }
    }
    for t in &mut travel {
        *t = t.min(MAX_TRAVEL);
    }
    // Smooth the remaining grid artifacts, keeping the shore at 0.
    let mut travel = blur(size, &travel, 2);
    for (t, &sea) in travel.iter_mut().zip(&sea) {
        if !sea {
            *t = 0.;
        }
    }

    // Exposure: waves travel down the travel time gradient, towards the shore.
    let wind = Vec2::from_angle(continent.wind_angle);
    let mut exposure = vec![0.; size * size];
    for z in 0..size {
        for x in 0..size {
            let at = |dx: i64, dz: i64| {
                let xx = (x as i64 + dx).clamp(0, size as i64 - 1) as usize;
                let zz = (z as i64 + dz).clamp(0, size as i64 - 1) as usize;
                travel[zz * size + xx]
            };
            let heading = -Vec2::new(at(1, 0) - at(-1, 0), at(0, 1) - at(0, -1));
            let facing = heading.normalize_or_zero().dot(wind);
            exposure[z * size + x] = 0.2 + 0.8 * smoothstep(-0.3, 0.7, facing);
        }
    }
    // Swell hardly gets into narrow channels (river mouths): exposure scales with how much
    // of the surroundings is open water.
    let open: Vec<f32> = sea.iter().map(|&s| if s { 1. } else { 0. }).collect();
    let open = blur(size, &open, OPEN_RADIUS);
    for (e, &o) in exposure.iter_mut().zip(&open) {
        *e *= smoothstep(OPEN_WATER.0, OPEN_WATER.1, o);
    }
    let exposure = blur(size, &blur(size, &exposure, 3), 3);

    let distance = shore_distance(size, &sea);
    let mut data = Vec::with_capacity(size * size * 8);
    for i in 0..size * size {
        for value in [
            travel[i],
            depth[i].min(MAX_DEPTH),
            exposure[i],
            (distance[i] * cell).min(MAX_DISTANCE),
        ] {
            data.extend(f16::from_f32(value).to_le_bytes());
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: size as u32,
            height: size as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::linear()
    });
    // Texel k is centered on world (k - size / 2) * cell.
    let scale = 1. / (cell * size as f32);
    let offset = ((size / 2) as f32 + 0.5) / size as f32;
    (image, Vec4::new(offset, offset, scale, scale))
}

/// Grid steps (dx, dz, length): the 4 sides first, then diagonals and knight moves.
const STEPS: [(i64, i64, f32); 16] = {
    const D: f32 = std::f32::consts::SQRT_2;
    const K: f32 = 2.236_068;
    [
        (1, 0, 1.),
        (-1, 0, 1.),
        (0, 1, 1.),
        (0, -1, 1.),
        (1, 1, D),
        (1, -1, D),
        (-1, 1, D),
        (-1, -1, D),
        (2, 1, K),
        (2, -1, K),
        (-2, 1, K),
        (-2, -1, K),
        (1, 2, K),
        (1, -2, K),
        (-1, 2, K),
        (-1, -2, K),
    ]
};

fn neighbours(
    size: usize,
    x: usize,
    z: usize,
    steps: &[(i64, i64, f32)],
) -> impl Iterator<Item = (usize, f32)> + '_ {
    steps.iter().filter_map(move |&(dx, dz, length)| {
        let (nx, nz) = (x as i64 + dx, z as i64 + dz);
        (nx >= 0 && nz >= 0 && nx < size as i64 && nz < size as i64)
            .then(|| (nz as usize * size + nx as usize, length))
    })
}

/// Dijkstra queue entry, smallest time first.
#[derive(PartialEq)]
struct Visit(f32, usize);

impl Eq for Visit {}

impl Ord for Visit {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0)
    }
}

impl PartialOrd for Visit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Distance (cells) from each sea cell to the nearest land cell (0 on land).
fn shore_distance(size: usize, sea: &[bool]) -> Vec<f32> {
    let mut distance = vec![f32::MAX; size * size];
    let mut heap = BinaryHeap::new();
    for (i, &sea) in sea.iter().enumerate() {
        if !sea {
            distance[i] = 0.;
            heap.push(Visit(0., i));
        }
    }
    while let Some(Visit(d, i)) = heap.pop() {
        if d > distance[i] {
            continue;
        }
        for (j, length) in neighbours(size, i % size, i / size, &STEPS) {
            if d + length < distance[j] {
                distance[j] = d + length;
                heap.push(Visit(d + length, j));
            }
        }
    }
    distance
}

/// Box blur (separable) of a square grid.
fn blur(size: usize, values: &[f32], radius: usize) -> Vec<f32> {
    let pass = |input: &[f32], horizontal: bool| -> Vec<f32> {
        let mut output = vec![0.; input.len()];
        for line in 0..size {
            for k in 0..size {
                let lo = k.saturating_sub(radius);
                let hi = (k + radius).min(size - 1);
                let mut sum = 0.;
                for m in lo..=hi {
                    let i = if horizontal { line * size + m } else { m * size + line };
                    sum += input[i];
                }
                let i = if horizontal { line * size + k } else { k * size + line };
                output[i] = sum / (hi - lo + 1) as f32;
            }
        }
        output
    };
    pass(&pass(values, true), false)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
