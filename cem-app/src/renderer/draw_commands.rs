use std::{
    convert::identity,
    ops::{
        Deref,
        DerefMut,
        Range,
    },
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use bevy_ecs::{
    component::Component,
    entity::Entity,
};
use bitflags::bitflags;
use cem_util::{
    ReusableSharedBuffer,
    ReusableSharedBufferGuard,
};
use nalgebra::Point3;

use crate::renderer::{
    Command,
    command::CommandSender,
    mesh::{
        Mesh,
        MeshBindGroup,
    },
    pipeline::Stencil,
    renderer::Renderer,
};

#[derive(Debug, Default)]
pub struct DrawCommandBuffer {
    buffer: ReusableSharedBuffer<DrawCommandBuilderBuffer>,
}

impl DrawCommandBuffer {
    pub fn builder(&mut self) -> DrawCommandBuilder<'_> {
        let mut buffer = self.buffer.write(Default::default);

        // very important lol
        buffer.clear();

        if buffer.reallocated() {
            tracing::warn!("draw command buffer reallocated");
        }

        DrawCommandBuilder { buffer }
    }

    pub fn finish(
        &self,
        renderer: &Renderer,
        camera_bind_group: wgpu::BindGroup,
        camera_position: Point3<f32>,
        flags: DrawCommandFlags,
        draw_command_info_sink: DrawCommandInfoSink,
    ) -> DrawCommand {
        DrawCommand {
            camera_bind_group,
            clear_pipeline: flags
                .contains(DrawCommandFlags::CLEAR)
                .then(|| renderer.clear_pipeline.pipeline.clone()),
            camera_position,
            flags,
            mesh_opaque_pipeline: flags
                .contains(DrawCommandFlags::MESH_OPAQUE)
                .then(|| renderer.mesh_opaque_pipeline.pipeline.clone()),
            mesh_transparent_pipeline: flags
                .contains(DrawCommandFlags::MESH_TRANSPARENT)
                .then(|| renderer.mesh_transparent_pipeline.pipeline.clone()),
            wireframe_pipeline: flags
                .intersects(DrawCommandFlags::WIREFRAME | DrawCommandFlags::DEBUG_WIREFRAME)
                .then(|| renderer.wireframe_pipeline.pipeline.clone()),
            outline_pipeline: flags
                .contains(DrawCommandFlags::OUTLINE)
                .then(|| renderer.outline_pipeline.pipeline.clone()),
            buffer: self.buffer.get(),
            draw_command_info_sink,
        }
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct DrawCommandFlags: u32 {
        const CLEAR            = 0x0000_0001;
        const MESH_OPAQUE      = 0x0000_0002;
        const MESH_TRANSPARENT = 0x0000_0004;
        const WIREFRAME        = 0x0000_0008;
        const OUTLINE          = 0x0000_0010;
        const DEBUG_WIREFRAME  = 0x0000_0020;
    }
}

#[derive(Debug)]
pub struct DrawCommandBuilder<'a> {
    buffer: ReusableSharedBufferGuard<'a, DrawCommandBuilderBuffer>,
}

impl<'a> DrawCommandBuilder<'a> {
    pub fn draw_mesh(
        &mut self,
        instances: Range<u32>,
        mesh: &Mesh,
        mesh_bind_group: &MeshBindGroup,
        transparent: Option<Point3<f32>>,
        outlined: bool,
    ) {
        let mut stencil_reference = Stencil::empty();

        if outlined {
            stencil_reference.insert(Stencil::OUTLINE);
        }

        let draw_mesh = DrawMesh {
            instances,
            indices: mesh.indices.clone(),
            mesh_bind_group: mesh_bind_group.bind_group.clone(),
            stencil_reference,
            depth_reference: transparent.unwrap_or_default(),
        };

        if transparent.is_some() {
            self.buffer.draw_meshes_transparent.push(draw_mesh);
        }
        else {
            self.buffer.draw_meshes_opaque.push(draw_mesh);
        }
    }

    pub fn draw_outline(
        &mut self,
        instances: Range<u32>,
        mesh: &Mesh,
        mesh_bind_group: &MeshBindGroup,
    ) {
        self.buffer.draw_outlines.push(DrawMesh {
            instances,
            indices: mesh.indices.clone(),
            mesh_bind_group: mesh_bind_group.bind_group.clone(),
            stencil_reference: Stencil::OUTLINE,
            depth_reference: Default::default(),
        })
    }

