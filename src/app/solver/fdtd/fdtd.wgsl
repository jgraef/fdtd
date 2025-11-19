struct Input {
    @builtin(global_invocation_id) worker_id: vec3u,
    @builtin(num_workgroups) num_workgroups: vec3u,
}

struct Config {
    size: vec4u,
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

// note: our H and E field buffers will align the elements to 16 bytes anyway,
// so we can use the 4 extra bytes to indicate if a source current is present.
struct Cell {
    value: vec3f,
    source_id: u32,
}

@group(0) @binding(2)
var<storage, read_write> h_field_next: array<Cell>;

@group(0) @binding(2)
var<storage, read_write> e_field_next: array<Cell>;

@group(0) @binding(3)
var<storage, read> h_field_prev: array<Cell>;

@group(0) @binding(4)
var<storage, read> e_field_prev: array<Cell>;


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
    h_field_next[index].value = coeff.x * h_field_prev[index].value + coeff.y * (-e_curl - m_source(index, x) + psi);
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
    e_field_next[index].value = coeff.x * e_field_prev[index].value + coeff.y * (h_curl - j_source(index, x) + psi);
}


// todo: source
fn m_source(index: u32, x: vec3u) -> vec3f {
    var output: vec3f;
    if x.x == 50 {
        output.z = gaussian_pulse(20.0, 10.0);
    }
    return output;
}

fn j_source(index: u32, x: vec3u) -> vec3f {
    var output: vec3f;
    if x.x == 50 {
        output.y = gaussian_pulse(20.0, 10.0);
    }
    return output;
}

fn gaussian_pulse(time: f32, duration: f32) -> f32 {
    return exp(-pow((config.time - time) / duration, 2.0));
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
        let e1 = e_field_prev[index - config.strides[axis]].value;
        let e2 = e_field_prev[index].value;
        return (e2 - e1) / config.resolution[axis];
    }
    else {
        // boundary condition
        return vec3f(0.0);
    }
}

fn dhdi(index: u32, x: vec3u, axis: u32) -> vec3f {
    if x[axis] + 1 < config.size[axis] {
        let h1 = h_field_prev[index].value;
        let h2 = h_field_prev[index + config.strides[axis]].value;
        return (h2 - h1) / config.resolution[axis];
    }
    else {
        // boundary condition
        return vec3f(0.0);
    }
}

fn input_to_index(input: Input) -> u32 {
    return input.worker_id.x + input.num_workgroups.x * workgroup_size_x * (input.worker_id.y + input.num_workgroups.y * workgroup_size_y * input.worker_id.z);
}

fn index_to_x(index: u32) -> vec3u {
    // x[k] = (index % strides[k + 1]) / strides[k] for k=0,1,2
    return vec3u(
        index % config.strides.y,
        (index % config.strides.z) / config.strides.y,
        // we exit early in main if index >= config.strides.w, so no need to mod with it.
        index / config.strides.z,
    );
}
