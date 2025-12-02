use std::{
    borrow::Cow,
    num::NonZero,
    sync::Arc,
};

use cem_util::{
    egui::file_dialog::FileDialog,
    wgpu::buffer::StagingPool,
};
use chrono::Local;
use color_eyre::eyre::{
    Error,
    OptionExt,
};
use eframe::NativeOptions;
use egui::ViewportBuilder;
use egui_wgpu::{
    SurfaceErrorAction,
    WgpuConfiguration,
    WgpuSetup,
    WgpuSetupCreateNew,
};
use image::RgbaImage;

use crate::{
    args::Args,
    build_info::BUILD_INFO,
    composer::Composers,
    config::AppConfig,
    error::{
        ErrorDialog,
        ResultExt,
        show_error_dialog,
    },
    files::AppFiles,
    menubar::{
        MenuBar,
        RecentlyOpenedFiles,
    },
    renderer::{
        RendererConfig,
        plugin::RenderPlugin,
    },
    solver::runner::SolverRunner,
};

#[derive(Clone, Debug)]
pub struct CreateAppContext {
    pub wgpu_context: WgpuContext,
    pub renderer_config: RendererConfig,
    pub egui_context: egui::Context,
    pub app_files: AppFiles,
    pub config: AppConfig,
    pub args: Args,
}

#[derive(Clone, Debug)]
pub struct WgpuContext {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: Arc<wgpu::AdapterInfo>,
    pub staging_pool: StagingPool,
}

impl WgpuContext {
    pub fn new(
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        staging_chunk_size: wgpu::BufferSize,
    ) -> Self {
        let adapter_info = Arc::new(adapter.get_info());
        tracing::debug!(?adapter_info);
        let staging_pool = StagingPool::new(staging_chunk_size, "staging pool");

        Self {
            adapter,
            device,
            queue,
            adapter_info,
            staging_pool,
        }
    }
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

    let memory_hints = config.graphics.memory_hints.clone();

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
                // eframe uses AutoVsync by default, but this causes issues: when the window is not
                // visible, `SurfaceTexture::present` will block while the queue is locked, causing
                // all other calls to wgpu that use the queue to hang. Specifically this causes a
                // wgpu-based solver to hang while the window is not visible.
                //
                // https://github.com/gfx-rs/wgpu/issues/8597
                present_mode: wgpu::PresentMode::Mailbox,
                wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
                    instance_descriptor: wgpu::InstanceDescriptor {
                        backends: config.graphics.backends,
                        ..Default::default()
                    }
                    .with_env(),
                    power_preference: wgpu::PowerPreference::from_env()
                        .unwrap_or(config.graphics.power_preference),
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
                            experimental_features: wgpu::ExperimentalFeatures::disabled(),
                            memory_hints: memory_hints.clone(),
                            trace: wgpu::Trace::Off,
                        }
                    }),
                    native_adapter_selector: None,
                }),
                desired_maximum_frame_latency: None,
            },
            vsync: false,
            hardware_acceleration: eframe::HardwareAcceleration::Preferred,
            renderer: eframe::Renderer::Wgpu,
            run_and_return: true,
            event_loop_builder: None,
            window_builder: None,
            centered: false,
            persist_window: true,
            dithering: false,
        },
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);

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

            // wgpu context. used for setting up the renderer and running compute shaders
            let wgpu_context = WgpuContext::new(
                render_state.adapter.clone(),
                render_state.device.clone(),
                render_state.queue.clone(),
                config.graphics.staging_chunk_size,
            );

            // store wgpu context in egui context
            cc.egui_ctx.data_mut(|data| {
                data.insert_temp(egui::Id::NULL, wgpu_context.clone());
            });

            // add our custom clipboard extension
            //cc.egui_ctx.add_plugin(EguiClipboardPlugin);

            // this is the egui-wgpu renderer, which we can use to create egui textures from
            // wgpu textures and vice versa. in case we ever need it
            //
            // render_state.renderer.clone(),

            let create_app_context = CreateAppContext {
                wgpu_context,
                renderer_config,
                egui_context: cc.egui_ctx.clone(),
                app_files,
                config,
                args,
            };

            Ok(Box::new(App::new(create_app_context)))
        }),
    )?;
    Ok(())
}

#[derive(Debug)]
pub struct App {
    pub app_files: AppFiles,
    pub config: AppConfig,
    pub file_dialog: FileDialog,
    pub show_about: bool,
    pub solver_runner: SolverRunner,
    pub composers: Composers,
    pub wgpu_context: WgpuContext,
    pub renderer_config: RendererConfig,
}

