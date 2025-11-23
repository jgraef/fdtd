pub mod camera;
mod command;
mod draw_commands;
pub mod grid;
pub mod light;
pub mod loader;
pub mod mesh;
pub mod texture_channel;

use std::{
    num::NonZero,
    sync::Arc,
};

use bitflags::bitflags;
use bytemuck::{
    Pod,
    Zeroable,
};
use nalgebra::{
    Matrix4,
    Vector2,
};
use palette::{
    Srgb,
    Srgba,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    Error,
    app::composer::{
        properties::{
            PropertiesUi,
            TrackChanges,
            label_and_value,
        },
        renderer::{
            camera::{
                CameraConfig,
                CameraResources,
            },
            command::{
                Command,
                CommandQueue,
            },
            draw_commands::{
                DrawCommand,
                DrawCommandBuffer,
                DrawCommandEnablePipelineFlags,
                DrawCommandOptions,
            },
            light::{
                LoadMaterialTextures,
                Material,
                MaterialData,
                MaterialTextures,
                TextureAndView,
                TextureSource,
            },
            loader::{
                Loader,
                LoadingProgress,
                RunLoaders,
                TextureCache,
            },
            mesh::{
                Mesh,
                MeshBindGroup,
                WindingOrder,
            },
        },
        scene::{
            Scene,
            transform::Transform,
        },
    },
    util::wgpu::{
        StagedTypedArrayBuffer,
        WriteImageToTextureExt,
        texture_descriptor,
        texture_view_from_color,
    },
};

/// Tag for entities that should be rendered
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Render;

// todo: respect eguis theme. we might just pass this in from the view when
// rendering and remove this component.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
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

#[derive(Clone, Debug)]
pub struct WgpuContext {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub renderer_config: RendererConfig,

    /// should this go in here? we need it to get egui texture handles for
    /// textures if we want to render them in widgets, and the other way if we
    /// want to get a wgpu texture from a egui texture id
    pub egui_wgpu_renderer: EguiWgpuRenderer,
}

#[derive(Clone, Copy, Debug)]
pub struct RendererConfig {
    pub target_texture_format: wgpu::TextureFormat,
    pub depth_texture_format: Option<wgpu::TextureFormat>,
    pub multisample_count: NonZero<u32>,
}

#[derive(Clone, derive_more::Debug)]
pub struct EguiWgpuRenderer {
    #[debug(skip)]
    inner: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
}

impl From<Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>> for EguiWgpuRenderer {
    fn from(value: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>) -> Self {
        Self { inner: value }
    }
}

/// # Notes
///
/// We use a left-handed coordinate system both in the scene and in NDC:
///  - x: from left to right
///  - y: from bottom to top
///  - z: from outside to inside of screen
///
/// Each [`Renderer`] can only render one scene at once, but through multiple
/// cameras.
///
/// # TODO
///
/// Split per-scene render-state from reusable state. Only `instance_buffer`,
/// `instance_bind_group` and `draw_command_buffer` are specific to the scene.
/// Everything else can be shared between renderers for multiple scenes. So we
/// could separate these out, and e.g. put them into a resource into the scene.
/// Note that `instance_data` can be shared. It's just a scratch buffer that is
/// used during `prepare_world`. Same for `command_buffer`.
#[derive(Debug)]
pub struct Renderer {
    wgpu_context: WgpuContext,

    camera_bind_group_layout: wgpu::BindGroupLayout,
    mesh_bind_group_layout: wgpu::BindGroupLayout,

    // this is actually used for everything, not just meshes. but we might split it into clear,
    // mesh, etc.
    mesh_shader_module: wgpu::ShaderModule,

    clear_pipeline: Pipeline,
    solid_pipeline: Pipeline,
    wireframe_pipeline: Pipeline,
    outline_pipeline: Pipeline,

    /// The instance buffer.
    ///
    /// This holds the handle to the GPU buffer for the instance data, a
    /// host staging buffer for the instance data, and the bind group for the
    /// GPU buffer.
    instance_buffer: StagedTypedArrayBuffer<InstanceData>,

    /// This stores all draw commands that are generated during `prepare_world`.
    /// Its `finish` method returns the finalized draw command (aggregate) for a
    /// specific camera.
    draw_command_buffer: DrawCommandBuffer,

