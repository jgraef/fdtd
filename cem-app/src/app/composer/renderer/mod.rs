pub mod camera;
mod command;
mod draw_commands;
pub mod grid;
pub mod light;
pub mod material;
pub mod mesh;
mod pipeline;
pub mod resource;
pub mod texture_channel;

use std::{
    num::NonZero,
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use bitflags::bitflags;
use bytemuck::{
    Pod,
    Zeroable,
};
use cem_util::{
    format_size,
    wgpu::{
        WriteImageToTextureExt,
        buffer::{
            StagedTypedArrayBuffer,
            StagingBufferProvider,
            WriteStagingTransaction,
        },
        create_texture_from_color,
        create_texture_view_from_texture,
    },
};
use nalgebra::Matrix4;
use palette::{
    LinSrgba,
    Srgb,
    Srgba,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    app::{
        composer::{
            properties::{
                PropertiesUi,
                TrackChanges,
                label_and_value,
                label_and_value_with_config,
                std::NumericPropertyUiConfig,
            },
            renderer::{
                camera::{
                    CameraConfig,
                    CameraResources,
                },
                command::{
                    Command,
                    CommandQueue,
                    CommandReceiver,
                },
                draw_commands::{
                    DrawCommand,
                    DrawCommandBuffer,
                    DrawCommandFlags,
                },
                material::{
                    AlbedoTexture,
                    Material,
                    MaterialData,
                    MaterialTexture,
                    Wireframe,
                },
                mesh::{
                    Mesh,
                    MeshBindGroup,
                    MeshFlags,
                    WindingOrder,
                },
                pipeline::{
                    clear::{
                        ClearPipeline,
                        ClearPipelineDescriptor,
                    },
                    mesh::{
                        MeshPipeline,
                        MeshPipelineDescriptor,
                        StencilStateExt,
                    },
                },
                resource::RenderResourceCreator,
            },
            scene::{
                EntityDebugLabel,
                Label,
                Scene,
                transform::GlobalTransform,
            },
        },
        debug::DebugUi,
        start::{
            CreateAppContext,
            WgpuContext,
        },
    },
    impl_register_component,
    util::egui::EguiUtilUiExt,
};

/// Tag for entities that should be rendered
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Hidden;

impl PropertiesUi for Hidden {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        ui.noop()
    }
}

impl_register_component!(Hidden where ComponentUi, default);

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

impl PropertiesUi for ClearColor {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        self.clear_color.properties_ui(ui, &())
    }
}