impl App {
    pub fn new(context: CreateAppContext) -> Self {
        tracing::info!(?context.app_files);

        let mut error_dialog = ErrorDialog::default();

        // modify egui styles
        context.egui_context.all_styles_mut(|style| {
            style.compact_menu_style = false;
            // this doesn't seem to work :(
            style.spacing.menu_spacing = 0.0;
            style.visuals.menu_corner_radius = egui::CornerRadius::same(4);

            style.visuals.window_shadow.offset = [5, 10];
            style.visuals.window_shadow.blur = 8;
            style.visuals.window_shadow.spread = 0;

            style.visuals.popup_shadow.offset = [5, 10];
            style.visuals.popup_shadow.blur = 8;
            style.visuals.popup_shadow.spread = 0;
        });

        let render_plugin =
            RenderPlugin::new(context.wgpu_context.clone(), context.renderer_config);
        let mut composers = Composers::new(render_plugin);
        let solver_runner = SolverRunner::from_app_context(&context);

        // create file dialog for opening and saving files
        let file_dialog = FileDialog::new()
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .add_file_filter_extensions("NEC", vec!["nec"]);

        if context.args.new_file {
            // command line telling us to directly go to a new file
            composers.new_file(&context.config);
        }
        else if let Some(path) = &context.args.file {
            // if a file was passed via command line argument, open it

            RecentlyOpenedFiles::insert(
                &context.egui_context,
                path,
                context.config.recently_opened_files_limit,
            );

            composers
                .open_file(&context.config, path)
                .ok_or_handle(&mut error_dialog);
        }

        error_dialog.register_in_context(&context.egui_context);

        Self {
            app_files: context.app_files,
            config: context.config,
            file_dialog,
            show_about: false,
            solver_runner,
            composers,
            wgpu_context: context.wgpu_context,
            renderer_config: context.renderer_config,
        }
    }

    fn save_screenshot(&self, image: &egui::ColorImage) -> Result<(), Error> {
        let filename = format!("{}.png", Local::now().format("%Y-%m-%d_%H:%M:%S"));

        let screenshot_path = self.app_files.screenshots_dir().join(&filename);

        let image = RgbaImage::from_raw(
            image.width() as u32,
            image.height() as u32,
            image.as_raw().to_owned(),
        )
        .ok_or_eyre("Invalid image data provided by egui")?;

        image.save(&screenshot_path)?;
        tracing::info!(path = %screenshot_path.display(), "Screenshot saved");

        Ok(())
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        {
            let mut take_screenshot = false;

            ctx.input(|input| {
                for event in &input.events {
                    match event {
                        egui::Event::Key {
                            key: egui::Key::F6,
                            repeat: false,
                            pressed: true,
                            ..
                        } => {
                            take_screenshot = true;
                        }
                        egui::Event::Screenshot {
                            viewport_id: _,
                            user_data: _,
                            image,
                        } => {
                            self.save_screenshot(image).ok_or_handle(ctx);
                        }
                        _ => {}
                    }
                }
            });

            if take_screenshot {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        }

        egui::Panel::top("top_panel")
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(2, 2))
                    .fill(ctx.style().visuals.panel_fill),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        egui::Vec2::new(32.0, 32.0),
                        egui::Image::new(egui::include_image!("../../assets/logo.png"))
                            .fit_to_fraction([0.9; 2].into()),
                    );

                    ui.vertical(|ui| {
                        MenuBar::new(self).show(ui);
                        self.composers.show_tabs(ui);
                    });
                });
            });

        // show solver ui window
        self.solver_runner.show_active_solver_ui(ctx);

        self.composers.show(ctx);

        show_about_window(ctx, &mut self.show_about);

        self.show_debug_window(ctx);

        self.file_dialog.update(ctx);
        if let Some(path) = self.file_dialog.take_picked() {
            if let Some(file_dialog_action) =
                self.file_dialog.user_data::<FileDialogAction>().copied()
            {
                match file_dialog_action {
                    FileDialogAction::Open => {
                        RecentlyOpenedFiles::insert(
                            ctx,
                            &path,
                            self.config.recently_opened_files_limit,
                        );

                        self.composers
                            .open_file(&self.config, path)
                            .ok_or_handle(ctx);
                    }
                    FileDialogAction::SaveAs => {
                        tracing::debug!("todo: save as");
                    }
                }
            }
            else {
                tracing::warn!("File dialog without action");
            }
        }

        show_error_dialog(ctx);
    }
}

fn todo_label(ui: &mut egui::Ui) {
    ui.label("todo");
}

#[derive(Clone, Copy, Debug)]
pub enum FileDialogAction {
    Open,
    SaveAs,
}

#[derive(Clone)]
pub struct GithubUrls {
    pub repository: Cow<'static, str>,
}

impl GithubUrls {
    pub const PACKAGE: Self = Self {
        repository: Cow::Borrowed(std::env!("CARGO_PKG_REPOSITORY")),
    };

    pub fn license(&self) -> String {
        format!("{}/blob/main/LICENSE", self.repository)
    }

    pub fn issues(&self) -> String {
        format!("{}/issues", self.repository)
    }

    pub fn documentation(&self) -> String {
        format!("{}/blob/main/doc", self.repository)
    }

    pub fn release_notes(&self) -> String {
        format!("{}/releases", self.repository)
    }

    pub fn commit(&self, hash: &str) -> String {
        format!("{}/commit/{hash}", self.repository)
    }

    pub fn branch(&self, branch: &str) -> String {
        format!("{}/tree/{branch}", self.repository)
    }
}

fn show_about_window(ctx: &egui::Context, is_open: &mut bool) {
    egui::Window::new("About")
        .movable(true)
        .collapsible(false)
        .open(is_open)
        .show(ctx, |ui| {
            ui.label(format!("Version: {}", std::env!("CARGO_PKG_VERSION")));

            if let Some(branch) = BUILD_INFO.git_branch {
                ui.small("Branch:");
                ui.hyperlink_to(
                    egui::WidgetText::from(branch).monospace(),
                    GithubUrls::PACKAGE.branch(branch),
                );
            }

            if let Some(commit) = BUILD_INFO.git_commit {
                ui.small("Commit:");
                ui.hyperlink_to(
                    egui::WidgetText::from(commit).monospace(),
                    GithubUrls::PACKAGE.commit(commit),
                );
            }
        });
}
