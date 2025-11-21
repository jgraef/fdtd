use std::{
    collections::{
        HashMap,
        hash_map,
    },
    sync::{
        Arc,
        Weak,
    },
};

use bytemuck::{
    Pod,
    Zeroable,
};
use nalgebra::{
    Matrix4,
    Vector3,
};
use parking_lot::Mutex;
use wgpu::util::DeviceExt;

use crate::app::solver::{
    FieldComponent,
    FieldProject,
    fdtd::{
        util::{
            SwapBuffer,
            SwapBufferIndex,
        },
        wgpu::{
            FdtdWgpuSolverInstance,
            FdtdWgpuSolverState,
        },
    },
};

#[derive(Clone, Debug)]
pub(super) struct ProjectionPipeline {
    bind_group_layout: wgpu::BindGroupLayout,
    shader_module: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    pipeline_cache: Arc<Mutex<HashMap<wgpu::TextureFormat, Weak<wgpu::RenderPipeline>>>>,
}

impl ProjectionPipeline {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdtd/project"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fdtd/project"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let shader_module = device.create_shader_module(wgpu::include_wgsl!("project.wgsl"));

        Self {
            bind_group_layout,
            pipeline_layout,
            shader_module,
            pipeline_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_pipeline(
        &self,
        device: &wgpu::Device,
        target_texture_format: wgpu::TextureFormat,
    ) -> Arc<wgpu::RenderPipeline> {
        let mut cache = self.pipeline_cache.lock();

        let create_pipeline = || {
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("fdtd/project"),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader_module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader_module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_texture_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::all(),
                    })],
                }),
                multiview: None,
                cache: None,
            });
            Arc::new(pipeline)
        };

        match cache.entry(target_texture_format) {
            hash_map::Entry::Occupied(mut occupied_entry) => {
                if let Some(pipeline) = occupied_entry.get().upgrade() {
                    pipeline
                }
                else {
                    let pipeline = create_pipeline();
                    occupied_entry.insert(Arc::downgrade(&pipeline));
                    pipeline
                }
            }
            hash_map::Entry::Vacant(vacant_entry) => {
                let pipeline = create_pipeline();
                vacant_entry.insert(Arc::downgrade(&pipeline));
                pipeline
            }
        }
    }
}

/// # TODO
///
/// - This could handle multiple projections for the same (instance, state,
///   target_texture_format). We would then write the projection data into an
///   instance buffer. This would also allow specifying the projection
///   parameters on the `project` method.
/// - We can also bind both field buffers, which would allow us to specify which
///   one we want in the `project` method.
#[derive(Clone, Debug)]
pub struct Projection {
    pipeline: Arc<wgpu::RenderPipeline>,
    projection_buffer: wgpu::Buffer,
    bind_groups: SwapBuffer<wgpu::BindGroup>,
    target_texture_view: wgpu::TextureView,
}

impl Projection {
    fn new(
        instance: &FdtdWgpuSolverInstance,
        state: &FdtdWgpuSolverState,
        target_texture_view: wgpu::TextureView,
        projection: Matrix4<f32>,
        field_component: FieldComponent,
        vector_components: Vector3<f32>,
    ) -> Self {
        let target_texture_format = target_texture_view.texture().format();

        let pipeline = instance
            .backend
            .projection
            .get_pipeline(&instance.backend.device, target_texture_format);

        let projection_data = ProjectionData {
            projection,
            vector_components,
            ..Default::default()
        };

        let projection_buffer =
            instance
                .backend
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("fdtd/project/projection"),
                    contents: bytemuck::bytes_of(&projection_data),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

        let field_component_buffer = |swap_buffer_index| {
            let field_buffers = &state.field_buffers[swap_buffer_index];
            field_buffers[field_component]
                .buffer()
                .unwrap()
                .as_entire_binding()
        };

        let bind_groups = SwapBuffer::from_fn(|swap_buffer_index| {
            instance
                .backend
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fdtd/project"),
                    layout: &instance.backend.projection.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: instance.config_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: projection_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: field_component_buffer(swap_buffer_index),
                        },
                    ],
                })
        });

        Self {
            pipeline,
            projection_buffer,
            bind_groups,
            target_texture_view,
        }
    }

    pub(super) fn project(
        &self,
        command_encoder: &mut wgpu::CommandEncoder,
        swap_buffer_index: SwapBufferIndex,
    ) {
        let mut render_pass = command_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fdtd/project"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.target_texture_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_groups[swap_buffer_index], &[]);
        render_pass.draw(0..6, 0..1);
    }
}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct ProjectionData {
    projection: Matrix4<f32>,
    vector_components: Vector3<f32>,
    _padding: [u32; 1],
}

impl FieldProject<wgpu::TextureView> for FdtdWgpuSolverInstance {
    type Projection = Projection;

    fn create_projection(
        &self,
        state: &FdtdWgpuSolverState,
        target: wgpu::TextureView,
        projection: &Matrix4<f32>,
        field_component: FieldComponent,
        vector_components: &Vector3<f32>,
    ) -> Projection {
        Projection::new(
            self,
            state,
            target,
            *projection,
            field_component,
            *vector_components,
        )
    }
}
