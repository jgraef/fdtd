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
    instance_flags: u32,
    mesh_flags: u32,
    base_vertex: u32,
    outline_thickness: f32,
    outline_color: vec4f,
    material: Material,
}

struct Material {
    wireframe: vec4f,
    edges: vec4f,
    albedo: vec4f,
    metalness: f32,
    roughness: f32,
    ambient_occlusion: f32,
    flags: u32,
    alpha_threshold: f32,
    // 12 bytes padding
}

struct PointLight {
    world_position: vec4f,
    color: vec4f,
}


const FLAG_MESH_UVS                    = 0x00000001;
const FLAG_MESH_NORMALS                = 0x00000002;
const FLAG_MESH_NORMALS_GENERATOR_MASK = 0xff000000;
const FLAG_MESH_NORMALS_FROM_FACE      = 0x01000000;
const FLAG_MESH_NORMALS_FROM_VERTEX    = 0x02000000;

const FLAG_MATERIAL_ALBEDO_TEXTURE: u32            = 0x00000001;
const FLAG_MATERIAL_METALLIC_TEXTURE: u32          = 0x00000002;
const FLAG_MATERIAL_ROUGHNESS_TEXTURE: u32         = 0x00000004;
const FLAG_MATERIAL_AMBIENT_OCCLUSION_TEXTURE: u32 = 0x00000008;
const FLAG_MATERIAL_ANY_ORM: u32                   = 0x0000000e;
const FLAG_MATERIAL_TRANSPARENT: u32               = 0x00000010;

const FLAG_CAMERA_AMBIENT_LIGHT: u32 = 0x01;
const FLAG_CAMERA_POINT_LIGHT: u32 = 0x02;
const FLAG_CAMERA_TONE_MAP: u32 = 0x04;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
}

