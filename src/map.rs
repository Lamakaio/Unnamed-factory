use bevy::{
    asset::RenderAssetUsages,
    math::{I64Vec2, NormedVectorSpace},
    platform::collections::HashMap,
    prelude::*,
    render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
};
use kdtree_collisions::{KdTree, KdValue};
use serde::Deserialize;

use crate::{CameraTarget, build::Building, mapgen::Continent, shaders::MapMaterial};
pub struct MapPlugin {
    pub seed: u128,
}
impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Map {
            material: Handle::default(),
            chunks: HashMap::new(),
            entities: KdTree::default(),
            continent: Continent::new_and_generate(self.seed as u32),
        });
        app.add_systems(Update, (spawn_chunk, display_rivers));
        app.add_systems(Startup, setup_map);
    }
}

pub const GRID_SQUARE_SIZE: f32 = 0.5;
/// An instance of a specific building at a position
/// Might contain other instance-specific stats in the future (damage, etc)
#[derive(PartialEq, Clone, Component)]
pub struct BuildingInstance {
    pub building: Handle<Building>,
    pub pos: Vec2,
    pub half_extents: Vec2,
    pub entity: Entity,
}

impl KdValue for BuildingInstance {
    type Position = f32;

    fn min_x(&self) -> Self::Position {
        self.pos.x
    }

    fn min_y(&self) -> Self::Position {
        self.pos.y
    }

    fn max_x(&self) -> Self::Position {
        self.pos.x + self.half_extents.x
    }

