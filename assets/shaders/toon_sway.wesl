// Vertex shader of swaying vegetation (tall grass, reeds): Bevy's mesh vertex shader
// (bevy_pbr render/mesh.wesl, without morph targets and skinning), with the model bending
// with the wind, more the higher above its base. Gusts and a per-plant phase keep it from
// moving in lockstep. Used with toon.wesl as fragment shader.

import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::globals,
};

// Same wind as the terrain grass (map_material.wesl).
const WIND: vec2<f32> = vec2(0.8, 0.6);

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    );

    // Height above the plant's base, in world units and in model units (metres).
    let origin = world_from_local[3].xyz;
    let height = max(world.y - origin.y, 0.0);
    let scale = max(length(world_from_local[1].xyz), 1e-4);
    let metres = height / scale;
    let time = globals.time;
    let along = dot(origin.xz, WIND);
    let gust = 0.55 + 0.45 * sin(time * 0.36 - along * 0.12);
    let sway = (0.5 + 0.5 * sin(time * 2.4 + along * 0.9 + origin.x * 0.37)) * gust;
    let bend = height * 0.3 * min(metres, 1.5) * sway;
    world = vec4(world.xyz + vec3(WIND.x, 0.0, WIND.y) * bend, 1.0);

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
