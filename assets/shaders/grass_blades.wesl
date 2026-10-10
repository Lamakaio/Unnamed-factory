// Vertex shader of the grass blades drawn as geometry close to the camera (`map::grass`):
// Bevy's mesh vertex shader (bevy_pbr render/mesh.wesl, without morph targets and skinning),
// with each blade bending downwind by UV0.x (0 at the base, 1 at the tip), squared. The wind,
// its gusts and the sway waves are those of the terrain's grass carpet (map_material.wesl),
// so the blades move with the carpet around them. Used with toon.wesl as fragment shader.

import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::globals,
};

// Prevailing wind direction (x, z).
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> wind: vec4<f32>;

// As in map_material.wesl.
const GUST_SPEED: f32 = 3.0;
// How far a blade tip bends (world units) per unit of bend.
const BEND: f32 = 0.035;

fn hash(c: vec2<i32>) -> f32 {
    var h = (bitcast<u32>(c.x) * 0x27d4eb2du) ^ (bitcast<u32>(c.y) * 0x165667b1u);
    h = (h ^ (h >> 15u)) * 0x85ebca6bu;
    h = (h ^ (h >> 13u)) * 0xc2b2ae35u;
    h = h ^ (h >> 16u);
    return f32(h) / 4294967295.0;
}

fn value_noise(p: vec2<f32>) -> f32 {
    let cell = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = hash(cell);
    let b = hash(cell + vec2(1, 0));
    let c = hash(cell + vec2(0, 1));
    let d = hash(cell + vec2(1, 1));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 2.0 - 1.0;
}

// map_material.wesl's fbm with 2 octaves and no footprint fade.
fn fbm2(p: vec2<f32>) -> f32 {
    return (value_noise(p) * 0.5 + value_noise(p * 2.03 + vec2(17.31, 9.17)) * 0.25) * 1.2;
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    );

    let time = globals.time;
    let xz = world.xz;
    let along = dot(xz, wind.xy);
    let across = dot(xz, vec2(-wind.y, wind.x));
    let gust = smoothstep(0.05, 0.3,
        fbm2(vec2(along * 0.12 - time * GUST_SPEED * 0.12, across * 0.05)));
    let sway = 0.6 * sin(time * 3.1 + along * 3.0 + 0.8 * sin(across * 1.7))
        + 0.3 * sin(time * 5.3 + along * 7.0 + across * 2.0);
@if(VERTEX_UVS_A) {
    let tip = vertex.uv.x * vertex.uv.x;
    let bend = (0.6 + 2.2 * gust + sway) * BEND * tip;
    // Bent blades also get a little shorter.
    world = vec4(world.xyz + vec3(wind.x * bend, -abs(bend) * 0.3, wind.y * bend), 1.0);
}

    out.world_position = world;
    out.position = position_world_to_clip(world.xyz);
@if(VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_in.instance_index
    );
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex_in.instance_index
    );
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index,
        world_from_local[3]
    );
    return out;
}