    fn max_y(&self) -> Self::Position {
        self.pos.y + self.half_extents.y
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub enum PatchOp {
    Up,
    Down,
    Flatten,
    Smooth,
}

#[derive(Component)]
pub struct ChunkMarker(pub I64Vec2);

/// A chunk, containing terrain data
pub struct Chunk {
    grid: Vec<f32>,
    hydro: Vec<f32>,
    chunk_position: I64Vec2,
    cached_mesh: Option<Handle<Mesh>>,
    spawned: bool,
}

impl Chunk {
    pub const CHUNK_SIZE: u32 = 256;
    pub const WORLD_CHUNK_SIZE: f32 = (Self::CHUNK_SIZE as f32 - 1.) * GRID_SQUARE_SIZE;
    pub const SCALE_Y: f32 = 100.;

    // fn get_noise(seed: u32) -> NoiseT {
    //     //let base_noise = OpenSimplex::new(seed as u32);
    //     Noise {
    //         noise: (
    //             LayeredNoise::new(
    //                 Normed::<f32>::default(),
    //                 Persistence(1.),
    //                 (
    //                     Octave(
    //                         (
    //                             (
    //                                 Scaled(0.1),
    //                                 noiz::prelude::Offset::<
    //                                     MixCellValuesForDomain<OrthoGrid, Smoothstep, SNorm>,
    //                                 > {
    //                                     offset_strength: 0.4,
    //                                     ..Default::default()
    //                                 },
    //                                 BlendCellGradients::<
    //                                     SimplexGrid,
    //                                     SimplecticBlend,
    //                                     QuickGradients,
    //                                 >::default(),
    //                                 NoiseCurve::<TestCurve>::default(),
    //                             ),
    //                             Scaled(0.15),
    //                         ),
    //                     ),
    //                     Octave(Masked(
    //                         (
    //                             LayeredNoise::new(
    //                                 NormedByDerivative::<
    //                                     f32,
    //                                     EuclideanLength,
    //                                     PeakDerivativeContribution,
    //                                 >::default()
    //                                 .with_falloff(0.35),
    //                                 Persistence(0.6),
    //                                 FractalLayers {
    //                                     layer: Octave::<
    //                                         MixCellGradients<
    //                                             OrthoGrid,
    //                                             Smoothstep,
    //                                             QuickGradients,
    //                                             true,
    //                                         >,
    //                                     >::default(),
    //                                     lacunarity: 1.8,
    //                                     amount: 8,
    //                                 },
    //                             ),
    //                             SNormToUNorm::default(),
    //                         ),
    //                         (
    //                             Masked(
    //                                 (
    //                                     Scaled(0.1),
    //                                     BlendCellGradients::<
    //                                         SimplexGrid,
    //                                         SimplecticBlend,
    //                                         QuickGradients,
    //                                     >::default(),
    //                                     SNormToUNorm::default(),
    //                                 ),
    //                                 (
    //                                     Scaled(0.2),
    //                                     PerCellPointDistances::<
    //                                         Voronoi,
    //                                         ManhattanLength,
    //                                         WorleyLeastDistance,
    //                                     >::default(),
    //                                 ),
    //                             ),
    //                             Pow2::default(),
    //                             Scaled (2.)
    //                         ),
    //                     )),
    //                 ),
    //             ),
    //             SNormToUNorm::default(),
    //         ),
    //         seed: NoiseRng(seed),
    //         frequency: 0.04,
    //         ..Default::default()
    //     }
    // }

    /// get a dummy terrain chunk for testing purpose
    fn new_and_generate(pos: &I64Vec2, continent: &Continent) -> Self {
        let mut chunk = Self {
            grid: Vec::with_capacity((Self::CHUNK_SIZE * Self::CHUNK_SIZE) as usize),
            hydro: Vec::with_capacity((Self::CHUNK_SIZE * Self::CHUNK_SIZE) as usize),
            chunk_position: pos.clone(),
            cached_mesh: None,
            spawned: false,
        };
        chunk.generate(continent);
        chunk
    }

    fn generate(&mut self, continent: &Continent) {
        let world_pos = (self.chunk_position * (Self::CHUNK_SIZE as i64 - 1)
            + Continent::CONTINENT_SIZE as i64 / 2)
            .abs()
            % ((Continent::CONTINENT_SIZE - Self::CHUNK_SIZE) as i64);
        self.grid.clear();
        for x in 0..Self::CHUNK_SIZE {
            for z in 0..Self::CHUNK_SIZE {
                let pos = (x + world_pos.x as u32, z + world_pos.y as u32);
                let sample: f32 = continent[pos].height;
                self.grid.push(sample);
                self.hydro.push(continent.get_hydro(pos.0, pos.1).amount);
            }
        }
    }

    /// Get the in-world position of the origin of the chunk.
    pub fn get_world_pos(&self) -> Vec3 {
        Vec3::new(
            self.chunk_position.x as f32,
            0.,
            self.chunk_position.y as f32,
        ) * Self::WORLD_CHUNK_SIZE
    }

    /// Generates the mesh for a chunk.
    // TODO: a way to regenerate mesh on terrain change
    fn make_mesh(&self) -> Mesh {
        let mut vertex_positions = Vec::with_capacity(Self::CHUNK_SIZE.pow(2) as usize);
        let mut uv = Vec::with_capacity(Self::CHUNK_SIZE.pow(2) as usize);
        let mut indices = Vec::with_capacity(((Self::CHUNK_SIZE - 1).pow(2) * 6) as usize);
        let offset = 0.;
        for (i, sq) in self.grid.iter().enumerate() {
            let x = GRID_SQUARE_SIZE * (i as u32 / Self::CHUNK_SIZE) as f32;
            let z = GRID_SQUARE_SIZE * (i as u32 % Self::CHUNK_SIZE) as f32;
            vertex_positions.push([x + offset, sq * Self::SCALE_Y, z + offset]);
            let uv_x = 1.3 * (*sq) - 0.35;
            let uv_y = self.hydro[i];
            //print!("{uv_y} ");
            uv.push([uv_x, uv_y]);
        }
        //println!("");
        for x in 1..Self::CHUNK_SIZE as u16 {
            for z in 1..Self::CHUNK_SIZE as u16 {
                fn id(x: u16, z: u16) -> u16 {
                    z + x * Chunk::CHUNK_SIZE as u16
                }
                //top top left triangle
                indices.extend(&[id(x, z), id(x, z - 1), id(x - 1, z - 1)]);
                //top left left triangle
                indices.extend(&[id(x, z), id(x - 1, z - 1), id(x - 1, z)]);
            }
        }

        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vertex_positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U16(indices))
        .with_computed_smooth_normals()
    }

    /// Get a handle to the mesh of the chunk, generating it on the fly if necessary.
    fn get_mesh(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        if let Some(mesh) = &self.cached_mesh {
            mesh.clone()
        } else {
            let mesh = meshes.add(self.make_mesh());
            self.cached_mesh = Some(mesh.clone());
            mesh
        }
    }

    fn get_mesh_mut<'a>(&mut self, meshes: &'a mut Assets<Mesh>) -> &'a mut Mesh {
        let handle = self.get_mesh(meshes);
        meshes.get_mut(&handle).expect("Mesh not found")
    }

    pub fn get_index(x: i32, y: i32) -> usize {
        x as usize * Chunk::CHUNK_SIZE as usize + y as usize
    }
    pub fn patch(
        &mut self,
        meshes: &mut Assets<Mesh>,
        pos: &Vec3,
        radius: f32,
        operation: PatchOp,
    ) -> Vec<(i64, i64)> {
        let mesh = self.get_mesh_mut(meshes);

        let mut ret = Vec::new();
        {
            let attrs = mesh.attributes_mut();
            let mut attrs = attrs.filter(|(s, _)| {
                s.id == Mesh::ATTRIBUTE_POSITION.id || s.id == Mesh::ATTRIBUTE_UV_0.id
            });
            let fst = attrs.next().unwrap();
            let snd = attrs.next().unwrap();
            let (v_pos, v_uv) = if fst.0.id == Mesh::ATTRIBUTE_POSITION.id {
                (fst.1, snd.1)
            } else {
                (snd.1, fst.1)
            };
            if let (
                VertexAttributeValues::Float32x3(vertex),
                VertexAttributeValues::Float32x2(uvs),
            ) = (v_pos, v_uv)
            {
                let local_pos = (pos - self.get_world_pos()).xz() / GRID_SQUARE_SIZE;
                let radius = radius / GRID_SQUARE_SIZE;
                let mut x_min = (local_pos.x - radius).ceil() as i32;
                let mut x_max = (local_pos.x + radius).floor() as i32;
                let mut y_min = (local_pos.y - radius).ceil() as i32;
                let mut y_max = (local_pos.y + radius).floor() as i32;

                if x_min <= 0 && y_min <= 0 {
                    ret.push((-1, -1));
                }
                if x_max >= Self::CHUNK_SIZE as i32 - 1 && y_max >= Self::CHUNK_SIZE as i32 - 1 {
                    ret.push((1, 1));
                }
                if x_min <= 0 {
                    ret.push((-1, 0));
                    x_min = 0;
                }
                if y_min <= 0 {
                    ret.push((0, -1));
                    y_min = 0;
                }
                if x_max >= Self::CHUNK_SIZE as i32 - 1 {
                    ret.push((1, 0));
                    x_max = Self::CHUNK_SIZE as i32 - 1;
                }
                if y_max >= Self::CHUNK_SIZE as i32 - 1 {
                    ret.push((0, 1));
                    y_max = Self::CHUNK_SIZE as i32 - 1;
                }

                match operation {
                    PatchOp::Up | PatchOp::Down => {
                        let sign = if let PatchOp::Down = operation {
                            -1.
                        } else {
                            1.
                        };
                        for x in x_min..=x_max {
                            for y in y_min..=y_max {
                                let dist = (local_pos - Vec2::new(x as f32, y as f32)).norm();
                                if dist <= radius {
                                    let index = Chunk::get_index(x, y);
                                    let delta = 0.1 * (1. - (dist / radius).powi(4)) * sign;
                                    vertex[index][1] += delta * Self::SCALE_Y;
                                    self.grid[index] += delta;
                                    uvs[index][0] += delta;
                                }
                            }
                        }
                    }
                    PatchOp::Flatten => {
                        for x in x_min..=x_max {
                            for y in y_min..=y_max {
                                let dist = (local_pos - Vec2::new(x as f32, y as f32)).norm();
                                if dist <= radius {
                                    let index =
                                        x as usize * Chunk::CHUNK_SIZE as usize + y as usize;
                                    let ratio = (dist / radius).powi(6);
                                    let height = ratio * vertex[index][1] + (1. - ratio) * pos.y;
                                    vertex[index][1] = height;
                                    self.grid[index] = height / Self::SCALE_Y;
                                    uvs[index][0] = height / Self::SCALE_Y;
                                }
                            }
                        }
                    }
                    PatchOp::Smooth => todo!(),
                }
            }
        }
        mesh.compute_smooth_normals();
        ret
    }
}

