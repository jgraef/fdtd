use std::{
    marker::PhantomData,
    num::NonZero,
    ops::{
        Deref,
        DerefMut,
        Range,
    },
};

use bytemuck::{
    Pod,
    Zeroable,
};
use egui_wgpu::CallbackResources;
use hecs::{
    CommandBuffer,
    Entity,
    Satisfies,
};
use nalgebra::{
    Matrix4,
    Point3,
};
use palette::{
    LinSrgba,
    Srgb,
};

use crate::composer::{
    renderer::{
        camera::{
            CameraData,
            CameraProjection,
            CameraResources,
            Viewport,
        },
        mesh::Mesh,
    },
    scene::{
        Changed,
        Scene,
        SharedShape,
        Transform,
        VisualColor,
    },
};

pub mod camera;
pub mod mesh;

/// Tag for entities that should be rendered
#[derive(Copy, Clone, Debug)]
pub struct Render;

#[derive(Clone, Copy, Debug)]
pub struct ClearColor {
    pub clear_color: Srgb,
}

impl From<Srgb> for ClearColor {
    fn from(value: Srgb) -> Self {
        Self { clear_color: value }
    }
}

impl From<Srgb<u8>> for ClearColor {
    fn from(value: Srgb<u8>) -> Self {
        Self::from(value.into_format::<f32>())
    }
}

