use std::{
    ops::{
        Deref,
        DerefMut,
        Range,
    },
    sync::Arc,
};

use bitflags::bitflags;

use crate::{
    app::composer::renderer::{
        Renderer,
        Stencil,
        mesh::Mesh,
        texture::Texture,
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
        options: DrawCommandOptions,
    ) -> DrawCommand {
        DrawCommand {
            camera_bind_group,
            clear_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::CLEAR)
                .then(|| renderer.clear_pipeline.pipeline.clone()),
            solid_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::SOLID)
                .then(|| renderer.solid_pipeline.pipeline.clone()),
            wireframe_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::WIREFRAME)
                .then(|| renderer.wireframe_pipeline.pipeline.clone()),
            outline_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::OUTLINE)
                .then(|| renderer.outline_pipeline.pipeline.clone()),
            quad_with_texture_pipeline: options
                .pipeline_enable_flags
                .contains(DrawCommandEnablePipelineFlags::QUADS_WITH_TEXTURE)
                .then(|| renderer.quad_with_texture_pipeline.pipeline.clone()),
            instance_bind_group: renderer.instance_bind_group.clone(),
            buffer: self.buffer.get(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DrawCommandOptions {
    pub pipeline_enable_flags: DrawCommandEnablePipelineFlags,
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct DrawCommandEnablePipelineFlags: u8 {
        const CLEAR = 0b0000_0001;
        const SOLID = 0b0000_0010;
        const WIREFRAME = 0b0000_0100;
        const OUTLINE = 0b0000_1000;
        const QUADS_WITH_TEXTURE = 0b0001_0000;
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
    pub fn draw_mesh(&mut self, instances: Range<u32>, mesh: &Mesh, outline: bool) {
        let mut stencil_reference = Stencil::default();

        if outline {
            stencil_reference.insert(Stencil::OUTLINE);
            let draw_mesh_index = self.buffer.draw_meshes.len();
            self.buffer
                .draw_outlines
                .push(DrawOutline { draw_mesh_index });
        }

        self.buffer.draw_meshes.push(DrawMesh {
            instances,
            indices: mesh.indices.clone(),
            bind_group: mesh.bind_group.clone(),
            stencil_reference,
        });
    }

    pub fn draw_quad_with_texture(&mut self, instances: Range<u32>, texture: &Texture) {
        self.buffer
            .draw_quads_with_texture
            .push(DrawQuadWithTexture {
                instances,
                texture: texture.bind_group.clone(),
            })
    }
}

#[derive(Debug, Default)]
struct DrawCommandBuilderBuffer {
    draw_meshes: Vec<DrawMesh>,
    draw_outlines: Vec<DrawOutline>,
    draw_quads_with_texture: Vec<DrawQuadWithTexture>,
}

impl DrawCommandBuilderBuffer {
    fn clear(&mut self) {
        self.draw_meshes.clear();
        self.draw_outlines.clear();
    }
}

#[derive(Debug)]
struct DrawMesh {
    /// range in the instance buffer to use
    instances: Range<u32>,

    /// range in the index buffer to use (usually `0..num_indices`)
    indices: Range<u32>,

    /// the bind group containing the index and vertex buffer for the mesh.
    bind_group: wgpu::BindGroup,

    /// the stencil reference to set before the draw call is issued.
    stencil_reference: Stencil,
}

#[derive(Debug)]
struct DrawOutline {
    // just point to the DrawMesh command
    draw_mesh_index: usize,
}

#[derive(Debug)]
struct DrawQuadWithTexture {
    /// range in the instance buffer to use
    instances: Range<u32>,

    texture: wgpu::BindGroup,
}

#[derive(Debug)]
pub struct DrawCommand {
    camera_bind_group: wgpu::BindGroup,
    clear_pipeline: Option<wgpu::RenderPipeline>,

    // draw meshes
    solid_pipeline: Option<wgpu::RenderPipeline>,
    wireframe_pipeline: Option<wgpu::RenderPipeline>,
    outline_pipeline: Option<wgpu::RenderPipeline>,
    quad_with_texture_pipeline: Option<wgpu::RenderPipeline>,
    instance_bind_group: Option<wgpu::BindGroup>,

    buffer: Arc<DrawCommandBuilderBuffer>,
}

impl DrawCommand {
    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        let mut render_pass = RenderPass::from(render_pass);

        // set camera
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);

        // clear
        if let Some(clear_pipeline) = &self.clear_pipeline {
            render_pass.set_pipeline(clear_pipeline);
            render_pass.draw(0..3, 0..1);
        }

        let Some(instance_bind_group) = &self.instance_bind_group
        else {
            // if there is no instance bind group, there's nothing else to render
            return;
        };

        // set instance buffer (this is shared between all draw calls)
        render_pass.set_bind_group(1, instance_bind_group, &[]);

        // solid mesh
        if let Some(solid_pipeline) = &self.solid_pipeline
            && !self.buffer.draw_meshes.is_empty()
        {
            render_pass.draw_meshes_with_pipeline(solid_pipeline, &self.buffer.draw_meshes, true);
        }

        // wireframe mesh
        if let Some(wireframe_pipeline) = &self.wireframe_pipeline
            && !self.buffer.draw_meshes.is_empty()
        {
            render_pass.draw_meshes_with_pipeline(
                wireframe_pipeline,
                &self.buffer.draw_meshes,
                false,
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
                    .map(|draw_outline| &self.buffer.draw_meshes[draw_outline.draw_mesh_index]),
                false,
            );
        }

        // quads with textures
        if let Some(quad_with_texture_pipeline) = &self.quad_with_texture_pipeline
            && !self.buffer.draw_quads_with_texture.is_empty()
        {
            render_pass.draw_quad_with_texture_with_pipeline(
                quad_with_texture_pipeline,
                &self.buffer.draw_quads_with_texture,
            );
        }
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
        set_stencil_reference: bool,
    ) {
        // set draw (solid) pipeline
        self.inner.set_pipeline(pipeline);

        if !set_stencil_reference {
            // make sure it's set to 0 in the pipeline
            self.set_stencil_reference(Default::default());
        }

        // issue draw commands
        for draw_command in draw_meshes {
            self.inner.set_bind_group(2, &draw_command.bind_group, &[]);

            if set_stencil_reference {
                self.set_stencil_reference(draw_command.stencil_reference);
            }

            self.inner
                .draw(draw_command.indices.clone(), draw_command.instances.clone());
        }
    }

    fn draw_quad_with_texture_with_pipeline<'b>(
        &mut self,
        pipeline: &wgpu::RenderPipeline,
        draw_quads_with_texture: impl IntoIterator<Item = &'b DrawQuadWithTexture>,
    ) {
        self.inner.set_pipeline(pipeline);

        for draw_command in draw_quads_with_texture {
            self.inner.set_bind_group(2, &draw_command.texture, &[]);

            self.inner.draw(0..6, draw_command.instances.clone());
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