impl_register_component!(ClearColor where ComponentUi, default);

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
    egui_wgpu_renderer: EguiWgpuRenderer,
    config: RendererConfig,

    camera_bind_group_layout: wgpu::BindGroupLayout,
    mesh_bind_group_layout: wgpu::BindGroupLayout,

    // this is actually used for everything, not just meshes. but we might split it into clear,
    // mesh, etc.
    mesh_shader_module: wgpu::ShaderModule,

    clear_pipeline: ClearPipeline,
    mesh_opaque_pipeline: MeshPipeline,
    mesh_transparent_pipeline: MeshPipeline,
    wireframe_pipeline: MeshPipeline,
    outline_pipeline: MeshPipeline,

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
    fallbacks: Fallbacks,

    /// Command queue to asynchronously send commands to the renderer.
    ///
    /// The queue is checked in [`prepare_world`](Self::prepare_world). Senders
    /// are e.g. handed out to
    /// [`TextureSender`s](texture_channel::TextureSender).
    command_queue: CommandQueue,

    info: RendererInfo,
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
        wgpu::include_wgsl!("mesh.wgsl");

    // We need to flip the interpretation of the winding order here, because this
    // actually depends on the orientation of our Z axis.
    pub const FRONT_FACE: wgpu::FrontFace = Renderer::WINDING_ORDER.flipped().front_face();

    pub fn from_app_context(context: &CreateAppContext) -> Self {
        let camera_bind_group_layout = context.wgpu_context.device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
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
            },
        );

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

            context
                .wgpu_context
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("mesh_bind_group_layout"),
                    entries: &[
                        // index buffer
                        vertex_buffer(0),
                        // vertex buffer
                        vertex_buffer(1),
                        // sampler
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        // material - albedo
                        texture(3),
                        // material - material
                        texture(4),
                    ],
                })
        };

        let mesh_shader_module = context
            .wgpu_context
            .device
            .create_shader_module(Self::MESH_SHADER_MODULE);

        let clear_pipeline = ClearPipeline::new(
            &context.wgpu_context.device,
            &ClearPipelineDescriptor {
                renderer_config: &context.renderer_config,
                camera_bind_group_layout: &camera_bind_group_layout,
                shader_module: &mesh_shader_module,
            },
        );

        let mesh_opaque_pipeline = MeshPipeline::new(
            &context.wgpu_context.device,
            &MeshPipelineDescriptor {
                label: "render/mesh/opaque",
                renderer_config: &context.renderer_config,
                camera_bind_group_layout: &camera_bind_group_layout,
                mesh_bind_group_layout: &mesh_bind_group_layout,
                shader_module: &mesh_shader_module,
                depth_state: DepthState::new(true, wgpu::CompareFunction::Less),
                stencil_state: wgpu::StencilState::new(Some(Stencil::OUTLINE), None),
                topology: wgpu::PrimitiveTopology::TriangleList,
                vertex_shader_entry_point: "vs_main_solid",
                fragment_shader_entry_point: "fs_main_solid",
                alpha_blending: false,
            },
        );

        let mesh_transparent_pipeline = MeshPipeline::new(
            &context.wgpu_context.device,
            &MeshPipelineDescriptor {
                label: "render/mesh/opaque",
                renderer_config: &context.renderer_config,
                camera_bind_group_layout: &camera_bind_group_layout,
                mesh_bind_group_layout: &mesh_bind_group_layout,
                shader_module: &mesh_shader_module,
                depth_state: DepthState::new(false, wgpu::CompareFunction::Less),
                stencil_state: wgpu::StencilState::new(Some(Stencil::OUTLINE), None),
                topology: wgpu::PrimitiveTopology::TriangleList,
                vertex_shader_entry_point: "vs_main_solid",
                fragment_shader_entry_point: "fs_main_solid",
                alpha_blending: true,
            },
        );

        let wireframe_pipeline = MeshPipeline::new(
            &context.wgpu_context.device,
            &MeshPipelineDescriptor {
                label: "render/mesh/wireframe",
                renderer_config: &context.renderer_config,
                camera_bind_group_layout: &camera_bind_group_layout,
                mesh_bind_group_layout: &mesh_bind_group_layout,
                shader_module: &mesh_shader_module,
                depth_state: DepthState::new(true, wgpu::CompareFunction::LessEqual),
                stencil_state: Default::default(),
                topology: wgpu::PrimitiveTopology::LineList,
                vertex_shader_entry_point: "vs_main_wireframe",
                fragment_shader_entry_point: "fs_main_single_color",
                alpha_blending: false,
            },
        );

        // the outline pipeline will draw a scaled version of the mesh with a solid
        // color. it will ignore depth tests, but will check if the OUTLINE bit
        // in the stencil mask is not set
        let outline_pipeline = MeshPipeline::new(
            &context.wgpu_context.device,
            &MeshPipelineDescriptor {
                label: "render/mesh/outline",
                renderer_config: &context.renderer_config,
                camera_bind_group_layout: &camera_bind_group_layout,
                mesh_bind_group_layout: &mesh_bind_group_layout,
                shader_module: &mesh_shader_module,
                depth_state: DepthState::new(false, wgpu::CompareFunction::Always),
                stencil_state: wgpu::StencilState::new(
                    None,
                    Some(StencilTest {
                        read_mask: Stencil::OUTLINE,
                        compare: wgpu::CompareFunction::NotEqual,
                    }),
                ),
                topology: wgpu::PrimitiveTopology::TriangleList,
                vertex_shader_entry_point: "vs_main_outline",
                fragment_shader_entry_point: "fs_main_single_color",
                alpha_blending: true,
            },
        );

        let instance_buffer = StagedTypedArrayBuffer::with_capacity(
            context.wgpu_context.device.clone(),
            "instance buffer",
            wgpu::BufferUsages::STORAGE,
            128,
        );
        assert!(instance_buffer.buffer.is_allocated());

        let fallbacks = Fallbacks::new(&context.wgpu_context.device, &context.wgpu_context.queue);

        Self {
            wgpu_context: context.wgpu_context.clone(),
            egui_wgpu_renderer: context.egui_wgpu_renderer.clone(),
            config: context.renderer_config,
            camera_bind_group_layout,
            mesh_bind_group_layout,
            mesh_shader_module,
            clear_pipeline,
            mesh_opaque_pipeline,
            mesh_transparent_pipeline,
            wireframe_pipeline,
            outline_pipeline,
            instance_buffer,
            draw_command_buffer: Default::default(),
            fallbacks,
            command_queue: CommandQueue::new(512),
            info: Default::default(),
        }
    }

    pub fn wgpu_context(&self) -> &WgpuContext {
        &self.wgpu_context
    }

    pub fn config(&self) -> &RendererConfig {
        &self.config
    }

    pub fn resource_creator(&self) -> RenderResourceCreator {
        RenderResourceCreator::from_renderer(self)
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
    pub fn prepare_world(&mut self, scene: &mut Scene) {
        let time_start = Instant::now();

        let mut command_encoder =
            self.wgpu_context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("render/prepare_world"),
                });

        let mut write_staging = WriteStagingTransaction::new(
            self.wgpu_context.staging_pool.start_write(),
            &self.wgpu_context.device,
            &mut command_encoder,
        );

        // handle command queue
        handle_commands(&mut self.command_queue.receiver, &mut write_staging, scene);

        // generate meshes (for rendering) for objects that don't have them yet.
        mesh::update_mesh_bind_groups(
            scene,
            &self.wgpu_context.device,
            &self.mesh_bind_group_layout,
            &self.fallbacks,
        );

        // next we prepare the draw commands.
        // this is done once in prepare, so multiple view widgets can then use the draw
        // commands to render their scenes from different cameras.
        // the draw commands are stored in an `Arc<Vec<_>>`, so they can be easily sent
        // via paint callbacks.
        let instance_buffer_reallocated = update_instance_buffer_and_draw_command(
            &mut scene.entities,
            &mut scene.command_buffer,
            &mut self.instance_buffer,
            &mut self.draw_command_buffer,
            &mut write_staging,
        );

        // update cameras
        //
        // note: this is done after the instance buffer has been filled, because we'll
        // know if it was reallocated. the instance buffer is in the camera bind group,
        // and that will need to be recreated in this case.
        camera::update_cameras(
            scene,
            &self.wgpu_context.device,
            &mut write_staging,
            &self.camera_bind_group_layout,
            self.instance_buffer.buffer.buffer().unwrap(),
            instance_buffer_reallocated,
        );

        // finish all staged writes
        self.info.prepare_world_staged_bytes = write_staging.total_staged();
        drop(write_staging);
        self.wgpu_context.queue.submit([command_encoder.finish()]);

        // apply deferred scene commands
        scene.apply_deferred();

        self.info.prepare_world_time = time_start.elapsed();
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
        // fixme: use a fallback camera buffer to call the clear screen
        // only.
        //
        // if we don't have a camera we can't do anything. since we can't
        // bind a camera bind group, we can't even clear. we
        // could have a fallback camera bind group that is only
        // used for clearing. or separate the clear color from
        // the camera.
        let camera_entity = camera_entity?;

        // get bind group and config for our camera
        let mut query = camera_entity.query::<(
            &CameraResources,
            Option<&CameraConfig>,
            hecs::Satisfies<&ClearColor>,
            &GlobalTransform,
        )>();

        let (camera_resources, camera_config, has_clear_color, camera_transform) = query.get()?;

        // default to all, then apply configuration, so by default stuff will render and
        // we don't have to debug for 15 minutes to find that we don't enable the
        // pipeline
        let mut draw_command_flags = DrawCommandFlags::all();
        draw_command_flags.set(DrawCommandFlags::CLEAR, has_clear_color);
        if let Some(camera_config) = camera_config {
            camera_config.apply_to_draw_command_flags(&mut draw_command_flags);
        }

        Some(self.draw_command_buffer.finish(
            self,
            camera_resources.bind_group.clone(),
            camera_transform.position(),
            draw_command_flags,
            camera_entity.entity(),
        ))
    }

    pub fn info(&self) -> RendererInfo {
        self.info
    }
}

