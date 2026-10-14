//! The planet's rivers and lakes: river ribbons along the rivers' water lines, and lake
//! surfaces over the cells under a lake (flat at the lake's level: where they run under the
//! shore, the terrain hides them). A first version: no flow, falls or fades yet (see the flat
//! map's `map/water.rs`).

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use worldgen::{RiverEnd, sphere::FACES, surface::PlanetSurface};

/// River ribbons reach this far past the channel edges, under the banks, and float this much
/// above their water line (to avoid fighting with lakes and the sea).
const RIVER_OVERLAP: f32 = 0.25;
const RIVER_OFFSET: f32 = 0.02;
/// Lake depth from which a cell's corner is under a lake.
const LAKE: f32 = 0.02;

/// River ribbons of the whole planet.
pub fn rivers_mesh(surface: &PlanetSurface) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for river in &surface.rivers {
        let points = &river.points;
        if points.len() < 2 {
            continue;
        }
        // Rivers into the sea stay just under its level at most (they end there).
        let floor = if river.end == RiverEnd::Sea { -0.08 } else { f32::MIN };
        let first = positions.len() as u32;
        for k in 0..points.len() {
            let point = points[k];
            let (before, after) = (points[k.saturating_sub(1)], points[(k + 1).min(points.len() - 1)]);
            let up = point.direction;
            let across = (after.direction - before.direction).cross(up).normalize_or_zero();
            let center = up * (surface.radius + point.level.max(floor) + RIVER_OFFSET);
            let half = point.width / 2. + RIVER_OVERLAP;
            positions.push((center - across * half).to_array());
            positions.push((center + across * half).to_array());
        }
        for k in 0..points.len() as u32 - 1 {
            let (a, b, c, d) = (first + 2 * k, first + 2 * k + 1, first + 2 * k + 2, first + 2 * k + 3);
            indices.extend([a, b, c, b, d, c]);
        }
    }
    water_mesh(positions, indices)
}

/// Lake surfaces of one face: a quad per cell square with a corner under a lake, at the
/// water level of its corners.
pub fn lakes_mesh(surface: &PlanetSurface, face: usize) -> Option<Mesh> {
    let n = surface.n;
    let at = |x: usize, z: usize| Vec2::new(x as f32, z as f32);
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    // (One past the face's edge: the squares across its edges.)
    for z in 0..n {
        for x in 0..n {
            let corners = [at(x, z), at(x + 1, z), at(x, z + 1), at(x + 1, z + 1)];
            let depths = corners.map(|p| surface.lake_depth.bilinear(face, p));
            if depths.iter().all(|&d| d < LAKE) {
                continue;
            }
            let first = positions.len() as u32;
            for (p, depth) in corners.into_iter().zip(depths) {
                let level = surface.heights.bilinear(face, p) + depth;
                positions.push((surface.direction(face, p) * (surface.radius + level)).to_array());
            }
            indices.extend([first, first + 1, first + 2, first + 1, first + 3, first + 2]);
        }
    }
    (!indices.is_empty()).then(|| water_mesh(positions, indices))
}

/// A water mesh: flat-shaded upward (normals along the radius), both sides drawn.
fn water_mesh(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Mesh {
    let normals: Vec<[f32; 3]> = positions
        .iter()
        .map(|p| Vec3::from(*p).normalize_or_zero().to_array())
        .collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
}

/// Spawn the rivers and lakes.
pub fn spawn(
    commands: &mut Commands,
    surface: &PlanetSurface,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.1, 0.35, 0.6, 0.8),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.2,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Name::new("Rivers"),
        Mesh3d(meshes.add(rivers_mesh(surface))),
        MeshMaterial3d(material.clone()),
        Transform::default(),
    ));
    for face in 0..FACES {
        if let Some(mesh) = lakes_mesh(surface, face) {
            commands.spawn((
                Name::new(format!("Lakes {face}")),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                Transform::default(),
            ));
        }
    }
}