    pub fn draw_wireframe(
        &mut self,
        instances: Range<u32>,
        mesh: &Mesh,
        mesh_bind_group: &MeshBindGroup,
    ) {
        self.buffer.draw_wireframes.push(DrawMesh {
            instances,
            indices: mesh.indices.clone(),
            mesh_bind_group: mesh_bind_group.bind_group.clone(),
            stencil_reference: Stencil::empty(),
            depth_reference: Default::default(),
        })
    }
}

#[derive(Debug, Default)]
struct DrawCommandBuilderBuffer {
    draw_meshes_opaque: Vec<DrawMesh>,
    draw_meshes_transparent: Vec<DrawMesh>,
    draw_outlines: Vec<DrawMesh>,
    draw_wireframes: Vec<DrawMesh>,
}

impl DrawCommandBuilderBuffer {
    fn clear(&mut self) {
        // note: we on purpose deconstruct here, so that we get a compiler error if we
        // forget something.

        let Self {
            draw_meshes_opaque,
            draw_meshes_transparent,
            draw_outlines,
            draw_wireframes,
        } = self;

        draw_meshes_opaque.clear();
        draw_meshes_transparent.clear();
        draw_outlines.clear();
        draw_wireframes.clear();
    }
}

#[derive(Debug)]
struct DrawMesh {
    /// range in the instance buffer to use
    instances: Range<u32>,

    /// range in the index buffer to use (usually `0..num_indices`)
    indices: Range<u32>,

    /// the bind group containing the index and vertex buffer for the mesh.
    mesh_bind_group: wgpu::BindGroup,

    /// the stencil reference to set before the draw call is issued.
    stencil_reference: Stencil,

    // depth reference used for transparent drawing
    depth_reference: Point3<f32>,
}

#[derive(Debug)]
pub struct DrawCommand {
    camera_bind_group: wgpu::BindGroup,
    camera_position: Point3<f32>,
    flags: DrawCommandFlags,

    // pipelines
    clear_pipeline: Option<wgpu::RenderPipeline>,
    mesh_opaque_pipeline: Option<wgpu::RenderPipeline>,
    mesh_transparent_pipeline: Option<wgpu::RenderPipeline>,
    wireframe_pipeline: Option<wgpu::RenderPipeline>,
    outline_pipeline: Option<wgpu::RenderPipeline>,

    buffer: Arc<DrawCommandBuilderBuffer>,

    draw_command_info_sink: DrawCommandInfoSink,
}

impl DrawCommand {
    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        let time_start = Instant::now();

        let mut render_pass = RenderPass::from(render_pass);

