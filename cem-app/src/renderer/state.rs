use bevy_ecs::resource::Resource;
use bitflags::bitflags;
use bytemuck::{
    Pod,
    Zeroable,
};
use cem_scene::transform::GlobalTransform;
use cem_util::wgpu::buffer::{
    StagedTypedArrayBuffer,
    WriteStagingBelt,
    WriteStagingTransaction,
};
use nalgebra::Matrix4;
use palette::LinSrgba;

use crate::renderer::{
    MaterialData,
    components::Outline,
    draw_commands::DrawCommandBuffer,
    material::{
        AlbedoTexture,
        Material,
        MaterialTexture,
        Wireframe,
    },
    mesh::{
        Mesh,
        MeshFlags,
    },
    renderer::Renderer,
};

#[derive(Debug, Resource)]
pub(super) struct RendererState {
    /// The instance buffer.
    ///
    /// This holds the handle to the GPU buffer for the instance data, a
    /// host staging buffer for the instance data, and the bind group for the
    /// GPU buffer.
    pub instance_buffer: StagedTypedArrayBuffer<InstanceData>,

    /// This stores all draw commands that are generated during `prepare_world`.
    /// Its `finish` method returns the finalized draw command (aggregate) for a
    /// specific camera.
    pub draw_command_buffer: DrawCommandBuffer,

    pub write_staging:
        Option<WriteStagingTransaction<WriteStagingBelt, wgpu::Device, wgpu::CommandEncoder>>,

    pub instance_buffer_reallocated: bool,
}

impl RendererState {
    pub fn new(device: &wgpu::Device) -> Self {
        let instance_buffer = StagedTypedArrayBuffer::with_capacity(
            device.clone(),
            "render/instance_buffer",
            wgpu::BufferUsages::STORAGE,
            128,
        );
        assert!(instance_buffer.buffer.is_allocated());

        Self {
            instance_buffer,
            draw_command_buffer: Default::default(),
            write_staging: None,
            instance_buffer_reallocated: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub(super) struct InstanceData {
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
