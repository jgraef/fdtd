#![allow(dead_code)]

pub mod composer;
pub mod fdtd;
pub mod feec;
pub mod file_formats;
pub mod geometry;
pub mod util;

use std::{
    fs::File,
    io::BufReader,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use clap::{
    Parser,
    Subcommand,
};
use color_eyre::eyre::{
    Context,
    Error,
    eyre,
};
use directories::ProjectDirs;
use dotenvy::dotenv;
use eframe::NativeOptions;
use egui::ViewportBuilder;
use egui_wgpu::{
    SurfaceErrorAction,
    WgpuConfiguration,
    WgpuSetup,
    WgpuSetupCreateNew,
};
use serde::{
    Serialize,
    de::DeserializeOwned,
};
use wgpu::SurfaceError;

use crate::{
    composer::renderer::{
        RendererConfig,
        WgpuContext,
    },
    file_formats::nec::NecFile,
};

fn main() -> Result<(), Error> {
    let _ = dotenv();
    tracing_subscriber::fmt::init();
    color_eyre::install()?;

    let args = Args::parse();
    match args.command {
        Command::Fdtd(args) => {
            args.run()?;
        }
        Command::Feec(args) => {
            args.run()?;
        }
        Command::ReadNec { file } => {
            let reader = BufReader::new(File::open(&file)?);
            let nec = NecFile::from_reader(reader)?;
            println!("{nec:#?}");
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
    Fdtd(fdtd::Args),
    Feec(feec::Args),
    ReadNec { file: PathBuf },
}

fn run_app<A: eframe::App>(create_app: impl FnOnce(CreateAppContext) -> A) -> Result<(), Error> {
    let multisample_count = 4;

    let app_files = AppFiles::open()?;

    eframe::run_native(
        "cem",
        NativeOptions {
            viewport: ViewportBuilder::default()
                .with_title("cem")
                .with_app_id("cem"),
            persistence_path: Some(app_files.egui_persist_path()),
            // corresponds to `wgpu::TextureFormat::Depth32Float` (https://docs.rs/egui-wgpu/0.33.0/src/egui_wgpu/lib.rs.html#375-385)
            depth_buffer: 32,
            multisampling: multisample_count as u16,
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

            // some config options our renderer needs to know
            let renderer_config = RendererConfig {
                target_texture_format: render_state.target_format,
                depth_texture_format: wgpu::TextureFormat::Depth32Float,
                multisample_count,
            };

            // pass wgpu context to app (e.g. for compute shaders)
            let wgpu_context = WgpuContext {
                adapter: render_state.adapter.clone(),
                device: render_state.device.clone(),
                queue: render_state.queue.clone(),
                renderer_config,
            };

            // store wgpu context in egui context
            cc.egui_ctx.data_mut(|data| {
                data.insert_temp(egui::Id::NULL, wgpu_context.clone());
            });

            let create_app_context = CreateAppContext {
                wgpu_context,
                egui_context: cc.egui_ctx.clone(),
                app_files,
            };

            Ok(Box::new(create_app(create_app_context)))
        }),
    )
    .map_err(|e| eyre!("{e}"))?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct CreateAppContext {
    pub wgpu_context: WgpuContext,
    pub egui_context: egui::Context,
    pub app_files: AppFiles,
}

pub trait CreateApp: Sized {
    type App: eframe::App;

    fn create_app(self, context: CreateAppContext) -> Self::App;

    fn run(self) -> Result<(), Error> {
        run_app(|context| self.create_app(context))
    }
}

#[derive(Clone, Debug)]
pub struct AppFiles {
    project_dirs: Arc<ProjectDirs>,
}

impl AppFiles {
    pub fn new(project_dirs: ProjectDirs) -> Self {
        Self {
            project_dirs: Arc::new(project_dirs),
        }
    }

    pub fn create_directories(&self) -> Result<(), Error> {
        std::fs::create_dir_all(self.state_dir_with_fallback())?;
        std::fs::create_dir_all(self.project_dirs.config_local_dir())?;
        Ok(())
    }

    pub fn open() -> Result<Self, Error> {
        let app_files = Self::default();
        app_files.create_directories()?;
        Ok(app_files)
    }

    /// Path to state directory.
    ///
    /// This tries to use the system's canonical state directory. If this is
    /// undefined, it will use the data-local directory.
    pub fn state_dir_with_fallback(&self) -> &Path {
        self.project_dirs
            .state_dir()
            .unwrap_or_else(|| self.project_dirs.data_local_dir())
    }

    /// Returns path to file for egui's persistence.
    pub fn egui_persist_path(&self) -> PathBuf {
        self.state_dir_with_fallback().join("ui_state")
    }

    pub fn read_config_or_create<T>(&self) -> Result<T, Error>
    where
        T: Serialize + DeserializeOwned + Default,
    {
        let path = self.project_dirs.config_local_dir().join("config.toml");

        let config = if !path.exists() {
            tracing::info!(path = %path.display(), "Creating config file");
            let config = T::default();
            let toml = toml::to_string_pretty(&config)?;
            std::fs::write(&path, &toml)
                .with_context(|| format!("Could not write config file: {}", path.display()))?;
            config
        }
        else {
            tracing::info!(path = %path.display(), "Reading config file");
            let toml = std::fs::read(&path)
                .with_context(|| format!("Could not read config file: {}", path.display()))?;

            toml::from_slice(&toml)
                .with_context(|| format!("Invalid config file: {}", path.display()))?
        };

        Ok(config)
    }
}

impl Default for AppFiles {
    fn default() -> Self {
        Self::new(
            ProjectDirs::from("", "switch", std::env!("CARGO_PKG_NAME"))
                .expect("Failed to create ProjectDirs"),
        )
    }
}