        // set camera
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);

        // clear
        if let Some(clear_pipeline) = &self.clear_pipeline {
            render_pass.set_pipeline(clear_pipeline);
            render_pass.draw(0..3, 0..1);
        }

        // solid opaque mesh
        if let Some(solid_pipeline) = &self.mesh_opaque_pipeline
            && !self.buffer.draw_meshes_opaque.is_empty()
        {
            render_pass.draw_meshes_with_pipeline(
                solid_pipeline,
                &self.buffer.draw_meshes_opaque,
                identity,
            );
        }

        // solid transparent mesh
        if let Some(solid_pipeline) = &self.mesh_transparent_pipeline
            && !self.buffer.draw_meshes_transparent.is_empty()
        {
            // sort transparent mesh draw commands by distance to camera (furthest first).
            // for now we'll allocate here :sobbing:
            let mut draw_meshes_transparent_sorted = self
                .buffer
                .draw_meshes_transparent
                .iter()
                .map(|draw_mesh| {
                    let distance_to_camera =
                        (draw_mesh.depth_reference - self.camera_position).norm_squared();
                    (draw_mesh, distance_to_camera)
                })
                .collect::<Vec<_>>();
            draw_meshes_transparent_sorted.sort_unstable_by(|(_, a), (_, b)| {
                b.partial_cmp(a).expect("invalid distance to camera")
            });

            render_pass.draw_meshes_with_pipeline(
                solid_pipeline,
                draw_meshes_transparent_sorted
                    .into_iter()
                    .map(|(draw_mesh, _)| draw_mesh),
                identity,
            );
        }

        // wireframe mesh
        if let Some(wireframe_pipeline) = &self.wireframe_pipeline {
            let map_indices = |Range { start, end }| {
                Range {
                    start: 2 * start,
                    end: 2 * end,
                }
            };

            if self.flags.contains(DrawCommandFlags::DEBUG_WIREFRAME) {
                if !self.buffer.draw_meshes_opaque.is_empty() {
                    render_pass.draw_meshes_with_pipeline(
                        wireframe_pipeline,
                        &self.buffer.draw_meshes_opaque,
                        map_indices,
                    );
                }
                if !self.buffer.draw_meshes_transparent.is_empty() {
                    render_pass.draw_meshes_with_pipeline(
                        wireframe_pipeline,
                        &self.buffer.draw_meshes_transparent,
                        map_indices,
                    );
                }
            }

            if !self.buffer.draw_wireframes.is_empty()
                && self
                    .flags
                    .intersects(DrawCommandFlags::WIREFRAME | DrawCommandFlags::DEBUG_WIREFRAME)
            {
                render_pass.draw_meshes_with_pipeline(
                    wireframe_pipeline,
                    &self.buffer.draw_wireframes,
                    map_indices,
                );
            }
        }

        // selection outline
        if let Some(outline_pipeline) = &self.outline_pipeline
            && !self.buffer.draw_outlines.is_empty()
        {
            render_pass.draw_meshes_with_pipeline(
                outline_pipeline,
                self.buffer.draw_outlines.iter(),
                identity,
            );
        }

        let total = time_start.elapsed();
        let draw_command_info = DrawCommandInfo {
            total,
            num_opaque: self.buffer.draw_meshes_opaque.len(),
            num_transparent: self.buffer.draw_meshes_transparent.len(),
            num_outlines: self.buffer.draw_outlines.len(),
        };
        self.draw_command_info_sink.send(draw_command_info);
    }
}

#[derive(Clone, Copy, Debug, Component)]
pub struct DrawCommandInfo {
    pub total: Duration,
    pub num_opaque: usize,
    pub num_transparent: usize,
    pub num_outlines: usize,
}

#[derive(Clone, Debug)]
pub struct DrawCommandInfoSink {
    pub command_sender: CommandSender,
    pub camera_entity: Entity,
}

impl DrawCommandInfoSink {
    pub fn send(&self, draw_command_info: DrawCommandInfo) {
        self.command_sender.send(Command::DrawCommandInfo {
            camera_entity: self.camera_entity,
            draw_command_info,
        })
    }
}

/// Wrapper around [`wgpu::RenderPass`] for convenience.
#[derive(Debug)]
struct RenderPass<'a> {
    inner: &'a mut wgpu::RenderPass<'static>,

    /// Currently set stencil reference.
    ///
    /// We keep track of this, so we only set this if we actually change the
    /// value.
    stencil_reference: Stencil,
}

impl<'a> RenderPass<'a> {
    pub fn set_stencil_reference(&mut self, stencil_reference: Stencil) {
        if self.stencil_reference != stencil_reference {
            self.inner.set_stencil_reference(stencil_reference.into());
            self.stencil_reference = stencil_reference;
        }
    }

    /// Helper function to render objects with a given pipeline.
    ///
    /// Obviously the pipeline must be compatible. This works
    /// with solid or wireframe rendering
    fn draw_meshes_with_pipeline<'b>(
        &mut self,
        pipeline: &wgpu::RenderPipeline,
        draw_meshes: impl IntoIterator<Item = &'b DrawMesh>,
        map_indices: impl Fn(Range<u32>) -> Range<u32>,
    ) {
        // set draw (solid) pipeline
        self.inner.set_pipeline(pipeline);

        // issue draw commands
        for draw_command in draw_meshes {
            self.inner
                .set_bind_group(1, &draw_command.mesh_bind_group, &[]);

            self.set_stencil_reference(draw_command.stencil_reference);

            let indices = map_indices(draw_command.indices.clone());

            self.inner.draw(indices, draw_command.instances.clone());
        }
    }
}

impl<'a> From<&'a mut wgpu::RenderPass<'static>> for RenderPass<'a> {
    fn from(value: &'a mut wgpu::RenderPass<'static>) -> Self {
        Self {
            inner: value,
            stencil_reference: Default::default(),
        }
    }
}

impl<'a> Deref for RenderPass<'a> {
    type Target = wgpu::RenderPass<'static>;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl<'a> DerefMut for RenderPass<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.inner
    }
}