    /// Fallbacks for textures and sampler
    texture_defaults: Fallbacks,

    /// Command queue to asynchronously send commands to the renderer.
    ///
    /// The queue is checked in [`prepare_world`](Self::prepare_world). Senders
    /// are e.g. handed out to
    /// [`TextureSender`s](texture_channel::TextureSender).
    command_queue: CommandQueue,

    /// Texture cache
    ///
    /// Caches textures loaded from files.
    texture_cache: TextureCache,
}

impl Renderer {
    /// The winding order used by the renderer (indicating the front face of a
    /// polygon).
    ///
    /// Apparently this is the default for left-handed coordinate systems (not
    /// sure why it matters). We use this either way, and will tell the
    /// vertex shader to fix the ordering for meshes that are wound opposite
    /// (the ones generated by parry clockwise apparently)
    pub const WINDING_ORDER: WindingOrder = WindingOrder::CounterClockwise;

    pub const MESH_SHADER_MODULE: wgpu::ShaderModuleDescriptor<'static> =
        wgpu::include_wgsl!("shaders/mesh.wgsl");

    pub fn new(wgpu_context: &WgpuContext) -> Self {
        let camera_bind_group_layout =
            wgpu_context
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("camera_bind_group_layout"),
                    entries: &[
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
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        let mesh_bind_group_layout = {
            let vertex_buffer = |binding| {
                wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }
            };

            let texture = |binding| {
                wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }
            };

            wgpu_context
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("mesh_bind_group_layout"),
                    entries: &[
                        // index buffer
                        vertex_buffer(0),
                        // vertex buffer
                        vertex_buffer(1),
                        // uv buffer
                        vertex_buffer(2),
                        // sampler
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        // material - ambient
                        texture(4),
                        // material - diffuse
                        texture(5),
                        // material - specular
                        texture(6),
                        // material - emissive
                        texture(7),
                    ],
                })
        };

        // should we store this in the struct?
        let mesh_shader_module = wgpu_context
            .device
            .create_shader_module(Self::MESH_SHADER_MODULE);

        let clear_pipeline = Pipeline::new(
            wgpu_context,
            "render/clear",
            &mesh_shader_module,
            &[&camera_bind_group_layout],
            DepthState {
                // egui clears the depth buffer with a value of 1.0 when starting the render pass.
                // but we could enable this to write to the depth buffer with the clear shader - if
                // we want to clear to another value.
                write_enable: false,
                // render regardless of depth state
                compare: wgpu::CompareFunction::Always,
                bias: Default::default(),
            },
            {
                // this should always zero the stencil state
                let stencil_face_state = wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Always,
                    fail_op: wgpu::StencilOperation::Zero,
                    depth_fail_op: wgpu::StencilOperation::Zero,
                    pass_op: wgpu::StencilOperation::Zero,
                };
                wgpu::StencilState {
                    // same stencil face state for both faces
                    front: stencil_face_state,
                    back: stencil_face_state,
                    // ignore buffer state
                    read_mask: 0x00,
                    // clear bits
                    // I thought a value of 0x00 would do it here, but the `is_enabled` method would
                    // return false then.
                    write_mask: 0xff,
                }
            },
            wgpu::PrimitiveTopology::TriangleList,
            Some("vs_main_clear"),
            Some("fs_main_single_color"),
            false,
        );

        let render_mesh_bind_group_layouts = [&camera_bind_group_layout, &mesh_bind_group_layout];

        let solid_pipeline = Pipeline::new_mesh_render_pipeline(
            wgpu_context,
            "render/object/solid",
            &mesh_shader_module,
            &render_mesh_bind_group_layouts,
            wgpu::CompareFunction::Less,
            Some(Stencil::OUTLINE),
            None,
            wgpu::PrimitiveTopology::TriangleList,
            Some("vs_main_solid"),
            Some("fs_main_solid"),
        );

        let wireframe_pipeline = Pipeline::new_mesh_render_pipeline(
            wgpu_context,
            "render/object/wireframe",
            &mesh_shader_module,
            &render_mesh_bind_group_layouts,
            wgpu::CompareFunction::LessEqual,
            None,
            None,
            wgpu::PrimitiveTopology::LineList,
            Some("vs_main_wireframe"),
            Some("fs_main_single_color"),
        );

        let outline_pipeline = Pipeline::new_mesh_render_pipeline(
            wgpu_context,
            "render/object/outline",
            &mesh_shader_module,
            &render_mesh_bind_group_layouts,
            wgpu::CompareFunction::Always,
            None,
            // mask used, todo: don't hardcode values like this :D
            Some(Stencil::OUTLINE),
            wgpu::PrimitiveTopology::TriangleList,
            Some("vs_main_outline"),
            Some("fs_main_single_color"),
        );

        let instance_buffer = StagedTypedArrayBuffer::with_capacity(
            &wgpu_context.device,
            "instance buffer",
            wgpu::BufferUsages::STORAGE,
            128,
        );
        assert!(instance_buffer.buffer.is_allocated());

        let texture_defaults = Fallbacks::new(&wgpu_context.device, &wgpu_context.queue);

        Self {
            wgpu_context: wgpu_context.clone(),
            camera_bind_group_layout,
            mesh_bind_group_layout,
            mesh_shader_module,
            clear_pipeline,
            solid_pipeline,
            wireframe_pipeline,
            outline_pipeline,
            instance_buffer,
            draw_command_buffer: Default::default(),
            texture_defaults,
            command_queue: CommandQueue::new(512),
            texture_cache: Default::default(),
        }
    }

    /// Prepares for the world to be rendered
    ///
    /// This must be called once a frame before any views render using it.
    ///
    /// This will update rendering-related information in the world (e.g. by
    /// creating meshes from shapes), update GPU buffers when the relevant
    /// world-state changed and prepare the draw calls for this frame.
    ///
    /// All draw calls are prepared here since they can be shared by multiple
    /// views that render the same scene. Internally they're put into a
    /// `Arc<Vec<_>>`, so they're cheap to clone (which is done in
    /// [`Self::prepare_frame`]).
    pub fn prepare_world(&mut self, scene: &mut Scene) -> Result<(), Error> {
        // handle command queue
        self.handle_commands();

        // load rendering assets
        let mut run_loaders = RunLoaders::new(self, scene);
        let load_result = run_loaders.run::<LoadMaterialTextures>();
        if let Err(error) = &load_result {
            tracing::warn!(?error);
        }

        // generate meshes (for rendering) for objects that don't have them yet.
        mesh::generate_meshes_for_shapes(
            scene,
            &self.wgpu_context.device,
            &self.mesh_bind_group_layout,
            &self.texture_defaults,
        );

        // next we prepare the draw commands.
        // this is done once in prepare, so multiple view widgets can then use the draw
        // commands to render their scenes from different cameras.
        // the draw commands are stored in an `Arc<Vec<_>>`, so they can be easily sent
        // via paint callbacks.
        let instance_buffer_reallocated =
            self.update_instance_buffer_and_draw_command(&mut scene.entities);

        // update cameras
        camera::update_cameras(
            scene,
            &self.wgpu_context.device,
            &self.wgpu_context.queue,
            &self.camera_bind_group_layout,
            self.instance_buffer.buffer.buffer().unwrap(),
            instance_buffer_reallocated,
        );

        load_result
    }

    fn update_instance_buffer_and_draw_command(&mut self, world: &mut hecs::World) -> bool {
        // for now every draw call will only draw one instance, but we could do
        // instancing for real later.
        let mut first_instance = 0;
        let mut instances = || {
            let instances = first_instance..(first_instance + 1);
            first_instance += 1;
            instances
        };

        // this is the second time I have a bug with this. keep this assert!
        assert!(
            self.instance_buffer.staging.is_empty(),
            "instance scratch buffer hasn't been cleared yet"
        );

        // prepare the actual draw commands
        let mut draw_command_builder = self.draw_command_buffer.builder();

        // draw meshes (solid, wireframe, outlines)
        for (_, (transform, mesh, mesh_bind_group, material, material_textures, outline)) in world
            .query_mut::<(
                &Transform,
                &Mesh,
                &MeshBindGroup,
                Option<&Material>,
                Option<&MaterialTextures>,
                Option<&Outline>,
            )>()
            .with::<&Render>()
        {
            // instance flags
            let mut flags = InstanceFlags::SHOW_SOLID
                | InstanceFlags::SHOW_WIREFRAME
                | InstanceFlags::ENABLE_TEXTURES;
            if mesh.winding_order != Self::WINDING_ORDER {
                flags |= InstanceFlags::REVERSE_WINDING;
            }
            if mesh.uv_buffer.is_some() {
                flags |= InstanceFlags::UV_BUFFER_VALID;
            }

            // write per-instance data into a buffer
            self.instance_buffer.push(InstanceData::new_mesh(
                transform,
                material,
                material_textures,
                flags,
                mesh.base_vertex,
                outline,
            ));

            // prepare draw commands
            draw_command_builder.draw_mesh(instances(), mesh, mesh_bind_group, outline.is_some());
        }

        // send instance data to gpu
        self.instance_buffer
            .flush(&self.wgpu_context.queue, |_buffer| {})
    }

    /// Prepares rendering a frame for a specific view.
    ///
    /// This just fetches camera information and the prepared draw commands
    /// (from [`Self::prepare_world`]) and returns them as a [`DrawCommand`].
    ///
    /// The [`DrawCommand`] can be cloned and passed via [`egui::PaintCallback`]
    /// to do the actual rendering with a [`wgpu::RenderPass`].
    ///
    /// Note that the actual draw commands are prepared in
    /// [`Self::prepare_world`], since they can be shared by multiple view
    /// widgets.
    pub fn prepare_frame(
        &mut self,
        camera_entity: Option<hecs::EntityRef<'_>>,
    ) -> Option<DrawCommand> {
        // get bind group and config for our camera
        let Some((camera_bind_group, camera_config, has_clear_color)) =
            camera_entity.and_then(|camera_entity| {
                let mut query = camera_entity.query::<(
                    &CameraResources,
                    Option<&CameraConfig>,
                    hecs::Satisfies<&ClearColor>,
                )>();

                query
                    .get()
                    .map(|(camera_resources, camera_config, has_clear_color)| {
                        (
                            camera_resources.bind_group.clone(),
                            camera_config.cloned().unwrap_or_default(),
                            has_clear_color,
                        )
                    })
            })
        else {
            // fixme: use a fallback camera buffer to call the clear shader only.

            // if we don't have a camera we can't do anything. since we can't bind a camera
            // bind group, we can't even clear. we could have a fallback camera
            // bind group that is only used for clearing. or separate the clear color from
            // the camera.
            return None;
        };

        // default to all, then apply configuration, so by default stuff will render and
        // we don't have to debug for 15 minutes to find that we don't enable the
        // pipeline
        let mut pipeline_enable_flags = DrawCommandEnablePipelineFlags::all();
        pipeline_enable_flags.set(DrawCommandEnablePipelineFlags::CLEAR, has_clear_color);
        camera_config.apply_to_pipeline_enable_flags(&mut pipeline_enable_flags);

        Some(self.draw_command_buffer.finish(
            self,
            camera_bind_group,
            DrawCommandOptions {
                pipeline_enable_flags,
            },
        ))
    }

    fn create_texture(&self, size: &Vector2<u32>, label: &str) -> wgpu::Texture {
        self.wgpu_context
            .device
            .create_texture(&texture_descriptor(size, label))
    }

    fn load_texture(
        &mut self,
        texture_source: &mut TextureSource,
    ) -> Result<LoadingProgress<Arc<TextureAndView>>, Error> {
        match texture_source {
            TextureSource::File { path } => {
                let texture_and_view = self.texture_cache.get_or_insert(path, || {
                    Ok::<_, Error>(Arc::new(TextureAndView::from_path(
                        &self.wgpu_context.device,
                        &self.wgpu_context.queue,
                        &path,
                    )?))
                })?;
                Ok(LoadingProgress::Ready(texture_and_view))
            }
            TextureSource::Channel { receiver } => {
                let texture_and_view = receiver
                    .register(&self.command_queue.sender, |size, label| {
                        self.create_texture(size, label)
                    });
                Ok(texture_and_view.into())
            }
        }
    }

    pub fn copy_image_to_texture(&self, image: &image::RgbaImage, texture: &wgpu::Texture) {
        image.write_to_texture(&self.wgpu_context.queue, texture);
    }

    fn handle_commands(&mut self) {
        // note: for now we handle everything on the same thread, having &mut access to
        // the whole renderer. but many commands we would better handle in a separate
        // thread (e.g. ones that only require access to device/queue).

        for command in self.command_queue.receiver.drain() {
            match command {
                Command::CopyImageToTexture(command) => {
                    command.handle(|image, texture| self.copy_image_to_texture(image, texture));
                }
                Command::CreateTextureForChannel(command) => {
                    command.handle(|size, label| self.create_texture(size, label))
                }
            }
        }
    }
}

