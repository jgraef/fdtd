struct CameraUniform {
    view_matrix: mat4x4f,
    clear_color: vec4f,
};

@group(0) @binding(0)
var<uniform> camera_uniform: CameraUniform;

struct VertexInput {
    @location(0) position: vec3f,
}

struct InstanceInput {
    // the row vectors of the transform matrix
    @location(1) transform_0: vec4f,
    @location(2) transform_1: vec4f,
    @location(3) transform_2: vec4f,
    @location(4) transform_3: vec4f,
    @location(5) solid_color: vec4f,
    @location(6) wireframe_color: vec4f,
}

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) color: vec4f,
}

struct FragmentOutput {
    @location(0) color: vec4f,
}

@vertex
fn vs_main_solid(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    return vs_main(vertex, instance, instance.solid_color);
}

@vertex
fn vs_main_wireframe(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    return vs_main(vertex, instance, instance.wireframe_color);
}

fn vs_main(vertex: VertexInput, instance: InstanceInput, color: vec4f) -> VertexOutput {
    var output: VertexOutput;

    // create the transform matrix from the row-vectors passed in from the instance buffer. they're row vectors, because nalgebra stores matrices in row-major order.
    let model_matrix = transpose(mat4x4f(
        instance.transform_0,
        instance.transform_1,
        instance.transform_2,
        instance.transform_3
    ));

    output.position = camera_uniform.view_matrix * model_matrix * vec4f(vertex.position, 1.0);
    output.color = color;
    
    return output;
}

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    var output: FragmentOutput;

    output.color = in.color;

    return output;
}
