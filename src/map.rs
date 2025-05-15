use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages,
    math::{I64Vec2, IVec2, NormedVectorSpace},
    pbr::{
        ExtendedMaterial, OpaqueRendererMethod,
        wireframe::{Wireframe, WireframeColor},
    },
    platform::collections::HashMap,
    prelude::*,
    render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
};
use kdtree_collisions::{KdTree, KdValue};
use noiz::{
    Noise, SampleableFor,
    cells::SimplexGrid,
    curves::Smoothstep,
    math_noise::NoiseCurve,
    prelude::{
        BlendCellGradients, EuclideanLength, FractalLayers, LayeredNoise, NormedByDerivative,
        Octave, PeakDerivativeContribution, Persistence, QuickGradients, SNormToUNorm,
        SimplecticBlend,
    },
    rng::NoiseRng,
};

use crate::{
    maptext::TerrainShader,
    parts::{Building, BuildingType},
};
pub struct MapPlugin {
    pub seed: u128,
}
impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Map {
            material: Handle::default(),
            chunks: HashMap::new(),
            entities: KdTree::default(),
            noise: Chunk::get_noise(self.seed as u32),
        });
        app.add_systems(Update, spawn_chunk);
        app.add_systems(Startup, setup_map);
    }
}

pub const GRID_SQUARE_SIZE: f32 = 1.;
/// An instance of a specific building at a position
/// Might contain other instance-specific stats in the future (damage, etc)
#[derive(PartialEq, Clone)]
pub struct BuildingInstance {
    building: Arc<Building>,
    grid_pos: I64Vec2,
    size: IVec2,
}

impl KdValue for BuildingInstance {
    type Position = i64;

    fn min_x(&self) -> Self::Position {
        self.grid_pos.x
    }

    fn min_y(&self) -> Self::Position {
        self.grid_pos.y
    }

    fn max_x(&self) -> Self::Position {
        self.grid_pos.x + self.building.size.x
    }

    fn max_y(&self) -> Self::Position {
        self.grid_pos.y + self.building.size.y
    }
}

/// A single point of terrain, with an height.
/// Potentially contains terrain type for texturing (and other stuff ?)
#[derive(Clone)]
pub struct TerrainPoint {
    height: f32,
}

/// A chunk, containing terrain data
pub struct Chunk {
    grid: Vec<TerrainPoint>,
    chunk_position: I64Vec2,
    cached_mesh: Option<Handle<Mesh>>,
    spawned: bool,
}

type NoiseT = Noise<(
    LayeredNoise<
        NormedByDerivative<f32, EuclideanLength, PeakDerivativeContribution>,
        Persistence,
        FractalLayers<
            Octave<BlendCellGradients<SimplexGrid, SimplecticBlend, QuickGradients, true>>,
        >,
    >,
    SNormToUNorm,
    NoiseCurve<Smoothstep>,
)>;

#[derive(Clone, Copy)]
pub enum PatchOp {
    Up,
    Down,
    Flatten,
    Smooth,
}

#[derive(Component)]
pub struct ChunkMarker(pub I64Vec2);

impl Chunk {
    pub const CHUNK_SIZE: u32 = 64;
    pub const WORLD_CHUNK_SIZE: f32 = (Self::CHUNK_SIZE as f32 - 1.) * GRID_SQUARE_SIZE;
    pub const SCALE_Y: f32 = 20.;

    fn get_noise(seed: u32) -> NoiseT {
        //let base_noise = OpenSimplex::new(seed as u32);
        Noise {
            noise: (
                LayeredNoise::new(
                    NormedByDerivative::default().with_falloff(0.5),
                    Persistence(0.7),
                    FractalLayers {
                        layer: Default::default(),
                        lacunarity: 1.6,
                        amount: 4,
                    },
                ),
                Default::default(),
                Default::default(),
            ),
            seed: NoiseRng(seed),
            frequency: 0.005,
        }
    }

    /// get a dummy terrain chunk for testing purpose
    fn new_and_generate(pos: &I64Vec2, noise: &NoiseT) -> Self {
        let mut chunk = Self {
            grid: Vec::with_capacity((Self::CHUNK_SIZE * Self::CHUNK_SIZE) as usize),
            chunk_position: pos.clone(),
            cached_mesh: None,
            spawned: false,
        };
        chunk.generate(noise);
        chunk
    }

