use std::sync::Arc;

use bevy_ecs::schedule::{
    IntoScheduleConfigs,
    SystemSet,
};
use cem_scene::{
    SceneBuilder,
    plugin::Plugin,
    schedule,
};

use crate::{
    app::WgpuContext,
    renderer::{
        renderer::{
            Renderer,
            RendererConfig,
            SharedRenderer,
        },
        systems,
        texture::cache::TextureCache,
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

#[derive(Clone, Copy, Debug, SystemSet, Hash, PartialEq, Eq)]
pub enum RenderSystems {
    Begin,
    UpdateCameras,
    EmitDrawList,
    End,
}

#[derive(Debug)]
pub struct RenderPlugin {
    renderer: SharedRenderer,
}

impl Plugin for RenderPlugin {
    fn setup(&self, builder: &mut SceneBuilder) {
        // todo: bevy-migrate: share the texture cache between worlds
        builder.insert_resource(TextureCache::default());

        builder
            .insert_resource(self.renderer.clone())
            .add_systems(
                schedule::Render,
                systems::begin_frame.in_set(RenderSystems::Begin),
            )
            .add_systems(
                schedule::Render,
                (
                    systems::update_camera_viewports,
                    systems::destroy_camera_bind_groups,
                )
                    .in_set(RenderSystems::UpdateCameras),
            )
            .add_systems(
                schedule::Render,
                systems::update_instance_buffer_and_draw_command
                    .chain()
                    .in_set(RenderSystems::EmitDrawList)
                    .after(RenderSystems::Begin)
                    .after(RenderSystems::UpdateCameras)
                    .before(RenderSystems::End),
            )
            .add_systems(
                schedule::Render,
                systems::end_frame.in_set(RenderSystems::End),
            );
    }
}
