use std::{
    convert::Infallible,
    ops::Range,
    sync::Arc,
    time::Duration,
};

use bytemuck::{
    Pod,
    Zeroable,
};
use nalgebra::{
    Point3,
    Scalar,
    Vector3,
};
use smallvec::SmallVec;
use wgpu::util::DeviceExt;

use crate::{
    app::solver::{
        config::{
            EvaluateStopCondition,
            StopCondition,
        },
        fdtd::{
            AccessFieldRegion,
            FdtdSolverConfig,
            FieldComponent,
            Resolution,
            SourceValues,
            lattice::Strider,
            util::{
                SwapBuffer,
                SwapBufferIndex,
                UpdateCoefficients,
                evaluate_stop_condition,
            },
        },
        traits::{
            DomainDescription,
            ReadState,
            SolverBackend,
            SolverInstance,
        },
    },
    util::{
        normalize_point_bounds,
        wgpu::TypedArrayBuffer,
    },
};

#[derive(Clone, Debug)]
pub struct FdtdWgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    limits: ComputeLimits,
    shader_module: wgpu::ShaderModule,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
}

impl FdtdWgpuBackend {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let limits = ComputeLimits::from_limits(&device.limits());

        let shader_module = device.create_shader_module(wgpu::include_wgsl!("fdtd.wgsl"));

        let bind_group_layout_entry = |binding, ty| {
            wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }
        };
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdtd/bind_group_layout"),
            entries: &[
                bind_group_layout_entry(0, wgpu::BufferBindingType::Uniform),
                bind_group_layout_entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
                bind_group_layout_entry(2, wgpu::BufferBindingType::Storage { read_only: false }),
                bind_group_layout_entry(3, wgpu::BufferBindingType::Storage { read_only: true }),
                bind_group_layout_entry(4, wgpu::BufferBindingType::Storage { read_only: true }),
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fdtd"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        Self {
            device: device.clone(),
            queue: queue.clone(),
            limits,
            shader_module,
            bind_group_layout,
            pipeline_layout,
        }
    }
}

impl SolverBackend for FdtdWgpuBackend {
    type Config = FdtdSolverConfig;
    type Point = Point3<usize>;
    type Instance = FdtdWgpuSolverInstance;
    type Error = Infallible;

    fn create_instance<D>(
        &self,
        config: &Self::Config,
        domain_description: D,
    ) -> Result<Self::Instance, Self::Error>
    where
        D: DomainDescription<Self::Point>,
    {
        Ok(FdtdWgpuSolverInstance::new(
            self,
            config,
            domain_description,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct FdtdWgpuSolverInstance {
    backend: FdtdWgpuBackend,
    resolution: Resolution,
    strider: Strider,
    config_buffer: wgpu::Buffer,
    material_buffer: Arc<TypedArrayBuffer<UpdateCoefficientsData>>,
    num_cells: usize,
    update_e_pipeline: wgpu::ComputePipeline,
    update_h_pipeline: wgpu::ComputePipeline,
    // for most use-cases one dispatch will be enough
    dispatches: SmallVec<[Vector3<u32>; 1]>,
}

impl FdtdWgpuSolverInstance {
    fn new(
        backend: &FdtdWgpuBackend,
        config: &FdtdSolverConfig,
        domain_description: impl DomainDescription<Point3<usize>>,
    ) -> Self {
        let strider = config.strider();
        let num_cells = strider.len();
        assert_ne!(num_cells, 0);

        let config_data = ConfigData::new(&strider, &config.resolution, 0.0);

        let config_buffer = backend
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fdtd/uniform"),
                contents: bytemuck::bytes_of(&config_data),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let material_buffer = TypedArrayBuffer::from_fn(
            &backend.device,
            "fdtd/material",
            num_cells,
            wgpu::BufferUsages::STORAGE,
            |index| {
                strider
                    .from_index(index)
                    .map(|point| {
                        UpdateCoefficients::new(
                            &config.resolution,
                            &config.physical_constants,
                            &domain_description.material(&point),
                        )
                    })
                    .unwrap_or_default()
                    .into()
            },
        );

        let workgroup_size = backend.limits.work_group_size_for(num_cells);

        let dispatches = backend
            .limits
            .divide_work_into_dispatches(num_cells, &workgroup_size)
            .collect();

        tracing::debug!(?workgroup_size, ?dispatches);

        let shader_constants = [
            ("workgroup_size_x", workgroup_size.x.into()),
            ("workgroup_size_y", workgroup_size.y.into()),
            ("workgroup_size_z", workgroup_size.z.into()),
        ];
        let create_pipeline = |label, entrypoint| {
            backend
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(label),
                    layout: Some(&backend.pipeline_layout),
                    module: &backend.shader_module,
                    entry_point: Some(entrypoint),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &shader_constants,
                        zero_initialize_workgroup_memory: true,
                    },
                    cache: None,
                })
        };

        let update_e_pipeline = create_pipeline("fdtd/update/e", "update_e");
        let update_h_pipeline = create_pipeline("fdtd/update/h", "update_h");

        Self {
            backend: backend.clone(),
            resolution: config.resolution,
            strider,
            config_buffer,
            material_buffer: Arc::new(material_buffer),
            num_cells,
            update_e_pipeline,
            update_h_pipeline,
            dispatches,
        }
    }

    fn update_impl(&self, state: &mut FdtdWgpuSolverState) {
        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick + 1);

        // update time
        // todo: would be nice if we could combine this with the command encoder
        let config_data = ConfigData::new(&self.strider, &self.resolution, state.time as f32);
        self.backend
            .queue
            .write_buffer(&self.config_buffer, 0, bytemuck::bytes_of(&config_data));

        // apply sources
        //todo!();
        // pretend data
        let sources: Vec<(Point3<usize>, SourceValues)> = vec![];
        // reallocate source buffer if needed. if it is being allocated, use the
        // constructor that fills it
        for (point, source_value) in sources {
            // todo
        }

        let mut command_encoder =
            self.backend
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("fdtd/update"),
                });

        // compute pass
        {
            let mut compute_pass =
                command_encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdtd/update"),
                    timestamp_writes: None,
                });

            let mut dispatch_update = |pipeline, bind_group| {
                compute_pass.set_pipeline(pipeline);
                compute_pass.set_bind_group(0, bind_group, &[]);

                for num_workgroups in &self.dispatches {
                    compute_pass.dispatch_workgroups(
                        num_workgroups.x,
                        num_workgroups.y,
                        num_workgroups.z,
                    );
                }
            };

            dispatch_update(
                &self.update_h_pipeline,
                &state.update_bind_groups.h[swap_buffer_index],
            );

            dispatch_update(
                &self.update_e_pipeline,
                &state.update_bind_groups.e[swap_buffer_index],
            );
        }

        let submission_index = self.backend.queue.submit([command_encoder.finish()]);
        self.backend
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: None,
            })
            .unwrap();

        state.tick += 1;
        state.time += self.resolution.temporal;
    }
}

