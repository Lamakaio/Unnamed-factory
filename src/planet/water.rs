//! The planet's water, drawn with `PlanetWaterMaterial` (`planet_water.wesl`, the flat map's
//! toon water on the sphere):
//! - rivers: ribbons along the water line of the flat map's river profile (`map::water`:
//!   pools and falls of varied slant, sloping chutes, domed steep stretches), with the flow
//!   speed and whitewater for the flow map, fading into the sea or lake they end in, out of the
//!   lake they leave, and into their parent at a junction (joined caps are not ported yet);
//! - lakes: cell squares with a corner under a lake, at the water level;
//! - the sea: a sphere at sea level, built face by face.
//!
//! Patterns use 2D coordinates in vertex colour red/green: along / across a river (so its
//! flow pattern follows it across cube faces), the face's cell coordinates elsewhere.

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use worldgen::{RiverEnd, RiverPoint, sphere::FACES, surface::PlanetSurface};

use crate::{
    map::water::{
        DOME_DROP, DOME_SEGMENTS, DOME_SHAPE, DOME_SLOPE, RIVER_OFFSET, RIVER_OVERLAP, fall_split,
        river_profile,
    },
    shaders::{PlanetWaterMaterial, PlanetWaterShader},
};

/// Lake depth from which a cell's corner is under a lake.
const LAKE: f32 = 0.02;
/// Rivers fade over this distance (a constant plus this many widths) into the water they flow
/// into, and over `FADE_IN` out of the lake they leave.
const RIVER_FADE: (f32, f32) = (1., 2.5);
const FADE_IN: f32 = 1.5;
/// Largest sag of the sea's triangles below sea level (world units).
const SEA_SAG: f32 = 0.02;

/// A mesh being built: positions, pattern coordinates (vertex colour), flow velocity (UV0)
/// and (whitewater, opacity) (UV1).
#[derive(Default)]
struct Water {
    positions: Vec<[f32; 3]>,
    coords: Vec<[f32; 4]>,
    flow: Vec<[f32; 2]>,
    flow_b: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Water {
    fn vertex(&mut self, position: Vec3, coords: Vec2, flow: Vec2, foam: f32, fade: f32) -> u32 {
        self.positions.push(position.to_array());
        self.coords.push([coords.x, coords.y, 0., 1.]);
        self.flow.push(flow.to_array());
        self.flow_b.push([foam, fade]);
        (self.positions.len() - 1) as u32
    }

    fn build(self, rivers: bool) -> Mesh {
        let normals: Vec<[f32; 3]> =
            self.positions.iter().map(|p| Vec3::from(*p).normalize_or_zero().to_array()).collect();
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.coords)
            .with_inserted_indices(Indices::U32(self.indices));
        // (UV1 tells the shader it is a river.)
        if rivers {
            mesh = mesh
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.flow)
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, self.flow_b);
        }
        mesh
    }
}

/// A cross-section of a river ribbon: its centre on the water line, the direction across, its
/// half-width, distance along the river, flow speed, whitewater, opacity and dome.
#[derive(Clone, Copy)]
struct Row {
    center: Vec3,
    across: Vec3,
    half: f32,
    along: f32,
    speed: f32,
    foam: f32,
    fade: f32,
    dome: f32,
}

