use std::sync::Arc;

use cem_scene::{
    SceneBuilder,
    plugin::Plugin,
};

use crate::{
    app::WgpuContext,
    renderer::renderer::{
        Renderer,
        RendererConfig,
        SharedRenderer,
    },
};

#[derive(Clone, Debug)]
pub struct RenderPluginBuilder {
    wgpu_context: WgpuContext,
    config: RendererConfig,
}

impl RenderPluginBuilder {
    pub fn new(wgpu_context: WgpuContext, renderer_config: RendererConfig) -> Self {
        Self {
            wgpu_context,
            config: renderer_config,
        }
    }

    pub fn build_plugin(&self) -> RenderPlugin {
        let renderer = Renderer::new(self.wgpu_context.clone(), self.config);
        RenderPlugin {
            renderer: SharedRenderer(Arc::new(renderer)),
        }
    }
}

#[derive(Debug)]
pub struct RenderPlugin {
    renderer: SharedRenderer,
}

impl Plugin for RenderPlugin {
    fn setup(&self, builder: &mut SceneBuilder) {
        builder.insert_resource(self.renderer.clone());
        todo!()
    }
}
