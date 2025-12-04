use std::sync::Arc;

use bevy_ecs::schedule::{
    IntoScheduleConfigs,
    SystemSet,
};
use cem_scene::{
    SceneBuilder,
    assets::AssetExt,
    plugin::Plugin,
    schedule,
};

use crate::{
    app::WgpuContext,
    renderer::{
        command,
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
        resource::RenderResourceTransactionState,
        state::RendererState,
        systems::{
            self,
            UpdateMeshBindGroupMessage,
            handle_command_queue,
        },
        texture::cache::TextureCache,
    },
};

#[derive(Clone, Copy, Debug, SystemSet, Hash, PartialEq, Eq)]
pub enum RenderSystems {
    Begin,
    UpdateCameras,
    UpdateMeshes,
    EmitDrawList,
    End,
    HandleCommandQueue,
}

#[derive(Clone, Debug)]
pub struct RenderPlugin {
    renderer: SharedRenderer,
}

impl RenderPlugin {
    pub fn new(wgpu_context: WgpuContext, config: RendererConfig) -> Self {
        let renderer = Renderer::new(wgpu_context, config);
        Self {
            renderer: SharedRenderer(Arc::new(renderer)),
        }
    }
}

impl Plugin for RenderPlugin {
    fn setup(&self, builder: &mut SceneBuilder) {
        // we need to make sure this is only reached if there's a bug (e.g. not reading
        // the queue)
        let (command_sender, command_receiver) = command::queue(1024);

        builder
            // todo: bevy-migrate: share the texture cache between worlds
            .insert_resource(TextureCache::default())
            // insert the shared renderer as resource
            .insert_resource(self.renderer.clone())
            .insert_resource(RendererState::new(&self.renderer.wgpu_context.device))
            .insert_resource(RenderResourceTransactionState::default())
            .insert_resource(command_sender)
            .insert_resource(command_receiver)
            // register messages
            .register_message::<UpdateMeshBindGroupMessage>()
            // add various rendering systems
            .add_systems(
                schedule::PostUpdate,
                handle_command_queue.in_set(RenderSystems::HandleCommandQueue),
            )
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
                        (systems::begin_frame, systems::commit_resource_transaction)
                            .in_set(RenderSystems::Begin),
                        (
                            systems::update_instance_buffer_and_draw_command,
                            // note: `create_camera_bind_groups` is run here after
                            // `update_instance_buffer_and_draw_command` because it needs to
                            // recreate the camera bind groups if the
                            // instance buffer was reallocated
                            systems::create_camera_bind_groups,
                            systems::update_camera_bind_groups,
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