fn create_instance_bind_group(
    device: &wgpu::Device,
    instance_bind_group_layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("instance bind group"),
        layout: instance_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

#[derive(Debug)]
struct Pipeline {
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
}

impl Pipeline {
    #[allow(clippy::too_many_arguments)]
    fn new(
        wgpu_context: &WgpuContext,
        label: &str,
        shader_module: &wgpu::ShaderModule,
        bind_group_layouts: &[&wgpu::BindGroupLayout],
        depth_state: DepthState,
        stencil_state: wgpu::StencilState,
        topology: wgpu::PrimitiveTopology,
        vertex_shader_entry_point: Option<&str>,
        fragment_shader_entry_point: Option<&str>,
        cull_back_faces: bool,
    ) -> Self {
        let cull_mode = cull_back_faces.then_some(wgpu::Face::Back);

        // We need to flip the interpretation of the winding order here, because this
        // actually depends on the orientation of our Z axis.
        let front_face = Renderer::WINDING_ORDER.flipped().front_face();

        let pipeline_layout =
            wgpu_context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(label),
                    bind_group_layouts,
                    push_constant_ranges: &[],
                });

        let pipeline =
            wgpu_context
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: shader_module,
                        entry_point: vertex_shader_entry_point,
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology,
                        strip_index_format: None,
                        front_face,
                        cull_mode,
                        unclipped_depth: false,
                        polygon_mode: Default::default(),
                        conservative: false,
                    },
                    depth_stencil: wgpu_context.renderer_config.depth_texture_format.map(
                        |depth_texture_format| {
                            wgpu::DepthStencilState {
                                format: depth_texture_format,
                                depth_write_enabled: depth_state.write_enable,
                                depth_compare: depth_state.compare,
                                stencil: stencil_state,
                                bias: depth_state.bias,
                            }
                        },
                    ),
                    multisample: wgpu::MultisampleState {
                        count: wgpu_context.renderer_config.multisample_count.get(),
                        mask: !0,
                        alpha_to_coverage_enabled: false,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: shader_module,
                        entry_point: fragment_shader_entry_point,
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu_context.renderer_config.target_texture_format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview: None,
                    cache: None,
                });

        Self {
            pipeline_layout,
            pipeline,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new_mesh_render_pipeline(
        wgpu_context: &WgpuContext,
        label: &str,
        shader_module: &wgpu::ShaderModule,
        bind_group_layouts: &[&wgpu::BindGroupLayout],
        depth_compare: wgpu::CompareFunction,
        write_to_stencil_buffer: Option<Stencil>,
        test_against_stencil_buffer: Option<Stencil>,
        topology: wgpu::PrimitiveTopology,
        vertex_shader_entry_point: Option<&str>,
        fragment_shader_entry_point: Option<&str>,
    ) -> Self {
        // enable/disable back-face culling. the mesh shader will paint back-faces
        // bright pink, so we'll know if something is flipped.
        const CULL_BACK_FACES: bool = true;

        let cull_back_faces = match topology {
            wgpu::PrimitiveTopology::PointList
            | wgpu::PrimitiveTopology::LineList
            | wgpu::PrimitiveTopology::LineStrip => false,
            wgpu::PrimitiveTopology::TriangleList | wgpu::PrimitiveTopology::TriangleStrip => {
                CULL_BACK_FACES
            }
        };

        let mut stencil_state = wgpu::StencilState::default();
        if let Some(write_mask) = write_to_stencil_buffer {
            // write stencil reference to stencil buffer
            stencil_state.front.pass_op = wgpu::StencilOperation::Replace;

            // remove this and the selected object will shine through anything occluding
            // with the outline color. give it a try :)
            stencil_state.front.depth_fail_op = wgpu::StencilOperation::Replace;

            // for now we'll replace all bits of the stencil buffer with the reference
            stencil_state.write_mask = write_mask.into();
        }
        if let Some(read_mask) = test_against_stencil_buffer {
            // check masked stencil buffer against stencil reference.

            // e.g. for outline drawing, when drawing the outline, the stencil reference
            // will be 0, so if the bit in the stencil buffer is set the test will fail and
            // thus the outline will not draw over the object itself.
            stencil_state.front.compare = wgpu::CompareFunction::Equal;
            stencil_state.read_mask = read_mask.into();
        }

        Self::new(
            wgpu_context,
            label,
            shader_module,
            bind_group_layouts,
            DepthState {
                write_enable: true,
                compare: depth_compare,
                bias: Default::default(),
            },
            stencil_state,
            topology,
            vertex_shader_entry_point,
            fragment_shader_entry_point,
            cull_back_faces,
        )
    }
}

