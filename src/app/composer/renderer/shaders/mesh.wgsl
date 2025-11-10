struct Camera {
    view_matrix: mat4x4f,
    clear_color: vec4f,
};

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var<storage, read> instance_buffer: array<Instance>;

@group(2) @binding(0)
var<storage, read> index_buffer: array<u32>;

// note: we interpret this as an array of f32's, otherwise we'll have to pad the vertices in the buffer.
@group(2) @binding(1)
var<storage, read> vertex_buffer: array<f32>;

struct Instance {
    transform: mat4x4f,
    solid_color: vec4f,
    wireframe_color: vec4f,
    flags: u32,
    base_vertex: u32,
}

const FLAG_REVERSE_WINDING: u32 = 1;
const FLAG_SHOW_SOLID: u32 = 2;
const FLAG_SHOW_WIREFRAME: u32 = 4;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) normal: vec4f,
    @location(1) color: vec4f,
}

struct FragmentOutput {
    @location(0) color: vec4f,
}

@vertex
fn vs_main_solid(input: VertexInput) -> VertexOutput {
    let instance = instance_buffer[input.instance_index];
    let vertex_index = fix_vertex_index(input.vertex_index, instance.flags, instance.base_vertex);
    return vs_main(instance, vertex_index, instance.solid_color);
}

@vertex
fn vs_main_wireframe(input: VertexInput) -> VertexOutput {
    let instance = instance_buffer[input.instance_index];
    let vertex_index = fix_vertex_index(input.vertex_index, instance.flags, instance.base_vertex);
    return vs_main(instance, vertex_index, instance.wireframe_color);
}

fn vs_main(instance: Instance, vertex_index: u32, color: vec4f) -> VertexOutput {
    var output: VertexOutput;

    let vertex = get_vertex(vertex_index);
    let normal = calculate_normal(
        vertex,
        get_vertex((vertex_index + 1) % 3),
        get_vertex((vertex_index + 2) % 3),
    );

    output.position = camera.view_matrix * instance.transform * vec4f(vertex, 1.0);
    output.color = color;
    
    return output;
}

@fragment
fn fs_main(input: VertexOutput, @builtin(front_facing) front_face: bool) -> FragmentOutput {
    var output: FragmentOutput;

    if front_face {
        output.color = input.color;
    }
    else {
        // for debugging purposes we'll show back-faces as pink for now
        output.color = vec4f(1.0, 0.0, 1.0, 1.0);
    }

    return output;
}

fn get_vertex(index: u32) -> vec3f {
    let i = index_buffer[index] * 3;
    return vec3f(
        vertex_buffer[i],
        vertex_buffer[i + 1],
        vertex_buffer[i + 2],
    );
}

fn calculate_normal(v1: vec3f, v2: vec3f, v3: vec3f,) -> vec3f {
    return cross(v2 - v1, v3 - v1);
}

fn fix_vertex_index(index: u32, flags: u32, base_vertex: u32) -> u32 {
    var output = index;

    if (flags & FLAG_REVERSE_WINDING) != 0 {
        // fix vertex order if mesh is wound in opposite order
        output = index -2 * (index % 3 - 1);
    }
    
    output += base_vertex;
    return output;
}