struct VertexOutputSolid {
    @builtin(position) fragment_position: vec4f,
    @location(0) world_position: vec4f,
    @location(1) world_normal: vec4f,
    @location(2) texture_position: vec2f,
    @location(3) @interpolate(flat, either) instance_index: u32,
    //@location(4) @interpolate(flat, either) vertex_output_flags: u32,
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

@group(1) @binding(1)
var<storage, read> vertex_buffer: array<Vertex>;

@group(1) @binding(2)
var texture_sampler: sampler;

@group(1) @binding(3)
var texture_albedo: texture_2d<f32>;

@group(1) @binding(4)
var texture_material: texture_2d<f32>;

struct Vertex {
    position_uvx: vec4f,
    normal_uvy: vec4f,
}


@vertex
fn vs_main_solid(input: VertexInput) -> VertexOutputSolid {
    let instance = instance_buffer[input.instance_index];

    let index = index_buffer[input.vertex_index];
    let vertex_data = vertex_buffer[index];
    let vertex_position = vertex_data.position_uvx.xyz;
    var vertex_normal = vertex_data.normal_uvy.xyz;
    var vertex_uv = vec2f(vertex_data.position_uvx.w, vertex_data.normal_uvy.w);

    if (instance.mesh_flags & FLAG_MESH_NORMALS) == 0 {
        let generator = instance.mesh_flags & FLAG_MESH_NORMALS_GENERATOR_MASK;

        if generator == FLAG_MESH_NORMALS_FROM_VERTEX {
            vertex_normal = normalize(vertex_position);
        }
        else {
            // FLAG_MESH_NORMALS_FROM_FACE (default)
            let first_face_vertex = (input.vertex_index / 3) * 3;
            let right_neighbor_vertex_index = (input.vertex_index + 1) % 3 + first_face_vertex;
            let left_neighbor_vertex_index = (input.vertex_index + 2) % 3 + first_face_vertex;

            vertex_normal = calculate_normal(
                vertex_position,
                vertex_buffer[index_buffer[right_neighbor_vertex_index]].position_uvx.xyz,
                vertex_buffer[index_buffer[left_neighbor_vertex_index]].position_uvx.xyz,
            );
        }
    }

    var output: VertexOutputSolid;
    output.instance_index = input.instance_index;
    output.world_position = instance.transform * vec4f(vertex_position, 1.0);
    output.world_normal = instance.transform * vec4f(vertex_normal, 0.0);
    output.fragment_position = camera.projection * camera.transform * output.world_position;
    output.texture_position = vertex_uv;

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
        var metalness = instance.material.metalness;
        var roughness = instance.material.roughness;
        var ambient_occlusion = instance.material.ambient_occlusion;

        // sample material textures
        if (instance.material.flags & FLAG_MATERIAL_ALBEDO_TEXTURE) != 0 {
            let albedo_and_alpha = textureSample(texture_albedo, texture_sampler, input.texture_position);
            albedo *= albedo_and_alpha.rgb;
            alpha *= albedo_and_alpha.a;
        }
        if (instance.material.flags & FLAG_MATERIAL_ANY_ORM) != 0 {
            let material = textureSample(texture_material, texture_sampler, input.texture_position);
            if (instance.material.flags & FLAG_MATERIAL_METALLIC_TEXTURE) != 0 {
                metalness *= material.r;
            }
            if (instance.material.flags & FLAG_MATERIAL_ROUGHNESS_TEXTURE) != 0 {
                roughness *= material.g;
            }
            if (instance.material.flags & FLAG_MATERIAL_AMBIENT_OCCLUSION_TEXTURE) != 0 {
                ambient_occlusion *= material.b;
            }
        }

        // discard fragments with alpha below threshold
        if alpha < instance.material.alpha_threshold {
            discard;
        }

        // optionally disable transparency
        if (instance.material.flags & FLAG_MATERIAL_TRANSPARENT) == 0 {
            alpha = 1.0;
        }

        // some light-independent geometry
        let world_normal = normalize(input.world_normal.xyz);
        let view_direction = normalize(camera.world_position.xyz - input.world_position.xyz);
        //let view_direction = normalize(input.world_position.xyz - camera.world_position.xyz);

        // mix between base reflectivity approximation and surface color (F_0)
        let surface_reflection = mix(vec3(0.04), albedo, metalness);

        // this is needed in `light_radiance` but can be computed once upfront
        let n_dot_v = max(dot(world_normal, view_direction), 0.0);

        // surfaces with roughness=0 won't show anything with point lights
        // https://computergraphics.stackexchange.com/a/9126
        const min_roughness: f32 = 0.001;
        roughness = max(roughness, min_roughness);

        // debug
        //alpha = 1.0;
        //ambient_occlusion = 1.0;
        //roughness = 0.8;
        //metalness = 0.5;

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
                metalness,
                surface_reflection,
                n_dot_v,
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
    metalness: f32,
    surface_reflection: vec3f,
    n_dot_v: f32,
) -> vec3f {
    let light_direction = normalize(light_position - world_position);
    let half = normalize(view_direction + light_direction);

    // all the dot products we'll need.
    let h_dot_v = max(dot(half, view_direction), 0.0);
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

    let k_d = (1.0 - metalness) * (vec3f(1.0) - f);
    let eps = 0.0001;
    let specular = ndf * g * f / (4.0 * n_dot_v * n_dot_l + eps);

    // hack for slightly better looks, but we want to better solution for this.
    let specular_clamped = clamp(specular, vec3f(0.01), vec3f(1.0));

    return (k_d * albedo / pi + specular_clamped) * radiance * n_dot_l;
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

    var vertex_index = ((input.vertex_index + 1) % 6) / 2 + (input.vertex_index / 6) * 3;
    vertex_index = index_buffer[vertex_index];
    let vertex_position = vertex_buffer[vertex_index].position_uvx.xyz;

    var output: VertexOutputSingleColor;
    output.color = instance.material.wireframe;
    output.fragment_position = camera.projection * camera.transform * instance.transform * vec4f(vertex_position, 1.0);

    return output;
}

// note: almost completely identical to vs_main_wireframe
@vertex
fn vs_main_outline(input: VertexInput) -> VertexOutputSingleColor {
    let instance = instance_buffer[input.instance_index];

    let vertex_index = index_buffer[input.vertex_index];
    let vertex_position = vertex_buffer[vertex_index].position_uvx.xyz;
    let scaling = 1.0 + instance.outline_thickness;

    var output: VertexOutputSingleColor;
    output.color = instance.outline_color;
    output.fragment_position = camera.projection * camera.transform * instance.transform * vec4f(vertex_position, 1.0 / scaling);

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

fn calculate_normal(v1: vec3f, v2: vec3f, v3: vec3f,) -> vec3f {
    return cross(v2 - v1, v3 - v1);
}