impl Default for ClearColor {
    fn default() -> Self {
        Self {
            clear_color: palette::named::ALICEBLUE.into_format(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SurfaceTextureFormat(pub wgpu::TextureFormat);

#[derive(Debug)]
pub struct Renderer {
    camera_bind_group_layout: wgpu::BindGroupLayout,
    clear_pipeline: Pipeline,
    solid_pipeline: Pipeline,
    wiremesh_pipeline: Pipeline,
    instance_buffer: InstanceBuffer<InstanceData>,
    draw_prepared: bool,
    instance_data: Vec<InstanceData>,
    draw_commands: Vec<DrawCommand>,
    enable_solid: bool,
    enable_wireframe: bool,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, target_texture_format: wgpu::TextureFormat) -> Self {
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("camera_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let clear_pipeline = Pipeline::new(
            device,
            "render/clear",
            wgpu::include_wgsl!("shaders/clear.wgsl"),
            &[&camera_bind_group_layout],
            &[],
            false,
            target_texture_format,
            wgpu::PolygonMode::Fill,
            None,
        );

        let solid_pipeline = Pipeline::new_objects(
            device,
            "render/object/solid",
            &camera_bind_group_layout,
            target_texture_format,
            wgpu::PolygonMode::Fill,
            Some("vs_main_solid"),
        );

        let wiremesh_pipeline = Pipeline::new_objects(
            device,
            "render/object/wireframe",
            &camera_bind_group_layout,
            target_texture_format,
            wgpu::PolygonMode::Line,
            Some("vs_main_wireframe"),
        );

        let instance_buffer = InstanceBuffer::new(256, device);

        Self {
            camera_bind_group_layout,
            clear_pipeline,
            solid_pipeline,
            wiremesh_pipeline,
            instance_buffer,
            draw_prepared: false,
            instance_data: vec![],
            draw_commands: vec![],
            enable_solid: true,
            enable_wireframe: true,
        }
    }

    pub fn get(callback_resources: &CallbackResources) -> Option<&Self> {
        callback_resources.get::<Renderer>()
    }

    pub fn get_mut(callback_resources: &mut CallbackResources) -> Option<&mut Self> {
        callback_resources.get_mut::<Renderer>()
    }

    pub fn get_mut_or_init<'a>(
        callback_resources: &'a mut CallbackResources,
        device: &wgpu::Device,
    ) -> &'a mut Self {
        if !callback_resources.contains::<Renderer>() {
            // setup renderer
            // this is a but ugly because we can't just use entry().or_insert_with, because
            // we need to access the callback resources during creation.
            // this will only be run once anyway, so it doesn't matter.
            let target_texture_format = callback_resources
                .get::<SurfaceTextureFormat>()
                .expect("surface texture format not set")
                .0;
            let renderer = Renderer::new(device, target_texture_format);
            callback_resources.insert(renderer);
        }

        callback_resources.get_mut::<Renderer>().unwrap()
    }

    pub fn prepare(&mut self, scene: &mut Scene, device: &wgpu::Device, queue: &wgpu::Queue) {
        // prepare draw
        // note: we only do this once for all views
        if !self.draw_prepared {
            // clear buffers
            self.instance_data.clear();
            self.draw_commands.clear();

            let mut commands = CommandBuffer::new();

            // generate meshes (for rendering) for objects that don't have them yet.
            // todo: we can also do this when the object is created, since we have the wgpu
            // context in the app.
            for (entity, shape) in scene
                .entities
                .query_mut::<&SharedShape>()
                .with::<&Render>()
                .without::<&Mesh>()
            {
                if let Some(mesh) = Mesh::from_shape(&*shape.0, device) {
                    commands.insert_one(entity, mesh);
                }
                else {
                    commands.remove::<(Render,)>(entity);
                }
            }

            // update cameras whose viewports changed
            for (entity, (camera_projection, viewport)) in scene
                .entities
                .query_mut::<(&mut CameraProjection, &Viewport)>()
                .with::<&Changed<Viewport>>()
            {
                // todo: disabled for debugging
                //camera_projection.set_viewport(viewport);
                commands.remove_one::<Changed<Viewport>>(entity);
            }

            // create uniforms for cameras
            for (entity, (camera_projection, camera_transform, clear_color)) in scene
                .entities
                .query_mut::<(&CameraProjection, &Transform, Option<&ClearColor>)>()
                .without::<&CameraResources>()
            {
                tracing::debug!(
                    ?entity,
                    ?camera_projection,
                    ?camera_transform,
                    ?clear_color,
                    "creating camera"
                );
                let camera_data = CameraData::new(camera_projection, camera_transform, clear_color);
                let camera_resources =
                    CameraResources::new(&self.camera_bind_group_layout, device, &camera_data);
                commands.insert_one(entity, camera_resources);
            }

            // update uniforms for cameras
            for (_, (camera_resources, camera_projection, camera_transform, clear_color)) in
                scene.entities.query_mut::<(
                    &mut CameraResources,
                    &CameraProjection,
                    &Transform,
                    Option<&ClearColor>,
                )>()
            {
                let camera_data = CameraData::new(camera_projection, camera_transform, clear_color);
                camera_resources.update(queue, &camera_data);
            }

            // apply buffered commands to world
            commands.run_on(&mut scene.entities);

            // prepare the actual draw commands
            let mut first_instance = 0;
            for (_, (transform, mesh, color)) in scene
                .entities
                .query_mut::<(&Transform, &Mesh, &VisualColor)>()
                .with::<&Render>()
            {
                // write per-instance data into a buffer
                self.instance_data.push(InstanceData::new(transform, color));

                // for now every draw call will only draw one instance, but we could do
                // instancing for real later.
                let instances = first_instance..(first_instance + 1);

                // prepare draw commands for actual drawing in `paint`
                self.draw_commands.push(DrawCommand {
                    instances,
                    mesh: mesh.clone(),
                });

                first_instance += 1;
            }

            // send instance data to gpu
            self.instance_buffer
                .write(&self.instance_data, device, queue);

            // the current render pass is fully prepared
            self.draw_prepared = true;
        }
    }

    pub fn finish_prepare(&mut self) {
        // all prepare calls are done, so we can reset the flag for the next frame.
        self.draw_prepared = false;

        // note: don't clear the buffer here! paint is called after this.
    }

    pub fn render(
        &self,
        camera_entity: Entity,
        scene: &mut Scene,
        info: &egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
    ) {
        let mut commands = CommandBuffer::new();

        if let Ok((camera_resources, has_clear_color, viewport)) =
            scene.entities.query_one_mut::<(
                &CameraResources,
                Satisfies<&ClearColor>,
                Option<&mut Viewport>,
            )>(camera_entity)
        {
            // update camera viewport (takes effect next frame)
            if let Some(viewport) = viewport {
                if info.viewport != viewport.viewport {
                    viewport.viewport = info.viewport;
                    commands.insert_one(camera_entity, Changed::<Viewport>::default());
                }
            }
            else {
                commands.insert(
                    camera_entity,
                    (
                        Viewport {
                            viewport: info.viewport,
                        },
                        Changed::<Viewport>::default(),
                    ),
                );
            }

            // set camera
            render_pass.set_bind_group(0, &camera_resources.bind_group, &[]);

            if has_clear_color {
                // clear
                render_pass.set_pipeline(&self.clear_pipeline.pipeline);
                render_pass.draw(0..3, 0..1);
            }

            // set instance buffer (this is shared between all draw calls)
            render_pass.set_vertex_buffer(1, self.instance_buffer.buffer().slice(..));

            // render all objects with the solid and/or wireframe pipeline
            if self.enable_solid {
                self.solid_pipeline
                    .render_objects(render_pass, &self.draw_commands);
            }

            if self.enable_wireframe {
                self.wiremesh_pipeline
                    .render_objects(render_pass, &self.draw_commands);
            }
        }
    }
}

#[derive(Debug)]
struct Pipeline {
    shader_module: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
}

impl Pipeline {
    pub fn new(
        device: &wgpu::Device,
        label: &str,
        shader_module_desc: wgpu::ShaderModuleDescriptor,
        bind_group_layouts: &[&wgpu::BindGroupLayout],
        vertex_buffer_layouts: &[wgpu::VertexBufferLayout],
        depth: bool,
        target_texture_format: wgpu::TextureFormat,
        polygon_mode: wgpu::PolygonMode,
        vertex_shader_entry_point: Option<&str>,
    ) -> Self {
        let shader_module = device.create_shader_module(shader_module_desc);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts,
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: vertex_shader_entry_point,
                compilation_options: Default::default(),
                buffers: vertex_buffer_layouts,
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                //cull_mode: Some(wgpu::Face::Front),
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                // we don't need to write depth for the clear pipeline. egui_wgpu clears the depth
                // buffer when it creates the render pass.
                depth_write_enabled: depth,
                depth_compare: if depth {
                    // egui_wgpu clears the depth buffer with 1.0. Smaller depth values are closer
                    // to the camera (-z pointing out of screen)
                    wgpu::CompareFunction::LessEqual
                }
                else {
                    wgpu::CompareFunction::Always
                },
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: None,
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_texture_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        Self {
            shader_module,
            pipeline_layout,
            pipeline,
        }
    }

