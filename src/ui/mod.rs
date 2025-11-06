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
use hecs::{
    CommandBuffer,
    Entity,
};
use nalgebra::{
    Matrix4,
    Point3,
    Projective3,
};
use palette::{
    LinSrgba,
    Srgba,
    WithAlpha,
};
use wgpu::util::DeviceExt;

use crate::{
    SurfaceTextureFormat,
    geometry::scene::{
        Shape,
        SharedShape,
        SharedWorld,
        SurfaceMesh,
        Transform,
        VisualColor,
    },
};

#[derive(Clone, Debug)]
pub struct SceneView {
    pub world: SharedWorld,
    pub camera: Option<Entity>,
}

impl egui::Widget for SceneView {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let response = ui.allocate_response(ui.available_size(), egui::Sense::empty());

        let painter = ui.painter();
        painter.add(egui_wgpu::Callback::new_paint_callback(
            response.rect,
            RenderCallback {
                world: self.world.clone(),
                camera: self.camera,
            },
        ));

        response
    }
}
struct RenderCallback {
    world: SharedWorld,
    camera: Option<Entity>,
}

impl egui_wgpu::CallbackTrait for RenderCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let mut world = self.world.world_mut();

        // setup renderer
        let renderer = {
            if let Some(renderer) = callback_resources.get_mut::<Renderer>() {
                renderer
            }
            else {
                // this is a but ugly because we can't just use entry().or_insert_with, because
                // we need to access the callback resources during creation.
                // this will only be run once anyway, so it doesn't matter.
                let target_texture_format = callback_resources
                    .get::<SurfaceTextureFormat>()
                    .expect("surface texture format not set")
                    .0;
                let renderer = Renderer::new(device, target_texture_format);
                callback_resources.insert(renderer);
                callback_resources.get_mut::<Renderer>().unwrap()
            }
        };

        // prepare draw
        // note: we only do this once for all views
        if !renderer.draw_prepared {
            // clear buffers
            renderer.instance_data.clear();
            renderer.draw_commands.clear();

            let mut commands = CommandBuffer::new();

            // generate meshes (for rendering) for objects that don't have them yet.
            // todo: we can also do this when the object is created, since we have the wgpu
            // context in the app.
            for (entity, shape) in world
                .entities
                .query_mut::<&SharedShape>()
                .with::<&Render>()
                .without::<&Mesh>()
            {
                if let Some(mesh) = Mesh::from_shape(&*shape.0, device) {
                    commands.insert(entity, (mesh,));
                }
                else {
                    commands.remove::<(Render,)>(entity);
                }
            }

            // create or update uniforms for cameras
            // todo: update when transform/projection changes
            for (entity, (camera, camera_transform)) in world
                .entities
                .query_mut::<(&Camera, &Transform)>()
                .without::<&CameraResources>()
            {
                let camera_data = CameraData::new(camera, camera_transform);
                let resources = CameraResources::new(renderer, device, &camera_data);
                commands.insert(entity, (resources,));
            }

            // apply buffered commands to world
            commands.run_on(&mut world.entities);

            // prepare the actual draw commands
            let mut first_instance = 0;
            for (_, (transform, mesh, color)) in world
                .entities
                .query_mut::<(&Transform, &Mesh, &VisualColor)>()
                .with::<&Render>()
            {
                // write per-instance data into a buffer
                renderer
                    .instance_data
                    .push(InstanceData::new(transform, color));

                // for now every draw call will only draw one instance, but we could do
                // instancing for real later.
                let instances = first_instance..(first_instance + 1);

                // prepare draw commands for actual drawing in `paint`
                renderer.draw_commands.push(DrawCommand {
                    instances,
                    mesh: mesh.clone(),
                });

                first_instance += 1;
            }

            // send instance data to gpu
            renderer
                .instance_buffer
                .write(&renderer.instance_data, device, queue);

            // the current render pass is fully prepared
            renderer.draw_prepared = true;
        }

        vec![]
    }

    fn finish_prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = callback_resources.get_mut::<Renderer>() {
            // all prepare calls are done, so we can reset the flag for the next frame.
            renderer.draw_prepared = false;

            // note: don't clear the buffer here! paint is called after this.
        }

        vec![]
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(renderer) = callback_resources.get::<Renderer>() {
            if let Some(camera_entity) = self.camera {
                let world = self.world.world();

                if let Ok(mut camera_query) = world
                    .entities
                    .query_one::<(&CameraResources, &Camera)>(camera_entity)
                {
                    let (camera_resources, camera) = camera_query.get().unwrap();

                    // set camera
                    render_pass.set_bind_group(0, &camera_resources.bind_group, &[]);

                    if camera.clear_color.is_some() {
                        // clear
                        render_pass.set_pipeline(&renderer.clear_pipeline.pipeline);
                        render_pass.draw(0..3, 0..1);
                    }

                    // set instance buffer (this is shared between all draw calls)
                    render_pass.set_vertex_buffer(1, renderer.instance_buffer.buffer().slice(..));

                    // render all objects with the solid and/or wireframe pipeline
                    if renderer.enable_solid {
                        renderer
                            .solid_pipeline
                            .render_objects(render_pass, &renderer.draw_commands);
                    }

                    if renderer.enable_wireframe {
                        renderer
                            .wiremesh_pipeline
                            .render_objects(render_pass, &renderer.draw_commands);
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct Renderer {
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
            true,
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
                depth_write_enabled: depth,
                depth_compare: if depth {
                    wgpu::CompareFunction::Always
                }
                else {
                    wgpu::CompareFunction::Less
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

/// Tag for entities that should be rendered
#[derive(Copy, Clone, Debug)]
pub struct Render;

#[derive(Clone, Debug)]
struct Mesh {
    index_buffer: wgpu::Buffer,
    vertex_buffer: wgpu::Buffer,
    indices: Range<u32>,
    base_vertex: i32,
}

impl Mesh {
    pub fn from_surface_mesh(surface_mesh: &SurfaceMesh, device: &wgpu::Device) -> Self {
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh index buffer"),
            contents: bytemuck::cast_slice(&surface_mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertex buffer"),
            contents: bytemuck::cast_slice(&surface_mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let num_indices = surface_mesh.indices.len() as u32;

        Self {
            index_buffer,
            vertex_buffer,
            indices: 0..num_indices,
            base_vertex: 0,
        }
    }

    pub fn from_shape<S: Shape + ?Sized>(shape: &S, device: &wgpu::Device) -> Option<Self> {
        shape
            .to_surface_mesh()
            .map(|surface_mesh| Self::from_surface_mesh(&surface_mesh, device))
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
            transform: transform.transform.cast().to_homogeneous().transpose(),
            // shaders work with linear colors
            solid_color: color.solid_color.into_linear(),
            wireframe_color: color.wireframe_color.into_linear(),
        }
    }
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

#[derive(Debug)]
struct DrawCommand {
    instances: Range<u32>,
    // note: we could also just store the entity id here and lookup the mesh in the paint call. but
    // `Mesh` is just 2 Arcs and a couple of integers.
    mesh: Mesh,
}

pub struct Camera {
    pub projection: Projective3<f32>,
    pub clear_color: Option<Srgba>,
}

impl Default for Camera {
    fn default() -> Self {
        //let perspective = Perspective3::new(1.0, FRAC_PI_2, -1.0, 1.0)
        Self {
            projection: Projective3::identity(),
            clear_color: Some(palette::named::ALICEBLUE.into_format().with_alpha(1.0)),
            //clear_color: None,
        }
    }
}

#[derive(Clone, Debug)]
struct CameraResources {
    dirty: bool,
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl CameraResources {
    pub fn new(renderer: &Renderer, device: &wgpu::Device, camera_data: &CameraData) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera uniform buffer"),
            contents: bytemuck::bytes_of(camera_data),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera uniform bind group"),
            layout: &renderer.camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(buffer.as_entire_buffer_binding()),
            }],
        });

        Self {
            dirty: false,
            buffer,
            bind_group,
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, camera_data: &CameraData) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(camera_data));

        self.dirty = false;
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct CameraData {
    view_matrix: Matrix4<f32>,
    clear_color: LinSrgba,
}

impl CameraData {
    pub fn new(camera: &Camera, camera_transform: &Transform) -> Self {
        Self {
            // apply inverse transform of camera, then projection
            view_matrix: camera.projection.to_homogeneous()
                * camera_transform.transform.inverse().to_homogeneous(),
            // note: shaders always work with linear colors.
            clear_color: camera
                .clear_color
                .map(|clear_color| clear_color.into_linear())
                .unwrap_or_default(),
        }
    }
}