/// The whole map. Contains chunks, and a kd-tree of building instances in the map.
#[derive(Resource)]
pub struct Map {
    material: Handle<MapMaterial>,
    pub chunks: HashMap<I64Vec2, Chunk>,
    pub entities: KdTree<BuildingInstance, 10>,
    pub continent: Continent,
}

impl Map {
    /// Get a mutable reference to a chunk (and make/ load it if it doesnt already exists)
    pub fn get_chunk_mut<'a>(&'a mut self, pos: &I64Vec2) -> &'a mut Chunk {
        //Apparently it's the best way to insert an element if it doesnt already exists, and get a mut ref to the result.
        self.chunks
            .raw_entry_mut()
            .from_key(pos)
            .or_insert_with(|| (pos.clone(), Chunk::new_and_generate(pos, &self.continent)))
            .1
    }

    pub fn get_height(&self, pos: Vec3) -> f32 {
        let chunk_pos = (pos / Chunk::WORLD_CHUNK_SIZE).floor();
        let chunk_pos = I64Vec2::new(chunk_pos.x as i64, chunk_pos.z as i64);
        let chunk = self.chunks.get(&chunk_pos);
        if let Some(chunk) = chunk {
            let offset = (pos - chunk.get_world_pos()) / GRID_SQUARE_SIZE;
            let floor = offset.floor();
            let fract = offset.fract();
            let h00 = chunk.grid[Chunk::get_index(floor.x as i32, floor.z as i32)];
            let h01 = chunk.grid[Chunk::get_index(floor.x as i32, floor.z as i32 + 1)];
            let h10 = chunk.grid[Chunk::get_index(floor.x as i32 + 1, floor.z as i32)];
            let h11 = chunk.grid[Chunk::get_index(floor.x as i32 + 1, floor.z as i32 + 1)];
            (h00 * (1. - fract.x.fract()) * (1. - fract.z.fract())
                + h01 * (1. - fract.x.fract()) * fract.z.fract()
                + h10 * fract.x.fract() * (1. - fract.z.fract())
                + h11 * fract.x.fract() * fract.z.fract())
                * Chunk::SCALE_Y
        } else {
            Chunk::SCALE_Y
        }
    }
}

