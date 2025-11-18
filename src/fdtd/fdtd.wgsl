struct Input {
    @builtin(global_invocation_id) worker_id: vec3u,
    @builtin(num_workgroups) num_workgroups: vec3u,
}

struct Config {
    size: vec3u,
    strides: vec4u,
    resolution: vec4f,
    time: f32,
}

@group(0) @binding(0)
var<uniform> config: Config;

override workgroup_size_x: u32 = 0;
override workgroup_size_y: u32 = 0;
override workgroup_size_z: u32 = 0;

@group(0) @binding(1)
var<storage, read> material_coeff: array<vec4f>;

// H-field update

@group(0) @binding(2)
var<storage, read_write> h_field_next: array<vec3f>;

@group(0) @binding(2)
var<storage, read_write> e_field_next: array<vec3f>;

@group(0) @binding(3)
var<storage, read> h_field_prev: array<vec3f>;

@group(0) @binding(4)
var<storage, read> e_field_prev: array<vec3f>;


@compute @workgroup_size(workgroup_size_x, workgroup_size_y, workgroup_size_z)
fn update_h(input: Input) {
    // calculate cell index
    let index = input_to_index(input);

    // check if our worker is outside of lattice
    if index >= config.strides.w {
        return;
    }

    // calculate point we're operating on
    let x = index_to_x(index);

    // calculate curl
    let dedx = dedi(index, x, 0);
    let dedy = dedi(index, x, 1);
    let dedz = dedi(index, x, 2);
    let e_curl = curl(dedx, dedy, dedz);

    // material coefficients: D_a, D_b
    let coeff = material_coeff[index].zw;

    // todo: pml
    let psi = vec3f(0.0);

    // update rule
    h_field_next[index] = coeff.x * h_field_prev[index] + coeff.y * (-e_curl - m_source(index, x) + psi);
}


@compute @workgroup_size(workgroup_size_x, workgroup_size_y, workgroup_size_z)
fn update_e(input: Input) {
    // calculate cell index
    let index = input_to_index(input);

    // check if our worker is outside of lattice
    if index >= config.strides.w {
        return;
    }

    // calculate point we're operating on
    let x = index_to_x(index);

    // calculate curl
    let dhdx = dhdi(index, x, 0);
    let dhdy = dhdi(index, x, 1);
    let dhdz = dhdi(index, x, 2);
    let h_curl = curl(dhdx, dhdy, dhdz);

    // material coefficients: C_a, C_b
    let coeff = material_coeff[index].xy;

    // todo: pml
    let psi = vec3f(0.0);

    // update rule
    //e_field_next[index] = coeff.x * e_field_prev[index] + coeff.y * (h_curl - j_source(index, x) + psi);
    e_field_next[index] = j_source(index, x);
}


// todo: source
fn m_source(index: u32, x: vec3u) -> vec3f {
    var output: vec3f;
    if x.x == 50 {
        output.y = gaussian_pulse(20.0, 10.0);
    }
    return output;
}

fn j_source(index: u32, x: vec3u) -> vec3f {
    var output: vec3f;
    if x.x == 50 {
        output.z = gaussian_pulse(20.0, 10.0);
    }
    return output;
}

fn gaussian_pulse(time: f32, duration: f32) -> f32 {
    return exp(-pow((config.time - time) / duration, 2));
}


fn curl(dfdx: vec3f, dfdy: vec3f, dfdz: vec3f) -> vec3f {
    return vec3f(
        dfdy.z - dfdz.y,
        dfdz.x - dfdx.z,
        dfdx.y - dfdy.x,
    );
}

fn dedi(index: u32, x: vec3u, axis: u32) -> vec3f {
    if x[axis] > 0 {
        let e1 = e_field_prev[index - config.strides[axis]];
        let e2 = e_field_prev[index];
        return (e2 - e1) / config.resolution[axis];
    }
    else {
        // boundary condition
        return vec3f(0.0);
    }
}

fn dhdi(index: u32, x: vec3u, axis: u32) -> vec3f {
    if x[axis] + 1 < config.size[axis] {
        let h1 = h_field_prev[index];
        let h2 = h_field_prev[index + config.strides[axis]];
        return (h2 - h1) / config.resolution[axis];
    }
    else {
        // boundary condition
        return vec3f(0.0);
    }
}


fn input_to_index(input: Input) -> u32 {
    // this might be borked
    return input.worker_id.x + workgroup_size_z * (input.worker_id.y * workgroup_size_y + input.worker_id.z);
}

fn index_to_x_i(index: u32, axis: u32) -> u32 {
    return index % config.strides[axis + 1] / config.strides[axis];
}

fn index_to_x(index: u32) -> vec3u {
    return vec3u(
        index_to_x_i(index, 0),
        index_to_x_i(index, 1),
        index_to_x_i(index, 2),
    );
}

// todo: not needed
fn strides_from_size(size: vec3u) -> vec4u {
    var strides: vec4u;
    strides.x = 1;
    strides.y = size.x;
    strides.z = strides.y * size.y;
    strides.w = strides.z * size.z;
    return strides;
}
