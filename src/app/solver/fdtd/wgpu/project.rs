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
use nalgebra::Matrix4;
use parking_lot::Mutex;
use wgpu::util::DeviceExt;

use crate::app::{
    composer::renderer::texture_channel::{
        TextureSender,
        UndecidedTextureSender,
    },
    solver::{
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
        project::{
            BeginProjectionPass,
            CreateProjection,
            ImageTarget,
            ProjectionParameters,
            ProjectionPass,
            ProjectionPassAdd,
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
                // config (of simulation domain)
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // projection (transform, color map)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // field buffer
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
#[derive(Debug)]
struct TextureProjectionInner {
    pipeline: Arc<wgpu::RenderPipeline>,
    projection_buffer: wgpu::Buffer,
    bind_groups: SwapBuffer<wgpu::BindGroup>,
}

impl TextureProjectionInner {
    fn new(
        instance: &FdtdWgpuSolverInstance,
        state: &FdtdWgpuSolverState,
        parameters: &ProjectionParameters,
        target_texture_format: wgpu::TextureFormat,
    ) -> Self {
        let pipeline = instance
            .backend
            .projection
            .get_pipeline(&instance.backend.device, target_texture_format);

        let projection_data = ProjectionData::new(parameters);

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
            field_buffers[parameters.field]
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
        }
    }

    fn project(
        &self,
        command_encoder: &mut wgpu::CommandEncoder,
        swap_buffer_index: SwapBufferIndex,
        target_texture_view: &wgpu::TextureView,
    ) {
        let mut render_pass = command_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fdtd/project"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_texture_view,
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

#[derive(Debug)]
pub struct TextureProjection {
    inner: TextureProjectionInner,
    texture_view: wgpu::TextureView,
}

impl CreateProjection<wgpu::Texture> for FdtdWgpuSolverInstance {
    type Projection = TextureProjection;

    fn create_projection(
        &self,
        state: &FdtdWgpuSolverState,
        target: wgpu::Texture,
        parameters: &ProjectionParameters,
    ) -> TextureProjection {
        let texture_view = target.create_view(&wgpu::TextureViewDescriptor {
            label: Some("fdtd-wgpu/projection"),
            ..Default::default()
        });
        let texture_format = target.format();

        TextureProjection {
            inner: TextureProjectionInner::new(self, state, parameters, texture_format),
            texture_view,
        }
    }
}

impl CreateProjection<wgpu::TextureView> for FdtdWgpuSolverInstance {
    type Projection = TextureProjection;

    fn create_projection(
        &self,
        state: &FdtdWgpuSolverState,
        target: wgpu::TextureView,
        parameters: &ProjectionParameters,
    ) -> TextureProjection {
        let texture_format = target.texture().format();

        TextureProjection {
            inner: TextureProjectionInner::new(self, state, parameters, texture_format),
            texture_view: target,
        }
    }
}

impl CreateProjection<UndecidedTextureSender> for FdtdWgpuSolverInstance {
    type Projection = TextureProjection;

    fn create_projection(
        &self,
        state: &FdtdWgpuSolverState,
        target: UndecidedTextureSender,
        parameters: &ProjectionParameters,
    ) -> TextureProjection {
        let texture_sender = target.send_texture();
        self.create_projection(state, texture_sender, parameters)
    }
}

impl CreateProjection<TextureSender> for FdtdWgpuSolverInstance {
    type Projection = TextureProjection;

    fn create_projection(
        &self,
        state: &FdtdWgpuSolverState,
        target: TextureSender,
        parameters: &ProjectionParameters,
    ) -> TextureProjection {
        let inner = TextureProjectionInner::new(self, state, parameters, target.format);
        TextureProjection {
            inner,
            texture_view: target.texture_and_view.view.clone(),
        }
    }
}

#[derive(Debug)]
pub struct ImageProjection<Target>
where
    Target: ImageTarget,
{
    target: Target,
    inner: TextureProjectionInner,
    buffer_texture: wgpu::Texture,
    buffer_texture_view: wgpu::TextureView,
    staging: Staging,
}

impl<Target> CreateProjection<Target> for FdtdWgpuSolverInstance
where
    Target: ImageTarget<Pixel = image::Rgba<u8>>,
{
    type Projection = ImageProjection<Target>;

    fn create_projection(
        &self,
        state: &FdtdWgpuSolverState,
        target: Target,
        parameters: &ProjectionParameters,
    ) -> ImageProjection<Target> {
        let size = target.size();

        // use srgba?
        let texture_format = wgpu::TextureFormat::Rgba8Unorm;

        let buffer_texture = self
            .backend
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("fdtd/projection/buffer"),
                size: wgpu::Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: texture_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

        let buffer_texture_view = buffer_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("fdtd/projection/buffer"),
            ..Default::default()
        });

        let staging = {
            let bytes_per_row_unpadded = size.x as u64 * 4;
            let bytes_per_row_padded =
                bytes_per_row_unpadded.max(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
            let staging_size = bytes_per_row_padded * size.y as u64;

            let buffer = self.backend.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fdtd/prrojection/staging"),
                size: staging_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

            Staging {
                bytes_per_row_unpadded,
                bytes_per_row_padded,
                buffer,
            }
        };

        let inner = TextureProjectionInner::new(self, state, parameters, texture_format);

        ImageProjection {
            target,
            inner,
            buffer_texture,
            buffer_texture_view,
            staging,
        }
    }
}

#[derive(Debug)]
struct Staging {
    bytes_per_row_unpadded: u64,
    bytes_per_row_padded: u64,
    buffer: wgpu::Buffer,
}

impl<'a, Target> ProjectionPassAdd<'a, ImageProjection<Target>> for FdtdWgpuProjectionPass<'a>
where
    Target: ImageTarget<Pixel = image::Rgba<u8>>,
{
    fn add_projection(&mut self, projection: &'a mut ImageProjection<Target>) {
        projection.inner.project(
            &mut self.command_encoder,
            self.swap_buffer_index,
            &projection.buffer_texture_view,
        );

        let size = projection.target.size();

        let bytes_per_row_unpadded = size.x * 4;
        let bytes_per_row_padded = bytes_per_row_unpadded.max(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

        // copy buffer texture to staging buffer
        self.command_encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &projection.buffer_texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &projection.staging.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    bytes_per_row: Some(bytes_per_row_padded),
                    ..Default::default()
                },
            },
            wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
        );

        // after the command encoder has been submitted, we can map the staging
        // buffer.
        self.command_encoder.map_buffer_on_submit(
            &projection.staging.buffer,
            wgpu::MapMode::Read,
            ..,
            |result| {
                // todo
                result.unwrap();
            },
        );

        // stash closure to copy from mapped staging buffer to image later
        self.copy_to_image_buffer.push({
            // make sure we unmap the staging buffer when we don't need read access to it
            // anymore. this will be moved into the closure, which is stored in the
            // projection pass. so if the pass is dropped before finish is called, the
            // buffer will be unmapped regardless.
            //
            // this wrapper will also hold the view while we need it, to make sure it's
            // dropped before the buffer is unmapped.
            let mut staging_buffer = MappedStagingBuffer {
                buffer: &projection.staging.buffer,
                view: None,
            };

            // move &mut of projection target into closure
            let target = &mut projection.target;

            let bytes_per_row_padded = bytes_per_row_padded as usize;

            Box::new(move || {
                // this closure finishes the projection by copying from the mapped staging
                // buffer to the image. it is executed in `finish` after the command buffer has
                // been submitted.

                let staging_view = staging_buffer.view();

                target.with_image_buffer(|image| {
                    image.enumerate_pixels_mut().for_each(|(x, y, pixel)| {
                        let staging_offset = y as usize * bytes_per_row_padded + 4 * x as usize;
                        pixel
                            .0
                            .copy_from_slice(&staging_view[staging_offset..staging_offset + 4]);
                    });
                });
            })
        });
    }
}

impl BeginProjectionPass for FdtdWgpuSolverInstance {
    type ProjectionPass<'a>
        = FdtdWgpuProjectionPass<'a>
    where
        Self: 'a;

    fn begin_projection_pass<'a>(
        &'a self,
        state: &'a FdtdWgpuSolverState,
    ) -> FdtdWgpuProjectionPass<'a> {
        FdtdWgpuProjectionPass::new(self, state)
    }
}