impl SolverInstance for FdtdWgpuSolverInstance {
    type State = FdtdWgpuSolverState;
    type Point = Point3<usize>;

    fn create_state(&self) -> Self::State {
        FdtdWgpuSolverState::new(self)
    }

    fn update(&self, state: &mut Self::State) {
        self.update_impl(state);
    }
}

impl EvaluateStopCondition for FdtdWgpuSolverInstance {
    fn evaluate_stop_condition(
        &self,
        state: &Self::State,
        stop_condition: &StopCondition,
        time_elapsed: Duration,
    ) -> bool {
        evaluate_stop_condition(stop_condition, time_elapsed, state.tick, state.time)
    }
}

#[derive(Debug)]
pub struct FdtdWgpuSolverState {
    field_buffers: SwapBuffer<FieldBuffers>,
    source_buffer: Option<TypedArrayBuffer<SourceData>>,
    update_bind_groups: UpdateBindGroups,
    tick: usize,
    time: f64,
}

impl FdtdWgpuSolverState {
    fn new(instance: &FdtdWgpuSolverInstance) -> Self {
        let field_buffers = {
            let default_value = Cell::default();
            SwapBuffer::from_fn(|_| {
                let buffer = |label| {
                    TypedArrayBuffer::from_fn(
                        &instance.backend.device,
                        label,
                        instance.num_cells,
                        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                        |_index| default_value,
                    )
                };
                FieldBuffers {
                    e: buffer("fdtd/field/e"),
                    h: buffer("fdtd/field/h"),
                }
            })
        };

        let update_bind_groups = UpdateBindGroups::new(instance, &field_buffers);

        Self {
            field_buffers,
            source_buffer: None,
            update_bind_groups,
            tick: 0,
            time: 0.0,
        }
    }

    pub fn tick(&self) -> usize {
        self.tick
    }

    pub fn time(&self) -> f64 {
        self.time
    }
}

impl ReadState<FdtdWgpuSolverInstance> for AccessFieldRegion {
    type Value<'a>
        = WgpuFieldRegionIter
    where
        Self: 'a,
        FdtdWgpuSolverInstance: 'a;

