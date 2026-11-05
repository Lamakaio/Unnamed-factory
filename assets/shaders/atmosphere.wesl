// The planet's atmosphere seen from outside: a shell a little above the ground, glowing in the
// sky's colour toward the limb (where the line of sight crosses the most air), in a toon band
// with a crisp inner edge, on the day side only. Added over what is behind it.
//
// `sky`: the sky's colour (rgb) and the glow's strength (a).

import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, lights},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sky: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let eye = normalize(view.world_position.xyz - in.world_position.xyz);
    let edge = 1.0 - saturate(dot(n, eye));
    // Two bands: a faint wide one and a brighter one at the limb.
    let w = max(fwidth(edge), 1e-4);
    let glow = 0.35 * smoothstep(0.55 - w, 0.55 + w, edge) + 0.65 * smoothstep(0.8 - w, 0.8 + w, edge);
    let sun = lights.directional_lights[0];
    let day = smoothstep(-0.25, 0.25, dot(n, sun.direction_to_light));
    let amount = glow * day * sky.a;
    // (As bright as lit ground: the terrain's sun level.)
    let sun_level = dot(sun.color.rgb * view.exposure, vec3(0.2126, 0.7152, 0.0722)) / 3.14159265;
    return vec4(sky.rgb * amount * sun_level, amount);
}
