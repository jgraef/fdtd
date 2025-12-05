#![warn(clippy::todo, unused_qualifications)]

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

use bevy_ecs::resource::Resource;
pub use draw_commands::{
    DrawCommand,
    DrawCommandInfo,
};
pub use renderer::RendererConfig;
pub use systems::grab_draw_list_for_camera;

use crate::{
    command::Command,
    material::MaterialData,
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
