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
    assets::AssetExt,
    renderer::{
        material::{
            LoadAlbedoTexture,
            LoadMaterialTexture,
        },
        mesh::LoadMesh,
        renderer::{
            Renderer,
            RendererConfig,
            SharedRenderer,
        },
        state::RendererState,
        systems::{
            self,
            UpdateMeshBindGroupMessage,
        },
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
    UpdateMeshes,
    EmitDrawList,
    End,
}

#[derive(Clone, Debug)]
pub struct RenderPlugin {
    renderer: SharedRenderer,
}

impl Plugin for RenderPlugin {
    fn setup(&self, builder: &mut SceneBuilder) {
        builder
            // todo: bevy-migrate: share the texture cache between worlds
            .insert_resource(TextureCache::default())
            // insert the shared renderer as resource
            .insert_resource(self.renderer.clone())
            .insert_resource(RendererState::new(&self.renderer.wgpu_context.device))
            // register messages
            .register_message::<UpdateMeshBindGroupMessage>()
            // add various rendering systems
            .add_systems(
                schedule::Render,
                (
                    // these update rendering-related components and bind groups. could be done
                    // in post-update?
                    (
                        (
                            systems::update_camera_viewports,
                            systems::destroy_camera_bind_groups,
                        )
                            .in_set(RenderSystems::UpdateCameras),
                        systems::update_mesh_bind_groups.in_set(RenderSystems::UpdateMeshes),
                    )
                        .before(RenderSystems::EmitDrawList),
                    // the actual rendering
                    (
                        systems::begin_frame.in_set(RenderSystems::Begin),
                        (
                            systems::update_instance_buffer_and_draw_command,
                            // note: `create_camera_bind_groups` is run here after
                            // `update_instance_buffer_and_draw_command` because it needs to
                            // recreate the camera bind groups if the
                            // instance buffer was reallocated
                            systems::create_camera_bind_groups,
                        )
                            .chain()
                            .in_set(RenderSystems::EmitDrawList)
                            .after(RenderSystems::Begin)
                            .before(RenderSystems::End),
                        systems::end_frame.in_set(RenderSystems::End),
                    ),
                ),
            )
            .register_asset_loader::<LoadMesh>()
            .register_asset_loader::<LoadAlbedoTexture>()
            .register_asset_loader::<LoadMaterialTexture>();
    }
}