    pub fn new_objects(
        device: &wgpu::Device,
        label: &str,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        target_texture_format: wgpu::TextureFormat,
        polygon_mode: wgpu::PolygonMode,
        vertex_shader_entry_point: Option<&str>,
    ) -> Self {
        Self::new(
            device,
            label,
            wgpu::include_wgsl!("shaders/solid.wgsl"),
            &[&camera_bind_group_layout],
            &[
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Point3<f32>>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        // vertex position
                        0 => Float32x3,
                    ],
                },
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<InstanceData>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        // model matrix
                        1 => Float32x4,
                        2 => Float32x4,
                        3 => Float32x4,
                        4 => Float32x4,
                        // solid color
                        5 => Float32x4,
                        // wireframe color
                        6 => Float32x4,
                    ],
                },
            ],
            true,
            target_texture_format,
            polygon_mode,
            vertex_shader_entry_point,
        )
    }

    /// Helper function to render objects with a given pipeline.
    ///
    /// Obviously the pipeline must be compatible. This works
    /// with solid or wireframe rendering
    fn render_objects(
        &self,
        render_pass: &mut wgpu::RenderPass<'static>,
        draw_commands: &[DrawCommand],
    ) {
        // set draw (solid) pipeline
        render_pass.set_pipeline(&self.pipeline);

        // issue draw commands
        for draw_command in draw_commands {
            render_pass.set_index_buffer(
                draw_command.mesh.index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            render_pass.set_vertex_buffer(0, draw_command.mesh.vertex_buffer.slice(..));

            render_pass.draw_indexed(
                draw_command.mesh.indices.clone(),
                draw_command.mesh.base_vertex,
                draw_command.instances.clone(),
            );
        }
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct InstanceData {
    transform: Matrix4<f32>,
    solid_color: LinSrgba,
    wireframe_color: LinSrgba,
}

impl InstanceData {
    pub fn new(transform: &Transform, color: &VisualColor) -> Self {
        Self {
            // note that since we pass this through a vertex buffer, we tell wgpu that this is 4
            // vec4f's.
            transform: transform.transform.cast().to_homogeneous(),
            // shaders work with linear colors
            solid_color: color.solid_color.into_linear(),
            wireframe_color: color.wireframe_color.into_linear(),
        }
    }
}

#[derive(Debug)]
struct DrawCommand {
    instances: Range<u32>,
    // note: we could also just store the entity id here and lookup the mesh in the paint call. but
    // `Mesh` is just 2 Arcs and a couple of integers.
    mesh: Mesh,
}

#[derive(Debug)]
pub struct InstanceBuffer<T> {
    buffer: wgpu::Buffer,
    capacity: usize,
    _phantom: PhantomData<[T]>,
}

impl<T> InstanceBuffer<T> {
    pub fn new(initial_capacity: usize, device: &wgpu::Device) -> Self {
        Self {
            buffer: allocate_instance_buffer::<T>(initial_capacity, device),
            capacity: initial_capacity,
            _phantom: PhantomData,
        }
    }

    pub fn resize(&mut self, new_capacity: usize, device: &wgpu::Device) {
        assert!(new_capacity != 0);
        self.buffer = allocate_instance_buffer::<T>(new_capacity, device);
    }

    pub fn resize_if_necessary(&mut self, needed_capacity: usize, device: &wgpu::Device) {
        if needed_capacity > self.capacity {
            self.resize((2 * self.capacity).max(needed_capacity), device);
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub fn write_view(&mut self, queue: &wgpu::Queue) -> InstanceBufferWriteView<'_, T> {
        let size = NonZero::new(buffer_size::<T>(self.capacity)).unwrap();
        let view = queue.write_buffer_with(&self.buffer, 0, size).unwrap();
        InstanceBufferWriteView {
            view,
            _phantom: PhantomData,
        }
    }
}

impl<T: Pod> InstanceBuffer<T> {
    pub fn write(&mut self, data: &[T], device: &wgpu::Device, queue: &wgpu::Queue) {
        self.resize_if_necessary(data.len(), device);
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(data));
    }
}

pub struct InstanceBufferWriteView<'a, T> {
    view: wgpu::QueueWriteBufferView,
    _phantom: PhantomData<&'a [T]>,
}

impl<'a, T: Pod> Deref for InstanceBufferWriteView<'a, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        bytemuck::cast_slice(&*self.view)
    }
}

impl<'a, T: Pod> DerefMut for InstanceBufferWriteView<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        bytemuck::cast_slice_mut(&mut *self.view)
    }
}

fn buffer_size<T>(num_elements: usize) -> u64 {
    // fixme: this needs alignment/rounding
    (std::mem::size_of::<T>() * num_elements) as u64
}

fn allocate_instance_buffer<T>(capacity: usize, device: &wgpu::Device) -> wgpu::Buffer {
    let size = buffer_size::<T>(capacity);
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instance buffer"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
