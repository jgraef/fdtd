struct CameraUniform {
    view_matrix: mat4x4f,
    clear_color: vec4f,
};

@group(0) @binding(0)
var<uniform> camera_uniform: CameraUniform;

struct VertexInput {
    @builtin(vertex_index) index: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) color: vec4f,
}

struct FragmentOutput {
    @location(0) color: vec4f,
}

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    output.position = vec4f(
        f32((vertex.index & 1) << 2) - 1.0,
        f32((vertex.index & 2) << 1) - 1.0,
        0.0,
        1.0,
    );
    
    output.color = camera_uniform.clear_color;
    
    return output;
}

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    var output: FragmentOutput;

    output.color = in.color;

    return output;
}
