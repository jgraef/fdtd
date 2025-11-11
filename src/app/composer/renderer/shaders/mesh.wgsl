struct Camera {
    transform: mat4x4f,
    projection: mat4x4f,
    world_position: vec4f,
    clear_color: vec4f,
    light_filter: LightFilter,
};

struct LightFilter {
    ambient: vec4f,
    diffuse: vec4f,
    specular: vec4f,
    emissive: vec4f,
}

struct Instance {
    transform: mat4x4f,
    flags: u32,
    base_vertex: u32,
    material: Material,
}

struct Material {
    wireframe: vec4f,
    outline: vec4f,
    ambient: vec4f,
    diffuse: vec4f,
    specular: vec4f,
    emissive: vec4f,
    shininess: f32,
}

struct PointLight {
    world_position: vec4f,
    diffuse: vec4f,
    specular: vec4f,
}

const FLAG_REVERSE_WINDING: u32 = 1;
const FLAG_SHOW_SOLID: u32 = 2;
const FLAG_SHOW_WIREFRAME: u32 = 4;
const FLAG_SHOW_OUTLINE: u32 = 8;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
}

struct VertexOutputSolid {
    @builtin(position) fragment_position: vec4f,
    @location(0) @interpolate(flat, either) instance_index: u32,
    @location(1) world_position: vec4f,
    @location(2) world_normal: vec4f,
}

struct VertexOutputSingleColor {
    @builtin(position) fragment_position: vec4f,
    @location(0) color: vec4f,
}

struct FragmentOutput {
    @location(0) color: vec4f,
}


@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var<storage, read> instance_buffer: array<Instance>;

// this would be for camera-independent point lights
//@group(1) @binding(1)
//var<uniform> point_light: PointLight;

@group(2) @binding(0)
var<storage, read> index_buffer: array<u32>;

// note: we interpret this as an array of f32's, otherwise we'll have to pad the vertices in the buffer.
@group(2) @binding(1)
var<storage, read> vertex_buffer: array<f32>;


@vertex
fn vs_main_solid(input: VertexInput) -> VertexOutputSolid {
    let instance = instance_buffer[input.instance_index];

    let first_face_vertex = (input.vertex_index / 3) * 3;
    let right_neighbor_vertex_index = (input.vertex_index + 1) % 3 + first_face_vertex;
    let left_neighbor_vertex_index = (input.vertex_index + 2) % 3 + first_face_vertex;

    let vertex = get_vertex(input.vertex_index);
    let normal = calculate_normal(
        vertex,
        get_vertex(right_neighbor_vertex_index),
        get_vertex(left_neighbor_vertex_index),
    );

    var output: VertexOutputSolid;
    output.instance_index = input.instance_index;
    output.world_position = instance.transform * vec4f(vertex, 1.0);
    output.world_normal = instance.transform * vec4f(normal, 0.0);
    output.fragment_position = camera.projection * camera.transform * output.world_position;
    
    return output;
}

@fragment
fn fs_main_solid(input: VertexOutputSolid, @builtin(front_facing) front_face: bool) -> FragmentOutput {
    var output: FragmentOutput;

    if front_face {
        let instance = instance_buffer[input.instance_index];
        
        // light definition
        // todo: move into buffer
        let point_light = PointLight(
            //vec4f(-10.0, 10.0, -10.0, 1.0),
            camera.world_position,
            vec4f(1.0),
            vec4f(1.0),
        );

        // ambient lighting
        let ambient_color = camera.light_filter.ambient * instance.material.ambient;

        // diffuse lighting
        let world_normal = normalize(input.world_normal.xyz);
        let light_direction = normalize(point_light.world_position.xyz - input.world_position.xyz);
        let diffuse_intensity = max(dot(world_normal, light_direction), 0.0);
        let diffuse_color = diffuse_intensity * camera.light_filter.diffuse * point_light.diffuse * instance.material.diffuse;

        // specular lighting
        let view_direction = normalize(camera.world_position.xyz - input.world_position.xyz);
        let reflect_direction = reflect(-light_direction, world_normal);
        let specular_intensity = pow(max(dot(view_direction, reflect_direction), 0.0), instance.material.shininess);
        let specular_color = specular_intensity * camera.light_filter.specular * point_light.specular * instance.material.specular;

        // emissive lighting
        let emissive_color = camera.light_filter.emissive * instance.material.emissive;

        let final_color = ambient_color + diffuse_color + specular_color + emissive_color;
        output.color = vec4f(final_color.xyz, 1.0);
    }
    else {
        // for debugging purposes we'll show back-faces as pink for now
        output.color = vec4f(1.0, 0.0, 1.0, 1.0);
    }

    return output;
}

@vertex
fn vs_main_wireframe(input: VertexInput) -> VertexOutputSingleColor {
    let instance = instance_buffer[input.instance_index];

    let vertex = get_vertex(input.vertex_index);

    var output: VertexOutputSingleColor;
    output.color = instance.material.wireframe;
    output.fragment_position = camera.projection * camera.transform * instance.transform * vec4f(vertex, 1.0);

    return output;
}


@fragment
fn fs_main_single_color(input: VertexOutputSingleColor) -> FragmentOutput {
    var output: FragmentOutput;
    output.color = input.color;
    return output;
}

@vertex
fn vs_main_clear(input: VertexInput) -> VertexOutputSingleColor {
    var output: VertexOutputSingleColor;

    output.fragment_position = vec4f(
        f32((input.vertex_index & 1) << 2) - 1.0,
        f32((input.vertex_index & 2) << 1) - 1.0,
        1.0, // that's what egui_wgpu clears the depth buffer to
        1.0,
    );

    output.color = camera.clear_color;

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
    
    return output;
}