/// River ribbons of the whole planet.
fn rivers_mesh(surface: &PlanetSurface) -> Mesh {
    let mut water = Water::default();
    for river in &surface.rivers {
        let points = &river.points;
        if points.len() < 3 {
            continue;
        }
        // The river unrolled along its length, for the flat map's profile (which only uses
        // distances along it and levels). Rivers into the sea stay just under its level.
        let floor = if river.end == RiverEnd::Sea { -0.08 } else { f32::MIN };
        let mut along = 0.;
        let unrolled: Vec<RiverPoint> = points
            .iter()
            .enumerate()
            .map(|(k, p)| {
                if k > 0 {
                    along += (p.direction - points[k - 1].direction).length() * surface.radius;
                }
                RiverPoint {
                    position: Vec3::new(along, p.level.max(floor), 0.),
                    width: p.width,
                    depth: p.depth,
                }
            })
            .collect();
        let profile = river_profile(&unrolled);
        let count = points.len();
        let mut rows: Vec<Row> = Vec::with_capacity(count + 8);
        let across_at = |k: usize| {
            let (before, after) = (points[k.saturating_sub(1)], points[(k + 1).min(count - 1)]);
            (after.direction - before.direction).cross(points[k].direction).normalize_or_zero()
        };
        for k in 0..count {
            let up = points[k].direction;
            let here = &profile[k];
            rows.push(Row {
                center: up * (surface.radius + here.level + RIVER_OFFSET),
                across: across_at(k),
                half: points[k].width / 2. + RIVER_OVERLAP,
                along: unrolled[k].position.x,
                speed: here.speed,
                foam: here.foam,
                fade: 1.,
                dome: 0.,
            });
            // A fall: a row a little downstream, at the next pool's level.
            if here.drop > 0. && k + 1 < count {
                let t = fall_split(&unrolled, &profile, k);
                let next = points[k + 1];
                let up = up.lerp(next.direction, t).normalize();
                rows.push(Row {
                    center: up * (surface.radius + here.level - here.drop + RIVER_OFFSET),
                    across: across_at(k),
                    half: points[k].width / 2. + RIVER_OVERLAP,
                    along: unrolled[k].position.x + (unrolled[k + 1].position.x - unrolled[k].position.x) * t,
                    speed: here.speed.max(2.),
                    foam: 1.,
                    fade: 1.,
                    dome: 0.,
                });
            }
        }
        // Volume on steep stretches (slope measured over a few rows), flat at both ends.
        let n = rows.len();
        let levels: Vec<f32> = rows.iter().map(|r| r.center.length()).collect();
        for k in 0..n {
            let (a, b) = (k.saturating_sub(2), (k + 2).min(n - 1));
            let run = (rows[b].along - rows[a].along).max(1e-3);
            let slope = (levels[a] - levels[b]).max(0.) / run;
            let ends = (k.min(n - 1 - k) as f32 / 3.).min(1.);
            rows[k].dome = rows[k].half * smoothstep(DOME_SLOPE.0, DOME_SLOPE.1, slope) * ends;
        }
        // Fades: into the sea or lake at the end (or the parent at a junction), out of the lake
        // at the start.
        let total = rows[n - 1].along;
        let end_fade = match river.end {
            RiverEnd::Junction(_) => points[count - 1].width.max(0.5),
            _ => RIVER_FADE.0 + RIVER_FADE.1 * points[count - 1].width,
        };
        let from_lake = surface.lake_depth_at(points[0].direction) > LAKE;
        for row in &mut rows {
            row.fade *= smoothstep(0., end_fade, total - row.along);
            if from_lake {
                row.fade *= smoothstep(0., FADE_IN, row.along);
            }
        }
        for pair in rows.windows(2) {
            strip(&mut water, &pair[0], &pair[1]);
        }
    }
    water.build(true)
}

/// A ribbon strip between two rows: flat across, or the top of an oval where domed (the edges
/// sunk under the banks, the whole lowered a little: see `map::water`).
fn strip(water: &mut Water, a: &Row, b: &Row) {
    let segments = if a.dome > 0. || b.dome > 0. { DOME_SEGMENTS } else { 1 };
    let first = water.positions.len() as u32;
    for row in [a, b] {
        let up = row.center.normalize();
        for k in 0..=segments {
            let (t, rise) = if segments == 1 {
                (k as f32, 0.)
            } else {
                let angle = std::f32::consts::PI * k as f32 / segments as f32;
                let rise = row.dome
                    * ((DOME_SHAPE.0 + DOME_SHAPE.1) * angle.sin() - DOME_SHAPE.1 - DOME_DROP);
                ((1. - angle.cos()) / 2., rise)
            };
            let offset = (t * 2. - 1.) * row.half;
            let position = row.center + row.across * offset + up * rise;
            water.vertex(
                position,
                Vec2::new(row.along, offset),
                Vec2::new(row.speed, 0.),
                row.foam,
                row.fade,
            );
        }
    }
    let columns = segments as u32 + 1;
    for k in 0..segments as u32 {
        let (a0, a1) = (first + k, first + k + 1);
        let (b0, b1) = (a0 + columns, a1 + columns);
        water.indices.extend([a0, b0, a1, a1, b0, b1]);
    }
}