    fn generate(&mut self, noise: &NoiseT) {
        let world_pos = self.get_world_pos();
        self.grid.clear();
        for x in 0..Self::CHUNK_SIZE {
            let fx = x as f32 * GRID_SQUARE_SIZE + world_pos.x;
            for z in 0..Self::CHUNK_SIZE {
                let fz = z as f32 * GRID_SQUARE_SIZE + world_pos.z;
                let sample: f32 = noise.sample(Vec2::new(fx, fz));
                self.grid.push(TerrainPoint { height: sample })
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
            vertex_positions.push([x + offset, sq.height * Self::SCALE_Y, z + offset]);
            let uv_x = sq.height;
            let uv_y = (x + z / 50. + rand::random_range(-0.1..0.1)).fract();
            uv.push([uv_x as f32, uv_y]);
        }
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
                let local_pos = (pos - self.get_world_pos()).xz();
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
                                    let index =
                                        x as usize * Chunk::CHUNK_SIZE as usize + y as usize;
                                    let delta = 0.1 * (1. - (dist / radius).powi(4)) * sign;
                                    vertex[index][1] += delta * Self::SCALE_Y;
                                    self.grid[index].height += delta;
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
                                    let ratio = (dist / radius).powi(4);
                                    let height = ratio * vertex[index][1] + (1. - ratio) * pos.y;
                                    vertex[index][1] = height;
                                    self.grid[index].height = height / Self::SCALE_Y;
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
    material: Handle<ExtendedMaterial<StandardMaterial, TerrainShader>>,
    chunks: HashMap<I64Vec2, Chunk>,
    entities: KdTree<BuildingInstance, 10>,
    noise: NoiseT,
}

impl Map {
    /// Get a mutable reference to a chunk (and make/ load it if it doesnt already exists)
    pub fn get_chunk_mut<'a>(&'a mut self, pos: &I64Vec2) -> &'a mut Chunk {
        //Apparently it's the best way to insert an element if it doesnt already exists, and get a mut ref to the result.
        self.chunks
            .raw_entry_mut()
            .from_key(pos)
            .or_insert_with(|| (pos.clone(), Chunk::new_and_generate(pos, &self.noise)))
            .1
    }
}

pub fn setup_map(
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, TerrainShader>>>,
    mut map: ResMut<Map>,
) {
    let text = asset_server.load("img/ZAtoon.png");
    let texture_handle = asset_server.load("img/terrain.png");
    let mat = materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color_texture: Some(texture_handle.clone()),
            // can be used in forward or deferred mode
            opaque_render_method: OpaqueRendererMethod::Auto,
            // in deferred mode, only the PbrInput can be modified (uvs, color and other material properties),
            // in forward mode, the output can also be modified after lighting is applied.
            // see the fragment shader `extended_material.wgsl` for more info.
            // Note: to run in deferred mode, you must also add a `DeferredPrepass` component to the camera and either
            // change the above to `OpaqueRendererMethod::Deferred` or add the `DefaultOpaqueRendererMethod` resource.
            ..Default::default()
        },
        extension: TerrainShader {
            mask: text,
            highlight_color: Srgba::hex("D8C37F").unwrap().into(),
            shadow_color: Srgba::hex("B09070").unwrap().into(),
            rim_color: Color::WHITE.into(),
            grass_color: Srgba::hex("92eb3f").unwrap().into(),
            ocean_color: Srgba::hex("5584f2").unwrap().into(),
            mountain_color: Srgba::hex("544a47").unwrap().into(),
            snow_color: Srgba::hex("f2efe4").unwrap().into(),
            sand_color: Srgba::hex("e0cf96").unwrap().into(),
        },
    });
    map.material = mat
}

/// Handles the spawning of chunks when the camera is close enough. (Currently only spawns the chunk the camera is on)
pub fn spawn_chunk(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut map: ResMut<Map>,
    camera: Query<&Transform, (With<Camera>, Changed<Transform>)>,
) -> Result {
    let camera_transform = camera.single()?;
    let camera_chunk_pos = camera_transform.translation / Chunk::WORLD_CHUNK_SIZE;
    let mat = map.material.clone();
    for (x, z) in [-1., 0., 1.]
        .into_iter()
        .map(|x| [-1., 0., 1.].into_iter().map(move |z| (x, z)))
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
                Mesh3d(mesh),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(chunk.get_world_pos()),
            ));

            for build in map.entities.query_rect(
                chunk_pos.x,
                chunk_pos.x + Chunk::CHUNK_SIZE as i64,
                chunk_pos.y,
                chunk_pos.y + Chunk::CHUNK_SIZE as i64,
            ) {
                let pos = Vec3::new(
                    (build.grid_pos.x - chunk_pos.x) as f32 * GRID_SQUARE_SIZE,
                    0.,
                    (build.grid_pos.y - chunk_pos.y) as f32 * GRID_SQUARE_SIZE,
                );
                match &build.building.typ {
                    BuildingType::Single { model } => {
                        entity.with_child((
                            Mesh3d(model.mesh.clone()),
                            MeshMaterial3d(model.material.clone()),
                            Transform::from_translation(pos),
                        ));
                    }
                    BuildingType::Zone { color } => {
                        entity.with_child((
                            // TODO : mesh for zone
                            Wireframe,
                            WireframeColor {
                                color: color.clone(),
                            },
                            Transform::from_translation(pos).with_scale(Vec3::new(
                                build.size.x as f32 * GRID_SQUARE_SIZE,
                                0.1,
                                build.size.y as f32 * GRID_SQUARE_SIZE,
                            )),
                        ));
                    }
                    _ => {}
                };
            }
        }
    }

    Ok(())
}