pub fn display_rivers(map: ResMut<Map>, mut gizmos: Gizmos) {
    // for c in &map.continent.river_paths {
    //     let c = c.0.to_curve().unwrap();
    //     let len = c.segments().len();
    //     gizmos.curve_3d(
    //         c,
    //         (0..=200).map(|i| i as f32 / 200. * len as f32),
    //         bevy::color::palettes::css::RED,
    //     );
    // }
    for p in &map.continent.lakes {
        let pos = map.continent.to_world(*p);
        gizmos.sphere(
            Isometry3d::from_translation(pos),
            3.,
            bevy::color::palettes::css::PINK,
        );
    }

    for p in map.continent.to_lake.keys() {
        let pos = map.continent.to_world(*p);
        gizmos.sphere(
            Isometry3d::from_translation(pos),
            1.,
            bevy::color::palettes::css::ORANGE,
        );
    }

    for p in map.continent.to_sea.keys() {
        let pos = map.continent.to_world(*p);
        gizmos.sphere(
            Isometry3d::from_translation(pos),
            1.,
            bevy::color::palettes::css::BLUE,
        );
    }
}

pub fn setup_map(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut map: ResMut<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let mat = asset_server.load("materials/map.mapmat");
    map.material = mat.clone();
    let bottomplanemat = mats.add(StandardMaterial {
        base_color: bevy::color::palettes::css::LIGHT_BLUE.into(),
        ..default()
    });
    commands.spawn((
        Name::new("bottom plane"),
        Mesh3d(
            meshes.add(
                Cuboid::from_size(Vec3::new(100000., 1., 100000.))
                    .mesh()
                    .build(),
            ),
        ),
        MeshMaterial3d(bottomplanemat),
        Transform::from_xyz(0., 0., 0.),
    ));
}
#[derive(Component)]
pub struct IsGround(pub I64Vec2);

