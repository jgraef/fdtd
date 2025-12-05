pub mod project;

use std::{
    convert::Infallible,
    ops::{
        Index,
        Range,
        RangeBounds,
    },
    sync::Arc,
};

use bytemuck::{
    Pod,
    Zeroable,
};
use cem_util::wgpu::buffer::{
    StagedTypedArrayBuffer,
    StagingPool,
    TypedArrayBuffer,
    TypedArrayBufferReadView,
    WriteStaging,
    WriteStagingCommit,
    WriteStagingTransaction,
};
use nalgebra::{
    Point3,
    Vector3,
};
use wgpu::util::DeviceExt;

pub use self::project::FdtdWgpuTextureProjection;
use crate::{
    DomainDescription,
    Field,
    FieldComponent,
    FieldView,
    SolverBackend,
    SolverInstance,
    Time,
    UpdatePass,
    UpdatePassForcing,
    fdtd::{
        FdtdSolverConfig,
        Resolution,
        strider::Strider,
        util::{
            SwapBuffer,
            SwapBufferIndex,
            UpdateCoefficients,
            normalize_point_bounds,
        },
        wgpu::project::ProjectionPipeline,
    },
    source::SourceValues,
};

#[derive(Clone, Debug)]
pub struct FdtdWgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    limits: ComputeLimits,
    shader_module: wgpu::ShaderModule,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    projection: ProjectionPipeline,
    staging_pool: StagingPool,
}

impl FdtdWgpuBackend {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, staging_pool: StagingPool) -> Self {
        let limits = ComputeLimits::from_limits(&device.limits());

        let shader_module = device.create_shader_module(wgpu::include_wgsl!("update.wgsl"));

        let bind_group_layout = BINDINGS.bind_group_layout(&device);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fdtd"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let projection = ProjectionPipeline::new(&device);

        Self {
            device,
            queue,
            limits,
            shader_module,
            bind_group_layout,
            pipeline_layout,
            projection,
            staging_pool,
        }
    }

    fn submit_and_poll(&self, command_buffers: impl IntoIterator<Item = wgpu::CommandBuffer>) {
        let submission_index = self.queue.submit(command_buffers);

        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: None,
            })
            .unwrap();
    }
}

impl SolverBackend<FdtdSolverConfig, Point3<usize>> for FdtdWgpuBackend {
    type Instance = FdtdWgpuSolverInstance;
    type Error = Infallible;

    fn create_instance<D>(
        &self,
        config: &FdtdSolverConfig,
        domain_description: D,
    ) -> Result<Self::Instance, Self::Error>
    where
        D: DomainDescription<Point3<usize>>,
    {
        Ok(FdtdWgpuSolverInstance::new(
            self,
            config,
            domain_description,
        ))
    }