/// # TODO
///
/// Could merge [`MaterialData`] directly into it and arrange fields such the we
/// save some bytes.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct InstanceData {
    transform: Matrix4<f32>,
    flags: InstanceFlags,
    base_vertex: u32,
    _padding: [u32; 2],
    material: MaterialData,
}

impl InstanceData {
    /// Creates instance data for mesh rendering
    pub fn new_mesh(
        transform: &Transform,
        material: Option<&Material>,
        material_textures: Option<&MaterialTextures>,
        flags: InstanceFlags,
        base_vertex: u32,
        outline: Option<&Outline>,
    ) -> Self {
        Self {
            transform: transform.transform.to_homogeneous(),
            flags,
            base_vertex,
            _padding: [0; _],
            material: MaterialData::new(material, material_textures, outline),
        }
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
    #[repr(C)]
    struct InstanceFlags: u32 {
        const REVERSE_WINDING = 0b0000_0001;

        // todo: these are not used currently. since we only render one
        // instance at a time currently, we could just not emit a draw call
        // of one of these is disabled.
        // if we were to render multiple instances at a time, we can use this
        // flag in the vertex shader to skip anything that it shouldn't render.
        const SHOW_SOLID      = 0b0000_0010;
        const SHOW_WIREFRAME  = 0b0000_0100;
        const SHOW_OUTLINE    = 0b0000_1000;

        const ENABLE_TEXTURES = 0b0001_0000;
        const UV_BUFFER_VALID = 0b0010_0000;
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Outline {
    #[serde(with = "crate::util::serde::palette")]
    pub color: Srgba,

    pub thickness: f32,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            color: Srgba::new(1.0, 1.0, 1.0, 0.75),
            thickness: 0.1,
        }
    }
}

impl PropertiesUi for Outline {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(ui, "Color", &mut changes, &mut self.color);
                label_and_value(ui, "Thickness", &mut changes, &mut self.thickness);
            })
            .response;

        changes.propagated(response)
    }
}

#[derive(Clone, Copy, Debug)]
struct DepthState {
    pub write_enable: bool,
    pub compare: wgpu::CompareFunction,
    pub bias: wgpu::DepthBiasState,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct Stencil: u8 {
        const OUTLINE = 0b0000_0001;
    }
}

impl From<Stencil> for u32 {
    fn from(value: Stencil) -> Self {
        value.bits().into()
    }
}

#[derive(Clone, Debug)]
struct Fallbacks {
    pub white: wgpu::TextureView,
    pub black: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub uv_buffer: wgpu::Buffer,
}

impl Fallbacks {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let white = texture_view_from_color(device, queue, Srgba::new(255, 255, 255, 255), "white");
        let black = texture_view_from_color(device, queue, Srgba::new(0, 0, 0, 255), "black");

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("default texture sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uv_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uv buffer dummy"),
            // the shader will expect at least one element in the array, which is a vec2f, so it's 8
            // bytes
            size: 8,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        Self {
            white,
            black,
            sampler,
            uv_buffer,
        }
    }
}
