use std::{
    num::NonZero,
    sync::Arc,
};

use color_eyre::eyre::Error;
use eframe::NativeOptions;
use egui::ViewportBuilder;
use egui_wgpu::{
    SurfaceErrorAction,
    WgpuConfiguration,
    WgpuSetup,
    WgpuSetupCreateNew,
};

use crate::{
    app::{
        App,
        args::Args,
        clipboard::EguiClipboardPlugin,
        composer::renderer::{
            EguiWgpuRenderer,
            RendererConfig,
        },
        config::AppConfig,
        files::AppFiles,
    },
    util::wgpu::WgpuContext,
};

#[derive(Clone, Debug)]
pub struct CreateAppContext {
    pub wgpu_context: WgpuContext,
    pub egui_context: egui::Context,
    pub renderer_config: RendererConfig,
    pub egui_wgpu_renderer: EguiWgpuRenderer,
    pub app_files: AppFiles,
    pub config: AppConfig,
    pub args: Args,
}

pub(super) fn run_app(args: Args) -> Result<(), Error> {
    let app_files = AppFiles::open()?;

    // load config
    let config = if args.ignore_config {
        AppConfig::default()
    }
    else {
        app_files.read_config_or_create::<AppConfig>()?
    };

    // these are more or less fixed
    let multisample_count = NonZero::new(4).unwrap(); // can really only be 1 or 4
    let depth_texture_format = Some(wgpu::TextureFormat::Depth24PlusStencil8);
    let required_features = wgpu::Features::default();
    let required_limits = Default::default();

    // derive `NativeOptions` values from depth texture format
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

    eframe::run_native(
        "cem",
        NativeOptions {
            viewport: ViewportBuilder::default()
                .with_title("cem")
                .with_app_id("cem"),
            persistence_path: Some(app_files.egui_persist_path()),
            depth_buffer,
            stencil_buffer,
            multisampling: multisample_count.get().try_into().unwrap(),
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
                    instance_descriptor: wgpu::InstanceDescriptor {
                        backends: config.graphics.backends,
                        ..Default::default()
                    }
                    .with_env(),
                    power_preference: config.graphics.power_preference,
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
            tracing::debug!(?renderer_config);

            // pass wgpu context to app (e.g. for compute shaders)
            let wgpu_context = WgpuContext::new(
                render_state.adapter.clone(),
                render_state.device.clone(),
                render_state.queue.clone(),
            );

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
                config,
                args,
                renderer_config,
                egui_wgpu_renderer: render_state.renderer.clone().into(),
            };

            Ok(Box::new(App::new(create_app_context)))
        }),
    )?;
    Ok(())
}