    fn memory_required(&self, config: &FdtdSolverConfig) -> Option<usize> {
        let per_cell =
            size_of::<UpdateCoefficientsData>() + 2 * size_of::<Cell>() + size_of::<SourceData>();
        Some(config.size().product() * per_cell)
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
    update_sources_pipeline: wgpu::ComputePipeline,
    update_e_pipeline: wgpu::ComputePipeline,
    update_h_pipeline: wgpu::ComputePipeline,
    workgroup_size: Vector3<u32>,
    dispatches: Vec<Vector3<u32>>,
}

impl FdtdWgpuSolverInstance {
    fn new(
        backend: &FdtdWgpuBackend,
        config: &FdtdSolverConfig,
        mut domain_description: impl DomainDescription<Point3<usize>>,
    ) -> Self {
        let strider = config.strider();
        let num_cells = strider.len();
        assert_ne!(num_cells, 0);

        let config_data = ConfigData::new(&strider, &config.resolution, 0.0, 0);

        let config_buffer = backend
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fdtd/uniform"),
                contents: bytemuck::bytes_of(&config_data),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let material_buffer = TypedArrayBuffer::from_fn(
            backend.device.clone(),
            "fdtd/material",
            num_cells,
            wgpu::BufferUsages::STORAGE,
            |index| {
                strider
                    .point(index)
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

        let update_sources_pipeline = create_pipeline("fdtd/update/sources", "update_sources");
        let update_e_pipeline = create_pipeline("fdtd/update/e", "update_e");
        let update_h_pipeline = create_pipeline("fdtd/update/h", "update_h");

        Self {
            backend: backend.clone(),
            resolution: config.resolution,
            strider,
            config_buffer,
            material_buffer: Arc::new(material_buffer),
            num_cells,
            update_sources_pipeline,
            update_e_pipeline,
            update_h_pipeline,
            workgroup_size,
            dispatches,
        }
    }
}

impl SolverInstance for FdtdWgpuSolverInstance {
    type State = FdtdWgpuSolverState;
    type UpdatePass<'a>
        = FdtdWgpuUpdatePass<'a>
    where
        Self: 'a;

    fn create_state(&self) -> Self::State {
        FdtdWgpuSolverState::new(self)
    }

    fn begin_update<'a>(&'a self, state: &'a mut Self::State) -> FdtdWgpuUpdatePass<'a> {
        FdtdWgpuUpdatePass::new(self, state)
    }
}

#[derive(Debug)]
pub struct FdtdWgpuSolverState {
    field_buffers: SwapBuffer<FieldBuffers>,
    source_buffer: StagedTypedArrayBuffer<SourceData>,
    update_bind_groups: SwapBuffer<wgpu::BindGroup>,
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
                        instance.backend.device.clone(),
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

        let source_buffer = StagedTypedArrayBuffer::with_capacity(
            instance.backend.device.clone(),
            "fdtd/sources",
            wgpu::BufferUsages::STORAGE,
            32,
        );
        let update_bind_groups =
            BINDINGS.bind_group(instance, &field_buffers, source_buffer.buffer.buffer().expect("source buffer should have a gpu buffer allocated because it is initialized with an non-zero initial capacity"));

        Self {
            field_buffers,
            source_buffer,
            update_bind_groups,
            tick: 0,
            time: 0.0,
        }
    }
}

impl Time for FdtdWgpuSolverState {
    fn tick(&self) -> usize {
        self.tick
    }

    fn time(&self) -> f64 {
        self.time
    }
}

#[derive(Debug)]
pub struct FdtdWgpuUpdatePass<'a> {
    instance: &'a FdtdWgpuSolverInstance,
    state: &'a mut FdtdWgpuSolverState,
    swap_buffer_index: SwapBufferIndex,
}

impl<'a> FdtdWgpuUpdatePass<'a> {
    fn new(instance: &'a FdtdWgpuSolverInstance, state: &'a mut FdtdWgpuSolverState) -> Self {
        // initialize source buffer
        assert!(state.source_buffer.host_staging.is_empty());
        state.source_buffer.push(SourceData::default());

        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick + 1);

        Self {
            instance,
            state,

            swap_buffer_index,
        }
    }
}

impl<'a> UpdatePassForcing<Point3<usize>> for FdtdWgpuUpdatePass<'a> {
    fn set_forcing(&mut self, point: &Point3<usize>, value: &SourceValues) {
        let cell_index = self
            .instance
            .strider
            .index(point)
            .unwrap_or_else(|| panic!("set_forcing called with invalid point: {point:?}"));

        // note: unlike in the cpu implementation, here we can't check if that point was
        // already inserted
        self.state
            .source_buffer
            .push(SourceData::new(cell_index, value.j, value.m));
    }
}

