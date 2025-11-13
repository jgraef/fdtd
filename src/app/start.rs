use std::sync::Arc;

use color_eyre::eyre::{
    Error,
    eyre,
};
use eframe::NativeOptions;
use egui::ViewportBuilder;
use egui_wgpu::{
    SurfaceErrorAction,
    WgpuConfiguration,
    WgpuSetup,
    WgpuSetupCreateNew,
};

use crate::app::{
    clipboard::EguiClipboardPlugin,
    composer::renderer::{
        RendererConfig,
        WgpuContext,
    },
    files::AppFiles,
};

#[derive(Clone, Debug)]
pub struct CreateAppContext {
    pub wgpu_context: WgpuContext,
    pub egui_context: egui::Context,
    pub app_files: AppFiles,
}

pub trait CreateApp: Sized {
    type App: eframe::App;

    fn multisample_count(&self) -> u32 {
        4
    }

    fn depth_texture_format(&self) -> Option<wgpu::TextureFormat> {
        None
    }

    fn required_features(&self) -> wgpu::Features {
        Default::default()
    }

    fn required_limits(&self) -> wgpu::Limits {
        Default::default()
    }

    fn create_app(self, context: CreateAppContext) -> Self::App;

    fn run(self) -> Result<(), Error> {
        let multisample_count = self.multisample_count();
        let depth_texture_format = self.depth_texture_format();
        let required_features = self.required_features();
        let required_limits = self.required_limits();

        // https://docs.rs/egui-wgpu/0.33.0/src/egui_wgpu/lib.rs.html#375-385
        let (depth_buffer, stencil_buffer) = match depth_texture_format {
            None => (0, 0),
            Some(wgpu::TextureFormat::Stencil8) => (0, 8),
            Some(wgpu::TextureFormat::Depth16Unorm) => (16, 0),
            Some(wgpu::TextureFormat::Depth24Plus) => (24, 0),
            Some(wgpu::TextureFormat::Depth24PlusStencil8) => (24, 8),
            Some(wgpu::TextureFormat::Depth32Float) => (32, 0),
            Some(wgpu::TextureFormat::Depth32FloatStencil8) => (32, 8),
            Some(_) => panic!("Unsupported depth texture format: {depth_texture_format:?}"),
        };

        let app_files = AppFiles::open()?;

        eframe::run_native(
            "cem",
            NativeOptions {
                viewport: ViewportBuilder::default()
                    .with_title("cem")
                    .with_app_id("cem"),
                persistence_path: Some(app_files.egui_persist_path()),
                depth_buffer,
                stencil_buffer,
                multisampling: multisample_count as u16,
                wgpu_options: WgpuConfiguration {
                    on_surface_error: Arc::new(|error| {
                        if error == wgpu::SurfaceError::Outdated {
                            // ignore
                        }
                        else {
                            tracing::error!("{}", error);
                        }
                        SurfaceErrorAction::SkipFrame
                    }),
                    wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
                        device_descriptor: Arc::new(move |adapter| {
                            let adapter_info = adapter.get_info();
                            tracing::debug!(
                                backend = ?adapter_info.backend,
                                name = adapter_info.name,
                                "using adapter"
                            );

                            // see https://docs.rs/egui-wgpu/0.33.0/src/egui_wgpu/setup.rs.html#174
                            let base_limits = if adapter.get_info().backend == wgpu::Backend::Gl {
                                wgpu::Limits::downlevel_webgl2_defaults()
                            }
                            else {
                                wgpu::Limits::downlevel_defaults()
                            };
                            let mut required_limits =
                                base_limits.or_better_values_from(&required_limits);
                            let mut required_features = required_features;

                            if depth_buffer != 0 || stencil_buffer != 0 {
                                // When using a depth buffer, we have to be able to create a
                                // texture large enough for
                                // the entire surface, and we want to support 4k+
                                // displays.
                                required_limits.max_texture_dimension_2d =
                                    required_limits.max_texture_dimension_2d.max(8192);
                            }

                            if depth_buffer == 32 && stencil_buffer == 8 {
                                // Needs to be enabled for a Depth32FloatStencil8 texture
                                required_features.insert(wgpu::Features::DEPTH32FLOAT_STENCIL8);
                            }

                            wgpu::DeviceDescriptor {
                                label: Some("egui wgpu device"),
                                required_limits,
                                required_features,
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
                    depth_texture_format,
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

                // add our custom clipboard extension
                cc.egui_ctx.add_plugin(EguiClipboardPlugin);

                let create_app_context = CreateAppContext {
                    wgpu_context,
                    egui_context: cc.egui_ctx.clone(),
                    app_files,
                };

                Ok(Box::new(self.create_app(create_app_context)))
            }),
        )
        .map_err(|e| eyre!("{e}"))?;
        Ok(())
    }
}
