const pi: f32 = 3.141592653589793;

struct Camera {
    transform: mat4x4f,
    projection: mat4x4f,
    world_position: vec4f,
    clear_color: vec4f,
    ambient_light_color: vec4f,
    point_light_color: vec4f,
    flags: u32,
    // 12 bytes padding
};

struct Instance {
    transform: mat4x4f,
    flags: u32,
    base_vertex: u32,
    outline_thickness: f32,
    // padding 4 bytes
    outline_color: vec4f,
    material: Material,
}

struct Material {
    wireframe: vec4f,
    edges: vec4f,
    albedo: vec4f,
    metallic: f32,
    roughness: f32,
    ambient_occlusion: f32,
    flags: u32,
}

struct PointLight {
    world_position: vec4f,
    color: vec4f,
}

const FLAG_INSTANCE_REVERSE_WINDING: u32 = 0x01;
//const FLAG_INSTANCE_SHOW_SOLID: u32 = 0x02;
//const FLAG_INSTANCE_SHOW_WIREFRAME: u32 = 0x04;
//const FLAG_INSTANCE_SHOW_OUTLINE: u32 = 0x08;
const FLAG_INSTANCE_ENABLE_TEXTURES: u32 = 0x10;
const FLAG_INSTANCE_UV_BUFFER_VALID: u32 = 0x20;
const FLAG_INSTANCE_ENABLE_TRANSPARENCY: u32 = 0x40;

const FLAG_MATERIAL_METALLIC: u32 = 0x01;
const FLAG_MATERIAL_ROUGHNESS: u32 = 0x02;
const FLAG_MATERIAL_AMBIENT_OCCLUSION: u32 = 0x04;

const FLAG_CAMERA_AMBIENT_LIGHT: u32 = 0x01;
const FLAG_CAMERA_POINT_LIGHT: u32 = 0x02;
const FLAG_CAMERA_TONE_MAP: u32 = 0x04;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
}

struct VertexOutputSolid {
    @builtin(position) fragment_position: vec4f,
    @location(0) @interpolate(flat, either) instance_index: u32,
    @location(1) world_position: vec4f,
    @location(2) world_normal: vec4f,
    @location(3) texture_position: vec2f,
}

struct VertexOutputSingleColor {
    @builtin(position) fragment_position: vec4f,
    @location(0) color: vec4f,
}

struct FragmentOutput {
    @location(0) color: vec4f,
}


// camera

@group(0) @binding(0)
var<uniform> camera: Camera;


// instance data

@group(0) @binding(1)
var<storage, read> instance_buffer: array<Instance>;

// this would be for camera-independent point lights
//@group(1) @binding(1)
//var<uniform> point_light: PointLight;


// mesh bindings - used by mesh renderers (solid, wireframe)

@group(1) @binding(0)
var<storage, read> index_buffer: array<u32>;

// note: we interpret this as an array of f32's, otherwise we'll have to pad the vertices in the buffer.
@group(1) @binding(1)
var<storage, read> vertex_buffer: array<f32>;

@group(1) @binding(2)
var<storage, read> uv_buffer: array<vec2f>;

@group(1) @binding(3)
var texture_sampler: sampler;

@group(1) @binding(4)
var texture_albedo: texture_2d<f32>;

@group(1) @binding(5)
var texture_material: texture_2d<f32>;


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
    if (instance.flags & FLAG_INSTANCE_UV_BUFFER_VALID) != 0 {
        output.texture_position = uv_buffer[index_buffer[input.vertex_index]];
    }

    return output;
}

@fragment
fn fs_main_solid(input: VertexOutputSolid, @builtin(front_facing) front_face: bool) -> FragmentOutput {
    // https://learnopengl.com/PBR/Theory
    // https://learnopengl.com/PBR/Lighting

    var output: FragmentOutput;

    if front_face {
        let instance = instance_buffer[input.instance_index];

        // uniform material
        var albedo = instance.material.albedo.rgb;
        var alpha = instance.material.albedo.a;
        var metallic = instance.material.metallic;
        var roughness = instance.material.roughness;
        var ambient_occlusion = instance.material.ambient_occlusion;

        // sample material textures
        if (instance.flags & FLAG_INSTANCE_ENABLE_TEXTURES) != 0 {
            let albedo_and_alpha = textureSample(texture_albedo, texture_sampler, input.texture_position);
            albedo *= albedo_and_alpha.rgb;
            alpha *= albedo_and_alpha.a;
            let material = textureSample(texture_material, texture_sampler, input.texture_position);
            if (instance.material.flags & FLAG_MATERIAL_METALLIC) != 0 {
                metallic *= material.r;
            }
            if (instance.material.flags & FLAG_MATERIAL_ROUGHNESS) != 0 {
                roughness *= material.g;
            }
            if (instance.material.flags & FLAG_MATERIAL_AMBIENT_OCCLUSION) != 0 {
                ambient_occlusion *= material.b;
            }
        }

        // optionally disable transparency
        if (instance.flags & FLAG_INSTANCE_ENABLE_TRANSPARENCY) == 0 {
            alpha = 1.0;
        }

        // some light-independent geometry
        let world_normal = normalize(input.world_normal.xyz);
        let view_direction = normalize(camera.world_position.xyz - input.world_position.xyz);
        //let view_direction = normalize(input.world_position.xyz - camera.world_position.xyz);

        // mix between base reflectivity approximation and surface color (F_0)
        let surface_reflection = mix(vec3(0.04), albedo, metallic);

        // debug
        //alpha = 1.0;
        //ambient_occlusion = 1.0;
        //roughness = 0.8;
        //metallic = 0.5;

        var color: vec3f;

        if (camera.flags & FLAG_CAMERA_AMBIENT_LIGHT) != 0 {
            color += camera.ambient_light_color.rgb * albedo * ambient_occlusion;
        }

        // point light attached to camera
        if (camera.flags & FLAG_CAMERA_POINT_LIGHT) != 0 {
            color += light_radiance(
                camera.world_position.xyz,
                camera.point_light_color.rgb,
                input.world_position.xyz,
                world_normal,
                view_direction,
                albedo,
                roughness,
                metallic,
                surface_reflection,
            );
        }

        // todo: add other point lights

        // tonemap hdr to ldr
        if (camera.flags & FLAG_CAMERA_TONE_MAP) != 0 {
            color /= color + vec3f(1.0);
        }

        output.color = vec4f(color, alpha);
    }
    else {
        // for debugging purposes we'll show back-faces as pink for now
        output.color = vec4f(1.0, 0.0, 1.0, 1.0);
    }

    return output;
}