fn handle_commands<P>(
    command_receiver: &mut CommandReceiver,
    write_staging: &mut WriteStagingTransaction<P>,
    scene: &mut Scene,
) where
    P: StagingBufferProvider,
{
    // todo: don't take the queue, but pass the WriteStaging
    //
    // note: for now we handle everything on the same thread, having &mut access to
    // the whole renderer. but many commands we would better handle in a separate
    // thread (e.g. ones that only require access to device/queue).

    for command in command_receiver.drain() {
        match command {
            Command::CopyImageToTexture(command) => {
                command.handle(|image, texture| {
                    image.write_to_texture(texture, write_staging);
                });
            }
            Command::DrawCommandInfo(info) => {
                scene
                    .command_buffer
                    .insert_one(info.camera_entity, info.info);
            }
        }
    }
}

fn update_instance_buffer_and_draw_command<P>(
    world: &mut hecs::World,
    command_buffer: &mut hecs::CommandBuffer,
    instance_buffer: &mut StagedTypedArrayBuffer<InstanceData>,
    draw_command_buffer: &mut DrawCommandBuffer,
    write_staging: &mut WriteStagingTransaction<P>,
) -> bool
where
    P: StagingBufferProvider,
{
    // for now every draw call will only draw one instance, but we could do
    // instancing for real later.
    let mut first_instance = 0;
    let mut next_instances = || {
        let instances = first_instance..(first_instance + 1);
        first_instance += 1;
        instances
    };

    // this is the second time I have a bug with this. keep this assert!
    assert!(
        instance_buffer.host_staging.is_empty(),
        "instance scratch buffer hasn't been cleared yet"
    );

    // prepare the actual draw commands
    let mut draw_command_builder = draw_command_buffer.builder();

    type AnyRenderable<'a> = hecs::Or<
        hecs::Or<&'a Material, &'a Wireframe>,
        hecs::Or<&'a AlbedoTexture, &'a MaterialTexture>,
    >;

    // draw meshes (opaque, transparent, wireframe, outlines)
    for (
        entity,
        (
            label,
            transform,
            mesh,
            mesh_bind_group,
            material,
            wireframe,
            albedo_texture,
            material_texture,
            outline,
        ),
    ) in world
        .query_mut::<(
            Option<&Label>,
            &GlobalTransform,
            &Mesh,
            &MeshBindGroup,
            Option<&Material>,
            Option<&Wireframe>,
            Option<&AlbedoTexture>,
            Option<&MaterialTexture>,
            Option<&Outline>,
        )>()
        .without::<&Hidden>()
        .with::<AnyRenderable>()
    {
        let has_material =
            material.is_some() || albedo_texture.is_some() || material_texture.is_some();
        let has_wireframe = wireframe.is_some();

        if !has_material && !has_wireframe {
            // note: this should not be triggered with the AnyRenderable filter
            let label = EntityDebugLabel {
                entity,
                label: label.cloned(),
                invalid: false,
            };
            tracing::warn!(entity = %label, "Entity with mesh, but without any materials or wiremesh. Attaching `Hidden` to it to prevent further render attempts");
            command_buffer.insert_one(entity, Hidden);
            continue;
        }

        // write per-instance data into a buffer
        instance_buffer.push(InstanceData::new_mesh(
            transform,
            mesh,
            material,
            wireframe,
            albedo_texture,
            material_texture,
            outline,
        ));

        let instances = next_instances();

        if has_material {
            // if it is transparent we need to remember its position to later sort by
            // distance from camera.
            let transparent = material
                .is_some_and(|material| material.transparent)
                .then(|| transform.position());

            draw_command_builder.draw_mesh(
                instances.clone(),
                mesh,
                mesh_bind_group,
                transparent,
                outline.is_some(),
            );
        }
        if outline.is_some() {
            draw_command_builder.draw_outline(instances.clone(), mesh, mesh_bind_group);
        }
        if has_wireframe {
            draw_command_builder.draw_wireframe(instances.clone(), mesh, mesh_bind_group);
        }
    }

    // send instance data to gpu
    instance_buffer.flush(|_buffer| {}, write_staging)
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct InstanceData {
    transform: Matrix4<f32>,
    instance_flags: InstanceFlags,
    mesh_flags: MeshFlags,
    base_vertex: u32,
    outline_thickness: f32,
    outline_color: LinSrgba,
    material: MaterialData,
}

impl InstanceData {
    /// Creates instance data for mesh rendering
    pub fn new_mesh(
        transform: &GlobalTransform,
        mesh: &Mesh,
        material: Option<&Material>,
        wireframe: Option<&Wireframe>,
        albedo_texture: Option<&AlbedoTexture>,
        material_texture: Option<&MaterialTexture>,
        outline: Option<&Outline>,
    ) -> Self {
        if mesh.winding_order != Renderer::WINDING_ORDER {
            todo!("fix winding order");
        }

        if !mesh.flags.contains(MeshFlags::UVS) {
            // could enable textures in this case, but we need to tell the
            // vertex shader to not index into the uv buffer anyway
            // flags.remove(InstanceFlags::ENABLE_TEXTURES);
        }

        let (outline_thickness, outline_color) = outline.map_or_else(Default::default, |outline| {
            (outline.thickness, outline.color.into_linear())
        });

        Self {
            transform: transform.isometry().to_homogeneous(),
            instance_flags: InstanceFlags::empty(),
            mesh_flags: mesh.flags,
            base_vertex: mesh.base_vertex,
            outline_thickness,
            outline_color,
            material: MaterialData::new(material, wireframe, albedo_texture, material_texture),
        }
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
    #[repr(C)]
    struct InstanceFlags: u32 {
        // unused currently, but surely will be useful in the future again.
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
                label_and_value_with_config(
                    ui,
                    "Thickness",
                    &mut changes,
                    &mut self.thickness,
                    &NumericPropertyUiConfig::Slider { range: 0.0..=10.0 },
                );
            })
            .response;

        changes.propagated(response)
    }
}