/// Handles the spawning of chunks when the camera is close enough. (Currently only spawns the chunk the camera is on)
pub fn spawn_chunk(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut map: ResMut<Map>,
    camera: Query<&CameraTarget, (With<Camera>, Changed<CameraTarget>)>,
) -> Result {
    let camera_transform = camera.single()?;
    let camera_chunk_pos = camera_transform.pos / Chunk::WORLD_CHUNK_SIZE;
    let mat = map.material.clone();
    for (x, z) in [-2., -1., 0., 1.]
        .into_iter()
        .map(|x| [-2., -1., 0., 1.].into_iter().map(move |z| (x, z)))
        .flatten()
    {
        let chunk_pos = I64Vec2::new(
            (camera_chunk_pos.x + x) as i64,
            (camera_chunk_pos.z + z) as i64,
        );
        let chunk = map.get_chunk_mut(&chunk_pos);
        if !chunk.spawned {
            chunk.spawned = true;
            let mesh = chunk.get_mesh(&mut *meshes);
            let mut entity = commands.spawn((
                Name::new(format!("chunk {} {}", chunk_pos.x, chunk_pos.y)),
                Mesh3d(mesh),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(chunk.get_world_pos()),
                IsGround(chunk_pos),
            ));

            // for build in map.entities.query_rect(
            //     chunk_pos.x,
            //     chunk_pos.x + Chunk::CHUNK_SIZE as i64,
            //     chunk_pos.y,
            //     chunk_pos.y + Chunk::CHUNK_SIZE as i64,
            // ) {
            //     let pos = Vec3::new(
            //         (build.grid_pos.x - chunk_pos.x) as f32 * GRID_SQUARE_SIZE,
            //         0.,
            //         (build.grid_pos.y - chunk_pos.y) as f32 * GRID_SQUARE_SIZE,
            //     );
            //     match &build.building.typ {
            //         BuildingType::Single { model } => {
            //             entity.with_child((
            //                 Mesh3d(model.mesh.clone()),
            //                 MeshMaterial3d(build.building.material.clone()),
            //                 Transform::from_translation(pos),
            //             ));
            //         }
            //         BuildingType::Zone { color } => {
            //             entity.with_child((
            //                 // TODO : mesh for zone
            //                 Wireframe,
            //                 WireframeColor {
            //                     color: color.clone(),
            //                 },
            //                 Transform::from_translation(pos).with_scale(Vec3::new(
            //                     build.size.x as f32 * GRID_SQUARE_SIZE,
            //                     0.1,
            //                     build.size.y as f32 * GRID_SQUARE_SIZE,
            //                 )),
            //             ));
            //         }
            //         _ => {}
            //     };
            // }
        }
    }

    Ok(())
}
