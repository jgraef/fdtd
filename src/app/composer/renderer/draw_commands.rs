use std::{
    ops::Range,
    sync::Arc,
};

use crate::{
    app::composer::renderer::{
        Renderer,
        mesh::Mesh,
    },
    util::{
        ReusableSharedBuffer,
        ReusableSharedBufferGuard,
    },
};

#[derive(Debug, Default)]
pub struct DrawCommandBuffer {
    draw_meshes: ReusableSharedBuffer<Vec<DrawMesh>>,
}

impl DrawCommandBuffer {
    pub fn builder(&mut self) -> DrawCommandBuilder<'_> {
        let mut draw_meshes = self.draw_meshes.write(Default::default);

        // very important lol
        draw_meshes.clear();

        if draw_meshes.reallocated() {
            tracing::warn!("draw command buffer reallocated");
        }

        DrawCommandBuilder { draw_meshes }
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
                .enable_clear
                .then(|| renderer.clear_pipeline.pipeline.clone()),
            solid_pipeline: options
                .enable_solid
                .then(|| renderer.solid_pipeline.pipeline.clone()),
            wireframe_pipeline: options
                .enable_wireframe
                .then(|| renderer.wireframe_pipeline.pipeline.clone()),
            mesh_instance_bind_group: renderer.instance_bind_group.clone(),
            draw_meshes: self.draw_meshes.get(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DrawCommandOptions {
    pub enable_clear: bool,
    pub enable_solid: bool,
    pub enable_wireframe: bool,
}

impl Default for DrawCommandOptions {
    fn default() -> Self {
        Self {
            enable_clear: true,
            enable_solid: true,
            enable_wireframe: true,
        }
    }
}

#[derive(Debug)]
pub struct DrawCommandBuilder<'a> {
    draw_meshes: ReusableSharedBufferGuard<'a, Vec<DrawMesh>>,
}

impl<'a> DrawCommandBuilder<'a> {
    pub fn draw_mesh(&mut self, instances: Range<u32>, mesh: &Mesh) {
        self.draw_meshes.push(DrawMesh {
            instances,
            indices: mesh.indices.clone(),
            bind_group: mesh.bind_group.clone(),
        });
    }
}

#[derive(Debug)]
pub struct DrawCommand {
    camera_bind_group: wgpu::BindGroup,
    clear_pipeline: Option<wgpu::RenderPipeline>,

    // draw meshes
    solid_pipeline: Option<wgpu::RenderPipeline>,
    wireframe_pipeline: Option<wgpu::RenderPipeline>,
    mesh_instance_bind_group: wgpu::BindGroup,
    draw_meshes: Arc<Vec<DrawMesh>>,
}

impl DrawCommand {
    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        // set camera
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);

        // clear
        if let Some(clear_pipeline) = &self.clear_pipeline {
            render_pass.set_pipeline(clear_pipeline);
            render_pass.draw(0..3, 0..1);
        }

        // set instance buffer (this is shared between all draw calls)
        render_pass.set_bind_group(1, &self.mesh_instance_bind_group, &[]);

        // render all objects with the solid and/or wireframe pipeline
        if let Some(solid_pipeline) = &self.solid_pipeline {
            render_meshes_with_pipeline(solid_pipeline, render_pass, &self.draw_meshes);
        }
        if let Some(wireframe_pipeline) = &self.wireframe_pipeline {
            render_meshes_with_pipeline(wireframe_pipeline, render_pass, &self.draw_meshes);
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

/// Helper function to render objects with a given pipeline.
///
/// Obviously the pipeline must be compatible. This works
/// with solid or wireframe rendering
fn render_meshes_with_pipeline(
    pipeline: &wgpu::RenderPipeline,
    render_pass: &mut wgpu::RenderPass<'static>,
    draw_commands: &[DrawMesh],
) {
    // set draw (solid) pipeline
    render_pass.set_pipeline(pipeline);

    // issue draw commands
    for draw_command in draw_commands {
        render_pass.set_bind_group(2, &draw_command.bind_group, &[]);
        render_pass.draw(draw_command.indices.clone(), draw_command.instances.clone());
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
}