impl<'a> UpdatePass for FdtdWgpuUpdatePass<'a> {
    fn finish(self) {
        let mut command_encoder =
            self.instance
                .backend
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("fdtd/update"),
                });

        let mut write_staging = WriteStagingTransaction::new(
            self.instance.backend.staging_pool.belt(),
            &self.instance.backend.device,
            &mut command_encoder,
        );

        // write source data
        let num_sources = self.state.source_buffer.host_staging.len();
        self.state.source_buffer.flush(
            |new_buffer| {
                self.state.update_bind_groups =
                    BINDINGS.bind_group(self.instance, &self.state.field_buffers, new_buffer)
            },
            &mut write_staging,
        );

        // update time
        // todo: would be nice if we could combine this with the command encoder
        let config_data = ConfigData::new(
            &self.instance.strider,
            &self.instance.resolution,
            self.state.time,
            num_sources,
        );
        write_staging.write_buffer_from_slice(
            self.instance.config_buffer.slice(..),
            bytemuck::bytes_of(&config_data),
        );

        write_staging.commit();

        // compute pass
        {
            let mut compute_pass =
                command_encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdtd/update"),
                    timestamp_writes: None,
                });

            compute_pass.set_bind_group(
                0,
                &self.state.update_bind_groups[self.swap_buffer_index],
                &[],
            );

            // update sources
            compute_pass.set_pipeline(&self.instance.update_sources_pipeline);
            for num_workgroups in self
                .instance
                .backend
                .limits
                .divide_work_into_dispatches(num_sources, &self.instance.workgroup_size)
            {
                compute_pass.dispatch_workgroups(
                    num_workgroups.x,
                    num_workgroups.y,
                    num_workgroups.z,
                );
            }

            let mut dispatch_update = |pipeline| {
                compute_pass.set_pipeline(pipeline);

                for num_workgroups in &self.instance.dispatches {
                    compute_pass.dispatch_workgroups(
                        num_workgroups.x,
                        num_workgroups.y,
                        num_workgroups.z,
                    );
                }
            };

            dispatch_update(&self.instance.update_h_pipeline);
            dispatch_update(&self.instance.update_e_pipeline);
        }

        self.instance
            .backend
            .submit_and_poll([command_encoder.finish()]);

        self.state.tick += 1;
        self.state.time += self.instance.resolution.temporal;
    }
}

impl Field<Point3<usize>> for FdtdWgpuSolverInstance {
    type View<'a>
        = WgpuFieldView<'a>
    where
        Self: 'a;

    fn field<'a, R>(
        &'a self,
        state: &'a Self::State,
        range: R,
        field_component: FieldComponent,
    ) -> WgpuFieldView<'a>
    where
        R: RangeBounds<Point3<usize>>,
    {
        let range = normalize_point_bounds(range, *self.strider.size());

        let fetch_data = |index_range: Range<usize>, check_inside: Option<Range<Point3<usize>>>| {
            let start_index = index_range.start;

            let swap_buffer_index = SwapBufferIndex::from_tick(state.tick);

            let field_buffers = &state.field_buffers[swap_buffer_index];
            let buffer = &field_buffers[field_component];
            let view = buffer.read_view(index_range, &self.backend.queue);

            WgpuFieldView {
                strider: &self.strider,
                check_inside,
                start_index,
                view,
            }
        };

        match self.strider.contiguous_index_range(range.clone()) {
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
pub struct WgpuFieldView<'a> {
    strider: &'a Strider,
    start_index: usize,
    view: TypedArrayBufferReadView<'a, Cell>,
    check_inside: Option<Range<Point3<usize>>>,
}

impl<'a> FieldView<Point3<usize>> for WgpuFieldView<'a> {
    type Iter<'b>
        = WgpuFieldIter<'b>
    where
        Self: 'b;

    fn at(&self, point: &Point3<usize>) -> Option<Vector3<f64>> {
        let view_index = self.strider.index(point)? - self.start_index;

        let check_passed = self
            .check_inside
            .as_ref()
            .is_none_or(|check_against| check_against.contains(point));

        check_passed.then(|| self.view[view_index].value.cast())
    }

    fn iter<'b>(&'b self) -> Self::Iter<'b> {
        WgpuFieldIter {
            strider: self.strider,
            start_index: self.start_index,
            view_index: 0,
            view: &self.view,
            check_inside: self.check_inside.clone(),
        }
    }
}

#[derive(Debug)]
pub struct WgpuFieldIter<'a> {
    strider: &'a Strider,
    start_index: usize,
    view_index: usize,
    view: &'a TypedArrayBufferReadView<'a, Cell>,
    check_inside: Option<Range<Point3<usize>>>,
}

impl<'a> Iterator for WgpuFieldIter<'a> {
    type Item = (Point3<usize>, Vector3<f64>);

    fn next(&mut self) -> Option<Self::Item> {
        while self.view_index < self.view.len() {
            let point = self
                .strider
                .point(self.view_index + self.start_index)
                .unwrap();

            let check_passed = self
                .check_inside
                .as_ref()
                .is_none_or(|check_against| check_against.contains(&point));

            let value = check_passed.then(|| self.view[self.view_index].value);

            self.view_index += 1;

            if let Some(value) = value {
                return Some((point, value.cast()));
            }
        }

        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.view.len() - self.view_index;
        (n, Some(n))
    }
}