    fn read_state<'a>(
        &'a self,
        instance: &'a FdtdWgpuSolverInstance,
        state: &'a FdtdWgpuSolverState,
    ) -> WgpuFieldRegionIter {
        let range = normalize_point_bounds(self.range, *instance.strider.size());

        let fetch_data = |index_range: Range<usize>, check_inside: Option<Range<Point3<usize>>>| {
            let start_index = index_range.start;

            let swap_buffer_index = SwapBufferIndex::from_tick(state.tick);

            let field_buffers = &state.field_buffers[swap_buffer_index];
            let buffer = match self.field_component {
                FieldComponent::E => &field_buffers.e,
                FieldComponent::H => &field_buffers.h,
            };

            // unfortunately we have to copy to a vec and can't return something that holds
            // the buffer view. this is because we would need to store a borrow
            // to something that is also in that struct, requiring a self-referential
            // struct. we could use a crate for this though.
            let data = buffer.read(
                &instance.backend.device,
                &instance.backend.queue,
                index_range,
                |view| view.iter().map(|data| data.value).collect::<Vec<_>>(),
            );

            WgpuFieldRegionIter {
                strider: instance.strider,
                start_index,
                data: data.into_iter().enumerate(),
                check_inside,
            }
        };

        match instance.strider.to_contiguous_index_range(range.clone()) {
            Ok(index_range) => fetch_data(index_range, Some(range)),
            Err(index_range) => {
                // todo: run a compute shader that projects the selected region into a first
                // staging buffer, then copy to the second staging buffer like in the contiguous
                // case. we could also skip the projection compute shader if the
                // holes are small.

                // for now we'll just fetch the whole range and ignore points that lie outside
                fetch_data(index_range, None)
            }
        }
    }
}

#[derive(Debug)]
pub struct WgpuFieldRegionIter {
    strider: Strider,
    start_index: usize,
    data: std::iter::Enumerate<std::vec::IntoIter<Vector3<f32>>>,
    check_inside: Option<Range<Point3<usize>>>,
}

impl Iterator for WgpuFieldRegionIter {
    type Item = (Point3<usize>, Vector3<f32>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (region_index, data) = self.data.next()?;
            let point = self
                .strider
                .from_index(region_index + self.start_index)
                .unwrap();

            if self
                .check_inside
                .as_ref()
                .is_none_or(|check_against| check_against.contains(&point))
            {
                return Some((point, data));
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.data.size_hint()
    }
}

impl ExactSizeIterator for WgpuFieldRegionIter {}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct ConfigData {
    size: [u32; 4],
    strides: [u32; 4],
    resolution: [f32; 4],
    time: f32,
    _padding: [u32; 3],
}

impl ConfigData {
    fn new(strider: &Strider, resolution: &Resolution, time: f32) -> Self {
        Self {
            size: {
                let size = strider.size().cast::<u32>();
                [size.x, size.y, size.z, 0]
            },
            strides: {
                let strides = strider.strides().cast::<u32>();
                [strides.x, strides.y, strides.z, strides.w]
            },
            resolution: {
                [
                    resolution.spatial.x as f32,
                    resolution.spatial.y as f32,
                    resolution.spatial.z as f32,
                    resolution.temporal as f32,
                ]
            },
            time,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct SourceData {
    j_source: Vector3<f32>,
    index: u32,
    m_source: Vector3<f32>,
    padding: u32,
}

impl SourceData {
    pub fn new(index: u32, j_source: Vector3<f32>, m_source: Vector3<f32>) -> Self {
        Self {
            index,
            j_source,
            m_source,
            ..Default::default()
        }
    }
}

fn array4_from_vector3<T>(vector: Vector3<T>) -> [T; 4]
where
    T: Scalar + Default,
{
    [
        vector.x.clone(),
        vector.y.clone(),
        vector.z.clone(),
        T::default(),
    ]
}

#[derive(Debug)]
struct FieldBuffers {
    e: TypedArrayBuffer<Cell>,
    h: TypedArrayBuffer<Cell>,
}

#[derive(Clone, Copy, Debug, Default, Zeroable, Pod)]
#[repr(C)]
struct Cell {
    value: Vector3<f32>,
    source_id: u32,
}

#[derive(Debug)]
struct UpdateBindGroups {
    h: SwapBuffer<wgpu::BindGroup>,
    e: SwapBuffer<wgpu::BindGroup>,
}

impl UpdateBindGroups {
    fn new(instance: &FdtdWgpuSolverInstance, field_buffers: &SwapBuffer<FieldBuffers>) -> Self {
        // note: all the unwraps are okay, since we never allocate empty buffers.

        let update_h_field_bind_group = {
            SwapBuffer::from_fn(|current| {
                let previous = current.other();
                instance
                    .backend
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("fdtd/bind_group/h/{current:?}")),
                        layout: &instance.backend.bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: instance.config_buffer.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: instance
                                    .material_buffer
                                    .buffer()
                                    .unwrap()
                                    .as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: field_buffers[current]
                                    .h
                                    .buffer()
                                    .unwrap()
                                    .as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: field_buffers[previous]
                                    .h
                                    .buffer()
                                    .unwrap()
                                    .as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 4,
                                resource: field_buffers[previous]
                                    .e
                                    .buffer()
                                    .unwrap()
                                    .as_entire_binding(),
                            },
                        ],
                    })
            })
        };

