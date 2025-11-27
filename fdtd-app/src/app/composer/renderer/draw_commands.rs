use std::{
    convert::identity,
    ops::{
        Deref,
        DerefMut,
        Index,
        Range,
    },
    sync::Arc,
    time::Instant,
};

use bitflags::bitflags;
use nalgebra::Point3;

use crate::{
    app::composer::renderer::{
        Renderer,
        Stencil,
        camera::CameraRenderInfo,
        command::CommandSender,
        mesh::{
            Mesh,
            MeshBindGroup,
        },
    },
    util::{
        ReusableSharedBuffer,
        ReusableSharedBufferGuard,
    },
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
        options: DrawCommandOptions,
        camera_entity: hecs::Entity,
    ) -> DrawCommand {
        DrawCommand {
            camera_bind_group,
            clear_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::CLEAR)
                .then(|| renderer.clear_pipeline.pipeline.clone()),
            camera_position,
            mesh_opaque_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::MESH_OPAQUE)
                .then(|| renderer.mesh_opaque_pipeline.pipeline.clone()),
            mesh_transparent_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::MESH_TRANSPARENT)
                .then(|| renderer.mesh_transparent_pipeline.pipeline.clone()),
            wireframe_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::WIREFRAME)
                .then(|| renderer.wireframe_pipeline.pipeline.clone()),
            outline_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::OUTLINE)
                .then(|| renderer.outline_pipeline.pipeline.clone()),
            buffer: self.buffer.get(),
            camera_entity,
            command_sender: renderer.command_queue.sender.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DrawCommandOptions {
    pub pipeline_enable_flags: DrawCommandEnablePipelineFlags,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct DrawCommandEnablePipelineFlags: u32 {
        const CLEAR            = 0b0000_0001;
        const MESH             = 0b0000_0010;
        const OPAQUE           = 0b0000_0100;
        const TRANSPARENT      = 0b0000_1000;
        const MESH_OPAQUE      = 0b0000_0110;
        const MESH_TRANSPARENT = 0b0000_1010;
        const WIREFRAME        = 0b0001_0000;
        const OUTLINE          = 0b0010_0000;
    }
}

impl Default for DrawCommandEnablePipelineFlags {
    fn default() -> Self {
        Self::all()
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
        outline: bool,
        transparent: Option<Point3<f32>>,
    ) {
        let mut stencil_reference = Stencil::empty();

        if outline {
            stencil_reference |= Stencil::OUTLINE;
            let draw_mesh_index = self.buffer.draw_mesh_index(transparent.is_some());
            self.buffer
                .draw_outlines
                .push(DrawOutline { draw_mesh_index });
        }

        let draw_mesh = DrawMesh {
            instances,
            indices: mesh.indices.clone(),
            mesh_bind_group: mesh_bind_group.bind_group.clone(),
            stencil_reference,
        };

        if let Some(depth_reference) = transparent {
            self.buffer
                .draw_meshes_transparent
                .push(DrawMeshTransparent {
                    draw_mesh,
                    depth_reference,
                });
        }
        else {
            self.buffer.draw_meshes_opaque.push(draw_mesh);
        }
    }
}

#[derive(Debug, Default)]
struct DrawCommandBuilderBuffer {
    draw_meshes_opaque: Vec<DrawMesh>,
    draw_meshes_transparent: Vec<DrawMeshTransparent>,
    draw_outlines: Vec<DrawOutline>,
}

impl DrawCommandBuilderBuffer {
    fn clear(&mut self) {
        self.draw_meshes_opaque.clear();
        self.draw_meshes_transparent.clear();
        self.draw_outlines.clear();
    }

    fn draw_mesh_index(&self, transparent: bool) -> DrawMeshIndex {
        if transparent {
            DrawMeshIndex::Transparent(self.draw_meshes_transparent.len())
        }
        else {
            DrawMeshIndex::Opaque(self.draw_meshes_opaque.len())
        }
    }
}

impl Index<DrawMeshIndex> for DrawCommandBuilderBuffer {
    type Output = DrawMesh;

    fn index(&self, index: DrawMeshIndex) -> &Self::Output {
        match index {
            DrawMeshIndex::Opaque(index) => &self.draw_meshes_opaque[index],
            DrawMeshIndex::Transparent(index) => &self.draw_meshes_transparent[index].draw_mesh,
        }
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
}

#[derive(Debug)]
struct DrawMeshTransparent {
    draw_mesh: DrawMesh,
    depth_reference: Point3<f32>,
}

#[derive(Clone, Copy, Debug)]
enum DrawMeshIndex {
    Opaque(usize),
    Transparent(usize),
}

#[derive(Clone, Copy, Debug)]
struct DrawOutline {
    // just point to the DrawMesh command
    draw_mesh_index: DrawMeshIndex,
}

#[derive(Debug)]
pub struct DrawCommand {
    camera_bind_group: wgpu::BindGroup,
    camera_position: Point3<f32>,

    clear_pipeline: Option<wgpu::RenderPipeline>,

    // draw meshes
    mesh_opaque_pipeline: Option<wgpu::RenderPipeline>,
    mesh_transparent_pipeline: Option<wgpu::RenderPipeline>,
    wireframe_pipeline: Option<wgpu::RenderPipeline>,
    outline_pipeline: Option<wgpu::RenderPipeline>,

    buffer: Arc<DrawCommandBuilderBuffer>,

    // for recording timings
    camera_entity: hecs::Entity,
    command_sender: CommandSender,
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
                    (&draw_mesh.draw_mesh, distance_to_camera)
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
        if let Some(wireframe_pipeline) = &self.wireframe_pipeline
            && !self.buffer.draw_meshes_opaque.is_empty()
        {
            render_pass.draw_meshes_with_pipeline(
                wireframe_pipeline,
                &self.buffer.draw_meshes_opaque,
                |Range { start, end }| {
                    Range {
                        start: 2 * start,
                        end: 2 * end,
                    }
                },
            );
        }

        // selection outline
        if let Some(outline_pipeline) = &self.outline_pipeline
            && !self.buffer.draw_outlines.is_empty()
        {
            render_pass.draw_meshes_with_pipeline(
                outline_pipeline,
                self.buffer
                    .draw_outlines
                    .iter()
                    .map(|draw_outline| &self.buffer[draw_outline.draw_mesh_index]),
                identity,
            );
        }

        let time = time_start.elapsed();
        self.command_sender.send(DrawCommandInfo {
            camera_entity: self.camera_entity,
            info: CameraRenderInfo {
                total: time,
                num_opaque: self.buffer.draw_meshes_opaque.len(),
                num_transparent: self.buffer.draw_meshes_transparent.len(),
                num_outlines: self.buffer.draw_outlines.len(),
            },
        })
    }
}

impl egui_wgpu::CallbackTrait for DrawCommand {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        self.render(render_pass);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DrawCommandInfo {
    pub camera_entity: hecs::Entity,
    pub info: CameraRenderInfo,
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

#[cfg(test)]
mod tests {
    use crate::app::composer::renderer::draw_commands::DrawCommandEnablePipelineFlags;

    #[test]
    fn test_bitflags() {
        assert_eq!(
            DrawCommandEnablePipelineFlags::MESH_OPAQUE,
            DrawCommandEnablePipelineFlags::MESH | DrawCommandEnablePipelineFlags::OPAQUE
        );
        assert_eq!(
            DrawCommandEnablePipelineFlags::MESH_TRANSPARENT,
            DrawCommandEnablePipelineFlags::MESH | DrawCommandEnablePipelineFlags::TRANSPARENT
        );
    }
}