/// Lake surfaces of one face: a quad per cell square with a corner under a lake, at the
/// water level of its corners.
fn lakes_mesh(surface: &PlanetSurface, face: usize) -> Option<Mesh> {
    let n = surface.n;
    let at = |x: usize, z: usize| Vec2::new(x as f32, z as f32);
    let mut water = Water::default();
    for z in 0..n {
        for x in 0..n {
            let corners = [at(x, z), at(x + 1, z), at(x, z + 1), at(x + 1, z + 1)];
            let depths = corners.map(|p| surface.lake_depth.bilinear(face, p));
            if depths.iter().all(|&d| d < LAKE) {
                continue;
            }
            let first = water.positions.len() as u32;
            for (p, depth) in corners.into_iter().zip(depths) {
                let level = surface.heights.bilinear(face, p) + depth;
                water.vertex(surface.direction(face, p) * (surface.radius + level), p, Vec2::ZERO, 0., 1.);
            }
            water.indices.extend([first, first + 1, first + 2, first + 1, first + 3, first + 2]);
        }
    }
    (!water.indices.is_empty()).then(|| water.build(false))
}

/// The sea over one face: a grid fine enough that its triangles sag less than [`SEA_SAG`].
fn sea_mesh(surface: &PlanetSurface, face: usize) -> Mesh {
    let n = surface.n as f32;
    // A chord of angle θ sags by about R θ² / 8; a face spans about π/2.
    let angle = (8. * SEA_SAG / surface.radius).sqrt();
    let segments = ((std::f32::consts::FRAC_PI_2 / angle).ceil() as usize).max(8);
    let mut water = Water::default();
    for z in 0..=segments {
        for x in 0..=segments {
            let p = Vec2::new(x as f32, z as f32) / segments as f32 * n - 0.5;
            water.vertex(surface.direction(face, p) * surface.radius, p, Vec2::ZERO, 0., 1.);
        }
    }
    let row = segments as u32 + 1;
    for z in 0..segments as u32 {
        for x in 0..segments as u32 {
            let (a, b, c, d) = (z * row + x, z * row + x + 1, (z + 1) * row + x, (z + 1) * row + x + 1);
            water.indices.extend([a, b, c, b, d, c]);
        }
    }
    water.build(false)
}

/// Spawn the sea, rivers and lakes.
pub fn spawn(
    commands: &mut Commands,
    surface: &PlanetSurface,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<PlanetWaterMaterial>,
) {
    let material = materials.add(PlanetWaterMaterial {
        base: StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: PlanetWaterShader {
            tint: LinearRgba::WHITE,
            planet: Vec4::new(surface.radius, 0., 0., 0.),
            wind: Vec4::new(1., 0., 0., 0.),
        },
    });
    let mut spawn = |name: String, mesh: Mesh| {
        commands.spawn((
            Name::new(name),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            NotShadowCaster,
            Pickable::IGNORE,
        ));
    };
    spawn("Rivers".into(), rivers_mesh(surface));
    for face in 0..FACES {
        if let Some(mesh) = lakes_mesh(surface, face) {
            spawn(format!("Lakes {face}"), mesh);
        }
        spawn(format!("Sea {face}"), sea_mesh(surface, face));
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
