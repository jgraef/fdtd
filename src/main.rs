#![allow(dead_code)]

pub mod fdtd;
pub mod feec;
pub mod geometry;
pub mod ui;
pub mod util;

use std::sync::Arc;

use clap::{
    Parser,
    Subcommand,
};
use color_eyre::eyre::{
    Error,
    eyre,
};
use dotenvy::dotenv;
use eframe::NativeOptions;
use egui::ViewportBuilder;
use egui_wgpu::{
    SurfaceErrorAction,
    WgpuConfiguration,
    WgpuSetup,
    WgpuSetupCreateNew,
};
use wgpu::{
    Adapter,
    Device,
    Queue,
    SurfaceError,
    TextureFormat,
};

use crate::{
    fdtd::FdtdApp,
    feec::FeecApp,
};

fn main() -> Result<(), Error> {
    let _ = dotenv();
    tracing_subscriber::fmt::init();
    color_eyre::install()?;

    let args = Args::parse();
    match args.command {
        Command::Fdtd => {
            run_app(FdtdApp::new)?;
        }
        Command::Feec => {
            run_app(FeecApp::new)?;
        }
    }

    Ok(())
}

#[derive(Debug, Parser)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Fdtd,
    Feec,
}

fn run_app<A: eframe::App>(create_app: impl FnOnce(AppContext) -> A) -> Result<(), Error> {
    eframe::run_native(
        "FDTD",
        NativeOptions {
            viewport: ViewportBuilder::default()
                .with_title("cem")
                .with_app_id("cem"),
            // corresponds to `wgpu::TextureFormat::Depth32Float` (https://docs.rs/egui-wgpu/0.33.0/src/egui_wgpu/lib.rs.html#375-385)
            depth_buffer: 32,
            wgpu_options: WgpuConfiguration {
                on_surface_error: Arc::new(|error| {
                    if error == SurfaceError::Outdated {
                        // ignore
                    }
                    else {
                        tracing::error!("{}", error);
                    }
                    SurfaceErrorAction::SkipFrame
                }),
                wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
                    device_descriptor: Arc::new(|adapter| {
                        // see https://docs.rs/egui-wgpu/0.33.0/src/egui_wgpu/setup.rs.html#174
                        let base_limits = if adapter.get_info().backend == wgpu::Backend::Gl {
                            wgpu::Limits::downlevel_webgl2_defaults()
                        }
                        else {
                            wgpu::Limits::default()
                        };

                        wgpu::DeviceDescriptor {
                            label: Some("egui wgpu device"),
                            required_limits: wgpu::Limits {
                                // When using a depth buffer, we have to be able to create a texture
                                // large enough for the entire surface, and we want to support 4k+
                                // displays.
                                max_texture_dimension_2d: 8192,
                                ..base_limits
                            },
                            required_features: wgpu::Features {
                                features_wgpu: wgpu::FeaturesWGPU::POLYGON_MODE_LINE,
                                features_webgpu: Default::default(),
                            },
                            ..Default::default()
                        }
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        Box::new(|cc| {
            let render_state = cc
                .wgpu_render_state
                .as_ref()
                .expect("missing wgpu render state");

            {
                // insert surface texture format into renderer's callback resources.
                let mut renderer = render_state.renderer.write();
                renderer
                    .callback_resources
                    .insert(SurfaceTextureFormat(render_state.target_format));
            }

            let wgpu_context = WgpuContext {
                adapter: render_state.adapter.clone(),
                device: render_state.device.clone(),
                queue: render_state.queue.clone(),
                target_format: render_state.target_format,
            };

            let app_context = AppContext { wgpu_context };

            Ok(Box::new(create_app(app_context)))
        }),
    )
    .map_err(|e| eyre!("{e}"))?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct AppContext {
    pub wgpu_context: WgpuContext,
}

#[derive(Clone, Debug)]
pub struct WgpuContext {
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
    pub target_format: TextureFormat,
}

#[derive(Clone, Copy, Debug)]
pub struct SurfaceTextureFormat(pub TextureFormat);