impl<'a> ExactSizeIterator for WgpuFieldIter<'a> {}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct ConfigData {
    size: [u32; 4],
    strides: [u32; 4],
    resolution: [f32; 4],
    time: f32,
    num_sources: u32,
    _padding: [u32; 2],
}

impl ConfigData {
    fn new(strider: &Strider, resolution: &Resolution, time: f64, num_sources: usize) -> Self {
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
            time: time as f32,
            num_sources: num_sources as u32,
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
    pub fn new(index: usize, j_source: Vector3<f64>, m_source: Vector3<f64>) -> Self {
        Self {
            index: index as u32,
            j_source: j_source.cast(),
            m_source: m_source.cast(),
            ..Default::default()
        }
    }
}

#[derive(Debug)]
struct FieldBuffers {
    e: TypedArrayBuffer<Cell>,
    h: TypedArrayBuffer<Cell>,
}

impl Index<FieldComponent> for FieldBuffers {
    type Output = TypedArrayBuffer<Cell>;

    fn index(&self, index: FieldComponent) -> &Self::Output {
        match index {
            FieldComponent::E => &self.e,
            FieldComponent::H => &self.h,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Zeroable, Pod)]
#[repr(C)]
struct Cell {
    value: Vector3<f32>,
    source_id: u32,
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
        divide_work_into_dispatches(work_size, workgroup_size, &self.max_workgroups_per_dispatch)
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

#[derive(Clone, Copy, Debug)]
struct Bindings {
    config: u32,
    material: u32,
    sources: u32,
    h_field_next: u32,
    e_field_next: u32,
    h_field_previous: u32,
    e_field_previous: u32,
}

impl Bindings {
    fn bind_group_layout(&self, device: &wgpu::Device) -> wgpu::BindGroupLayout {
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

        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdtd/bind_group_layout"),
            entries: &[
                bind_group_layout_entry(self.config, wgpu::BufferBindingType::Uniform),
                bind_group_layout_entry(
                    self.material,
                    wgpu::BufferBindingType::Storage { read_only: true },
                ),
                bind_group_layout_entry(
                    self.sources,
                    wgpu::BufferBindingType::Storage { read_only: true },
                ),
                bind_group_layout_entry(
                    self.h_field_next,
                    wgpu::BufferBindingType::Storage { read_only: false },
                ),
                bind_group_layout_entry(
                    self.e_field_next,
                    wgpu::BufferBindingType::Storage { read_only: false },
                ),
                bind_group_layout_entry(
                    self.h_field_previous,
                    wgpu::BufferBindingType::Storage { read_only: true },
                ),
                bind_group_layout_entry(
                    self.e_field_previous,
                    wgpu::BufferBindingType::Storage { read_only: true },
                ),
            ],
        })
    }

    fn bind_group(
        &self,
        instance: &FdtdWgpuSolverInstance,
        field_buffers: &SwapBuffer<FieldBuffers>,
        source_buffer: &wgpu::Buffer,
    ) -> SwapBuffer<wgpu::BindGroup> {
        // note: all the unwraps are okay, since we never allocate empty buffers.
        fn field_binding<'a>(buffer: &'a TypedArrayBuffer<Cell>) -> wgpu::BindingResource<'a> {
            buffer.buffer().unwrap().as_entire_binding()
        }

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
                            binding: self.config,
                            resource: instance.config_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: self.material,
                            resource: instance
                                .material_buffer
                                .buffer()
                                .unwrap()
                                .as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: self.sources,
                            resource: source_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: self.h_field_next,
                            resource: field_binding(&field_buffers[current].h),
                        },
                        wgpu::BindGroupEntry {
                            binding: self.e_field_next,
                            resource: field_binding(&field_buffers[current].e),
                        },
                        wgpu::BindGroupEntry {
                            binding: self.h_field_previous,
                            resource: field_binding(&field_buffers[previous].h),
                        },
                        wgpu::BindGroupEntry {
                            binding: self.e_field_previous,
                            resource: field_binding(&field_buffers[previous].e),
                        },
                    ],
                })
        })
    }
}

const BINDINGS: Bindings = Bindings {
    config: 0,
    material: 1,
    sources: 2,
    h_field_next: 3,
    e_field_next: 4,
    h_field_previous: 5,
    e_field_previous: 6,
};