        let update_e_field_bind_group = SwapBuffer::from_fn(|current| {
            let previous = current.other();
            instance
                .backend
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("fdtd/bind_group/e/{current:?}")),
                    layout: &instance.backend.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: instance.config_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: instance
                                .material_buffer
                                .buffer()
                                .unwrap()
                                .as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: field_buffers[current]
                                .e
                                .buffer()
                                .unwrap()
                                .as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            // note: this uses the current h buffer, since we update h first.
                            resource: field_buffers[current]
                                .h
                                .buffer()
                                .unwrap()
                                .as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: field_buffers[previous]
                                .e
                                .buffer()
                                .unwrap()
                                .as_entire_binding(),
                        },
                    ],
                })
        });

        Self {
            h: update_h_field_bind_group,
            e: update_e_field_bind_group,
        }
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct UpdateCoefficientsData {
    c_a: f32,
    c_b: f32,
    d_a: f32,
    d_b: f32,
}

impl From<UpdateCoefficients> for UpdateCoefficientsData {
    fn from(value: UpdateCoefficients) -> Self {
        Self {
            c_a: value.c_a as f32,
            c_b: value.c_b as f32,
            d_a: value.d_a as f32,
            d_b: value.d_b as f32,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ComputeLimits {
    pub max_workgroup_storage_size: u32,
    pub max_invocations_per_workgroup: u32,
    pub max_workgroup_size: Vector3<u32>,

    /// Max number of workgroups per dispatch.
    ///
    /// This is one value for each dimension in wgpu, but for more flexibility
    /// we make it a vector.
    pub max_workgroups_per_dispatch: Vector3<u32>,
}

impl ComputeLimits {
    pub fn from_limits(limits: &wgpu::Limits) -> Self {
        Self {
            max_workgroup_storage_size: limits.max_compute_workgroup_storage_size,
            max_invocations_per_workgroup: limits.max_compute_invocations_per_workgroup,
            max_workgroup_size: Vector3::new(
                limits.max_compute_workgroup_size_x,
                limits.max_compute_workgroup_size_y,
                limits.max_compute_workgroup_size_z,
            ),
            max_workgroups_per_dispatch: Vector3::repeat(
                limits.max_compute_workgroups_per_dimension,
            ),
        }
    }

    pub fn work_group_size_for(&self, work_size: usize) -> Vector3<u32> {
        work_group_size_for(
            work_size,
            &self.max_workgroup_size,
            self.max_invocations_per_workgroup,
        )
    }

    pub fn divide_work_into_dispatches(
        &self,
        work_size: usize,
        workgroup_size: &Vector3<u32>,
    ) -> impl Iterator<Item = Vector3<u32>> + 'static {
        divide_work_into_dispatches(
            work_size,
            &workgroup_size,
            &self.max_workgroups_per_dispatch,
        )
    }
}

pub fn work_group_size_for(
    work_size: usize,
    max_workgroup_size: &Vector3<u32>,
    max_invocations_per_workgroup: u32,
) -> Vector3<u32> {
    let max_workgroup_size = max_workgroup_size.cast::<usize>();
    let mut n = work_size.min(max_invocations_per_workgroup as usize);
    let mut workgroup_size = Vector3::zeros();

    workgroup_size.x = n.clamp(1, max_workgroup_size.x);
    n /= workgroup_size.x;

    workgroup_size.y = n.clamp(1, max_workgroup_size.y);
    n /= workgroup_size.y;

    workgroup_size.z = n.clamp(1, max_workgroup_size.z);

    workgroup_size.cast()
}

pub fn divide_work_into_dispatches(
    work_size: usize,
    workgroup_size: &Vector3<u32>,
    max_workgroups_per_dispatch: &Vector3<u32>,
) -> impl Iterator<Item = Vector3<u32>> + 'static {
    let max_workgroups_per_dispatch = max_workgroups_per_dispatch.cast::<usize>();

    // how many total workgroups we need
    let mut remaining_workgroups = work_size.div_ceil(workgroup_size.cast::<usize>().product());

    std::iter::from_fn(move || {
        (remaining_workgroups > 0).then(|| {
            let mut num_workgroups = Vector3::zeros();
            let mut n = remaining_workgroups;

            // put as much work as possible into x
            num_workgroups.x = n.clamp(1, max_workgroups_per_dispatch.x);
            n /= num_workgroups.x;

            // put as much remaining work as possible into y
            num_workgroups.y = n.clamp(1, max_workgroups_per_dispatch.y);
            n /= num_workgroups.y;

            // put as much remaining work as possible into z
            num_workgroups.z = n.clamp(1, max_workgroups_per_dispatch.z);

            remaining_workgroups = remaining_workgroups.saturating_sub(num_workgroups.product());
            num_workgroups.cast()
        })
    })
}