#[derive(derive_more::Debug)]
pub struct FdtdWgpuProjectionPass<'a> {
    instance: &'a FdtdWgpuSolverInstance,
    state: &'a FdtdWgpuSolverState,
    command_encoder: wgpu::CommandEncoder,
    swap_buffer_index: SwapBufferIndex,
    #[debug(skip)]
    copy_to_image_buffer: Vec<Box<dyn FnOnce() + 'a>>,
}

impl<'a> FdtdWgpuProjectionPass<'a> {
    fn new(instance: &'a FdtdWgpuSolverInstance, state: &'a FdtdWgpuSolverState) -> Self {
        let command_encoder =
            instance
                .backend
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("fdtd/update/post"),
                });

        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick + 1);

        Self {
            instance,
            state,
            command_encoder,
            swap_buffer_index,
            copy_to_image_buffer: vec![],
        }
    }
}

impl<'a> ProjectionPassAdd<'a, TextureProjection> for FdtdWgpuProjectionPass<'a> {
    fn add_projection(&mut self, projection: &mut TextureProjection) {
        projection.inner.project(
            &mut self.command_encoder,
            self.swap_buffer_index,
            &projection.texture_view,
        );
    }
}

impl<'a> ProjectionPass for FdtdWgpuProjectionPass<'a> {
    fn finish(mut self) {
        self.instance
            .backend
            .submit_and_poll([self.command_encoder.finish()]);

        self.copy_to_image_buffer.drain(..).for_each(|f| f());
    }
}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct ProjectionData {
    projection: Matrix4<f32>,
    color_map: Matrix4<f32>,
}

impl ProjectionData {
    pub fn new(parameters: &ProjectionParameters) -> Self {
        Self {
            projection: parameters.projection,
            color_map: parameters.color_map,
        }
    }
}

#[derive(Debug)]
struct MappedStagingBuffer<'a> {
    pub buffer: &'a wgpu::Buffer,
    pub view: Option<wgpu::BufferView>,
}

impl<'a> MappedStagingBuffer<'a> {
    pub fn view(&mut self) -> &wgpu::BufferView {
        self.view
            .get_or_insert_with(|| self.buffer.get_mapped_range(..))
    }
}

impl<'a> Drop for MappedStagingBuffer<'a> {
    fn drop(&mut self) {
        // make sure any mapped view is dropped first
        drop(self.view.take());

        self.buffer.unmap();
    }
}
