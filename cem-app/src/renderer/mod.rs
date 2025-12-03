//! # Notes
//!
//! We use a left-handed coordinate system both in the scene and in NDC:
//!  - x: from left to right
//!  - y: from bottom to top
//!  - z: from outside to inside of screen

pub mod camera;
mod command;
pub mod components;
mod draw_commands;
pub mod grid;
pub mod light;
pub mod material;
pub mod mesh;
mod pipeline;
pub mod plugin;
mod renderer;
pub mod resource;
mod state;
mod systems;
pub mod texture;

use std::time::Duration;

use bevy_ecs::entity::Entity;
use cem_scene::Scene;
use cem_util::{
    format_size,
    wgpu::{
        ImageTextureExt,
        buffer::WriteStaging,
    },
};
pub use renderer::RendererConfig;

use crate::{
    debug::DebugUi,
    renderer::{
        command::{
            Command,
            CommandReceiver,
        },
        draw_commands::DrawCommand,
        material::MaterialData,
    },
};

fn handle_commands<S>(
    command_receiver: &mut CommandReceiver,
    mut write_staging: S,
    _scene: &mut Scene,
) where
    S: WriteStaging,
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
                    image.write_to_texture(texture, &mut write_staging);
                });
            }
            Command::DrawCommandInfo(_info) => {
                // todo: bevy-migrate
                //scene
                //    .command_buffer
                //    .insert_one(info.camera_entity, info.info);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RendererInfo {
    pub prepare_world_staged: StagingInfo,
    pub prepare_world_time: Duration,
}

impl DebugUi for RendererInfo {
    fn show_debug(&self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(format!(
            "Bytes last frame: {}",
            format_size(self.prepare_world_staged.total),
        ))
        .id_salt(ui.id().with("prepare_world_staged"))
        .default_open(true)
        .show(ui, |ui| {
            ui.label(format!(
                "Command Queue: {}",
                format_size(self.prepare_world_staged.command_queue),
            ));
            ui.label(format!(
                "Instance Buffer: {}",
                format_size(self.prepare_world_staged.instance_buffer),
            ));
            ui.label(format!(
                "Camera Buffers: {}",
                format_size(self.prepare_world_staged.camera_buffers),
            ));
        });

        ui.label(format!("Prepare world time: {:?}", self.prepare_world_time));
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StagingInfo {
    pub total: u64,
    pub command_queue: u64,
    pub instance_buffer: u64,
    pub camera_buffers: u64,
}

/// Grabs the draw list for a camera from the scene
pub fn grab_draw_list(scene: &mut Scene, camera_entity: Option<Entity>) -> Option<DrawCommand> {
    scene
        .world
        .run_system_cached_with(systems::grab_draw_list, camera_entity)
        .unwrap()
}