impl_register_component!(Outline where ComponentUi, default);

#[derive(Clone, Copy, Debug)]
struct DepthState {
    pub write_enable: bool,
    pub compare: wgpu::CompareFunction,
    pub bias: wgpu::DepthBiasState,
}

impl DepthState {
    pub fn new(write_enable: bool, compare: wgpu::CompareFunction) -> Self {
        Self {
            write_enable,
            compare,
            bias: Default::default(),
        }
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct Stencil: u8 {
        const OUTLINE = 0b0000_0001;
        const ALL     = 0b1111_1111;
    }
}

impl From<Stencil> for u32 {
    fn from(value: Stencil) -> Self {
        value.bits().into()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StencilTest {
    pub read_mask: Stencil,
    pub compare: wgpu::CompareFunction,
}

#[derive(Clone, Debug)]
struct Fallbacks {
    pub white: wgpu::TextureView,
    pub black: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub vertex_buffer: wgpu::Buffer,
}

impl Fallbacks {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let white = create_texture_from_color(
            device,
            queue,
            &Srgba::new(255, 255, 255, 255),
            wgpu::TextureUsages::TEXTURE_BINDING,
            "white",
        );
        let white = create_texture_view_from_texture(&white, "white");

        let black = create_texture_from_color(
            device,
            queue,
            &Srgba::new(0, 0, 0, 255),
            wgpu::TextureUsages::TEXTURE_BINDING,
            "black",
        );
        let black = create_texture_view_from_texture(&black, "black");

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("default texture sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uv buffer dummy"),
            // the shader will expect at least one element in the array:
            // - uv: vec2f, 8 bytes
            // - normals: vec3f, 16 bytes (they're padded)
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        Self {
            white,
            black,
            sampler,
            vertex_buffer,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RendererInfo {
    pub prepare_world_staged_bytes: u64,
    pub prepare_world_time: Duration,
}

impl DebugUi for Renderer {
    fn show_debug(&self, ui: &mut egui::Ui) {
        ui.label(format!(
            "Bytes last frame: {}",
            format_size(self.info.prepare_world_staged_bytes),
        ));
        ui.label(format!(
            "Prepare world time: {:?}",
            self.info.prepare_world_time
        ));

        ui.label(format!(
            "Surface texture: {:?}",
            self.config.target_texture_format
        ));
        ui.label(format!(
            "Depth texture: {:?}",
            self.config.depth_texture_format
        ));
        ui.label(format!(
            "Multisampling: {:?}",
            self.config.multisample_count
        ));
    }
}
