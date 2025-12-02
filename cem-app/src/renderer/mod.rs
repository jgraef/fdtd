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

use bevy_ecs::{
    resource::Resource,
    system::{
        Res,
        SystemParam,
    },
};
use cem_util::format_size;
pub use draw_commands::{
    DrawCommand,
    DrawCommandInfo,
};
pub use renderer::RendererConfig;
pub use systems::grab_draw_list_for_camera;

use crate::{
    debug::DebugUi,
    renderer::{
        command::Command,
        material::MaterialData,
    },
};

#[derive(Clone, Copy, Debug, Default, Resource)]
pub struct RendererInfo {
    pub prepare_world_staged: StagingInfo,
    pub prepare_world_time: Duration,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StagingInfo {
    pub total: u64,
    pub command_queue: u64,
    pub instance_buffer: u64,
    pub camera_buffers: u64,
}

#[derive(Debug, SystemParam)]
pub struct RendererDebugUi<'w> {
    info: Option<Res<'w, RendererInfo>>,
}

impl<'w> DebugUi for &RendererDebugUi<'w> {
    fn show_debug(self, ui: &mut egui::Ui) {
        if let Some(info) = &self.info {
            egui::CollapsingHeader::new(format!(
                "Bytes last frame: {}",
                format_size(info.prepare_world_staged.total),
            ))
            .id_salt(ui.id().with("prepare_world_staged"))
            .default_open(true)
            .show(ui, |ui| {
                ui.label(format!(
                    "Command Queue: {}",
                    format_size(info.prepare_world_staged.command_queue),
                ));
                ui.label(format!(
                    "Instance Buffer: {}",
                    format_size(info.prepare_world_staged.instance_buffer),
                ));
                ui.label(format!(
                    "Camera Buffers: {}",
                    format_size(info.prepare_world_staged.camera_buffers),
                ));
            });
        }
    }
}
