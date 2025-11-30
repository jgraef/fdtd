struct Config {
    size: vec4u,
    strides: vec4u,
    resolution: vec4f,
    time: f32,
    num_sources: u32,
}

@group(0) @binding(0)
var<uniform> config: Config;

@group(0) @binding(1)
var<storage, read> materials: array<vec4f>;

struct Source {
    j_source: vec3f,
    index: u32,
    m_source: vec3f,
}

@group(0) @binding(2)
var<storage, read> sources: array<Source>;

// note: our H and E field buffers will align the elements to 16 bytes anyway,
// so we can use the 4 extra bytes to indicate if a source current is present.
struct Cell {
    value: vec3f,
    source_id: u32,
}

@group(0) @binding(3)
var<storage, read_write> h_field_next: array<Cell>;

@group(0) @binding(4)
var<storage, read_write> e_field_next: array<Cell>;

@group(0) @binding(5)
var<storage, read> h_field_prev: array<Cell>;

@group(0) @binding(6)
var<storage, read> e_field_prev: array<Cell>;


// override constants for the workgroup size being used
override workgroup_size_x: u32 = 0;
override workgroup_size_y: u32 = 0;
override workgroup_size_z: u32 = 0;

// compute shader input
struct Input {
    @builtin(global_invocation_id) worker_id: vec3u,
    @builtin(num_workgroups) num_workgroups: vec3u,
}


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
    let coeff = materials[index].zw;

    // source
    var m_source: vec3f;
    let source_id = h_field_next[index].source_id;
    if source_id != 0 {
        m_source = sources[source_id].m_source;
    }

    // todo: pml
    let psi = vec3f(0.0);

    // update rule
    let h = coeff.x * h_field_prev[index].value + coeff.y * (-e_curl - m_source + psi);
    h_field_next[index] = Cell(h, 0);
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
    let coeff = materials[index].xy;

    // source
    var j_source: vec3f;
    let source_id = e_field_next[index].source_id;
    if source_id != 0 {
        j_source = sources[source_id].j_source;
    }

    // todo: pml
    let psi = vec3f(0.0);

    // update rule
    let e = coeff.x * e_field_prev[index].value + coeff.y * (h_curl - j_source + psi);
    e_field_next[index] = Cell(e, 0);
}


@compute @workgroup_size(workgroup_size_x, workgroup_size_y, workgroup_size_z)
fn update_sources(input: Input) {
    let source_id = input_to_index(input);

    if source_id >= config.num_sources {
        return;
    }

    let source = sources[source_id];

    // put source id into cell, so it's quick to lookup the other way
    e_field_next[source.index].source_id = source_id;
    h_field_next[source.index].source_id = source_id;
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
        let h1 = h_field_next[index].value;
        let h2 = h_field_next[index + config.strides[axis]].value;
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