fn light_radiance(
    light_position: vec3f,
    light_color: vec3f,
    world_position: vec3f,
    world_normal: vec3f,
    view_direction: vec3f,
    albedo: vec3f,
    roughness: f32,
    metallic: f32,
    surface_reflection: vec3f,
) -> vec3f {
    let light_direction = normalize(light_position - world_position);
    let half = normalize(view_direction + light_direction);

    // all the dot products we'll need.
    let h_dot_v = max(dot(half, view_direction), 0.0);
    let n_dot_v = max(dot(world_normal, view_direction), 0.0); // todo: this only needs to be computed once
    let n_dot_l = max(dot(world_normal, light_direction), 0.0);
    let n_dot_h = max(dot(world_normal, half), 0.0);

    // calculate radiance
    //let distance = length(light_position - world_position);
    //let attenuation = 1.0 / (distance * distance);
    let attenuation = 20.0;
    let radiance = light_color * attenuation;

    // cook-torrance brdf
    let ndf = throwbridge_reitz_ggx(n_dot_h, roughness);
    let g = geometry_smith(n_dot_v, n_dot_l, roughness);
    let f = fresnel_schlick(h_dot_v, surface_reflection);

    let k_d = (1.0 - metallic) * (vec3f(1.0) - f);
    let eps = 0.0001;
    let specular = ndf * g * f / (4.0 * n_dot_v * n_dot_l + eps);

    return (k_d * albedo / pi + specular) * radiance * n_dot_l;
}

fn throwbridge_reitz_ggx(n_dot_h: f32, a: f32) -> f32 {
    let a_2 = a * 2;
    let denom = n_dot_h * n_dot_h * (a_2 - 1.0) + 1.0;
    return a_2 / (pi * denom * denom);
}

fn geometry_schlick_ggx(n_dot_x: f32, k: f32) -> f32 {
    return n_dot_x / (n_dot_x * (1.0 - k) + k);
}

fn geometry_smith(n_dot_v: f32, n_dot_l: f32, k: f32) -> f32 {
    let ggx1 = geometry_schlick_ggx(n_dot_v, k);
    let ggx2 = geometry_schlick_ggx(n_dot_l, k);
    return ggx1 * ggx2;
}

fn fresnel_schlick(cos_theta: f32, f_0: vec3f) -> vec3f {
    return f_0 + (vec3f(1.0) - f_0) * pow(1.0 - cos_theta, 5.0);
}

@vertex
fn vs_main_wireframe(input: VertexInput) -> VertexOutputSingleColor {
    let instance = instance_buffer[input.instance_index];

    /*
        0----2
        |   /
        | /
        1

        shader will be called with vertex_index = [0, 1, 2, 3, 4, 5] (2 * number of vertices)

        vertex_index | draw vertex
                   0 | 0
                   1 | 1
                   2 | 1
                   3 | 2
                   4 | 2
                   5 | 0

        `(i + 1) % 6 / 2` gives the vertex indices for lines of a single triangle.
        `(i / 6) * 3` gives the vertex index for the first index of a triangle.
    */

    let vertex_index = ((input.vertex_index + 1) % 6) / 2 + (input.vertex_index / 6) * 3;
    let vertex = get_vertex(vertex_index);

    var output: VertexOutputSingleColor;
    output.color = instance.material.wireframe;
    output.fragment_position = camera.projection * camera.transform * instance.transform * vec4f(vertex, 1.0);

    return output;
}

// note: almost completely identical to vs_main_wireframe
@vertex
fn vs_main_outline(input: VertexInput) -> VertexOutputSingleColor {
    let instance = instance_buffer[input.instance_index];

    let vertex = get_vertex(input.vertex_index);
    let scaling = 1.0 + instance.outline_thickness;

    var output: VertexOutputSingleColor;
    output.color = instance.outline_color;
    output.fragment_position = camera.projection * camera.transform * instance.transform * vec4f(vertex, 1.0 / scaling);

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

    if (flags & FLAG_INSTANCE_REVERSE_WINDING) != 0 {
        // fix vertex order if mesh is wound in opposite order
        output = index -2 * (index % 3 - 1);
    }

    return output;
}
