
struct Config {
    size: vec4u,
    strides: vec4u,
    resolution: vec4f,
    time: f32,
    num_sources: u32,
}

@group(0) @binding(0)
var<uniform> config: Config;

struct Cell {
    value: vec3f,
    source_id: u32,
}

@group(0) @binding(1)
var<uniform> projection: Projection;

@group(0) @binding(2)
var<storage, read> field: array<Cell>;


struct Projection {
    transform: mat4x4f,
    color_map: mat4x4f,
}


struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
}

struct VertexOutput {
    @builtin(position) fragment_position: vec4f,
    @location(0) field_position: vec3f,
}

struct FragmentOutput {
    @location(0) color: vec4f,
}


@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    let vertex = quad_vertices[input.vertex_index];

    output.fragment_position = vec4f(vertex * vec2f(2.0) - vec2f(1.0), 0.0, 1.0);
    let projected = projection.transform * vec4f(vertex, 0.0, 1.0);
    output.field_position = projected.xyz * vec3f(config.size.xyz - vec3u(1));

    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> FragmentOutput {
    let point = vec3u(round(input.field_position));
    let index = point_to_index(point);

    let value = field[index].value;
    let color = clamp(projection.color_map * vec4f(value, 1.0), vec4f(0.0), vec4f(1.0));

    return FragmentOutput(color);
}


const quad_vertices: array<vec2f, 6> = array<vec2f, 6>(
    // first tri
    vec2f(0.0, 0.0),
    vec2f(1.0, 0.0),
    vec2f(0.0, 1.0),
    // second tri
    vec2f(1.0, 0.0),
    vec2f(1.0, 1.0),
    vec2f(0.0, 1.0),
);

fn point_to_index(point: vec3u) -> u32 {
    return dot(point, config.strides.xyz);
}
