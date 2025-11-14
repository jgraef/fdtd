pub mod args;
pub mod clipboard;
pub mod composer;
pub mod config;
pub mod files;
pub mod menubar;
pub mod start;

use std::borrow::Cow;

use chrono::Local;
use color_eyre::eyre::{
    Error,
    OptionExt,
};
use egui_file_dialog::FileDialog;
use image::RgbaImage;

use crate::app::{
    composer::Composer,
    config::AppConfig,
    files::AppFiles,
    menubar::{
        MenuBar,
        RecentlyOpenedFiles,
    },
    start::CreateAppContext,
};

#[derive(Debug)]
pub struct App {
    app_files: AppFiles,
    config: AppConfig,
    composer: Composer,
    file_dialog: FileDialog,
    show_about: bool,
    show_debug: bool,
    error_dialog: ErrorDialog,
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
        });

        // create file dialog for opening and saving files
        let file_dialog = FileDialog::new()
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .add_file_filter_extensions("NEC", vec!["nec"]);

        // create composer ui
        let mut composer = Composer::new(&context.wgpu_context);

        if context.args.new_file {
            // command line telling us to directly go to a new file
            composer.new_file(&context.config);
        }
        else if let Some(path) = &context.args.file {
            // if a file was passed via command line argument, open it

            RecentlyOpenedFiles::insert(
                &context.egui_context,
                path,
                context.config.recently_opened_files_limit,
            );

            composer
                .open_file(&context.config, path)
                .unwrap_or_else(|error| error_dialog.display_error(error));
        }

        Self {
            app_files: context.app_files,
            config: context.config,
            composer,
            file_dialog,
            show_about: false,
            show_debug: false,
            error_dialog,
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
                            self.save_screenshot(image)
                                .unwrap_or_else(|error| self.error_dialog.display_error(error));
                        }
                        _ => {}
                    }
                }
            });

            if take_screenshot {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        }

        // show top menubar
        MenuBar::new(self).show(ctx);

        // show composer UI
        self.composer.show(ctx);

        egui::Window::new("About")
            .movable(true)
            .collapsible(false)
            .open(&mut self.show_about)
            .show(ctx, |ui| {
                ui.label(format!("Version: {}", std::env!("CARGO_PKG_VERSION")))
                // todo: display other information (build commit hash, mayve
                // wgpu info?)
            });

        egui::Window::new("Debug Info")
            .movable(true)
            .default_size([300.0, 300.0])
            .max_size([f32::INFINITY, f32::INFINITY])
            .open(&mut self.show_debug)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("debug_panel")
                    .show(ui, |ui| {
                        egui::ScrollArea::both().show(ui, |ui| {
                            ui.collapsing("Settings", |ui| {
                                ctx.settings_ui(ui);
                            });

                            ui.collapsing("Inspection", |ui| {
                                ctx.inspection_ui(ui);
                            });

                            ui.collapsing("Memory", |ui| {
                                ctx.memory_ui(ui);
                            });

                            self.composer.show_debug(ui);
                        });
                    });
                ui.take_available_space();
            });

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

                        self.composer
                            .open_file(&self.config, path)
                            .unwrap_or_else(|error| self.error_dialog.display_error(error));
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

        self.error_dialog.show(ctx);
    }
}

fn todo_label(ui: &mut egui::Ui) {
    ui.label("todo");
}

#[derive(Clone, Copy, Debug)]
enum FileDialogAction {
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
}

#[derive(Debug, Default)]
pub struct ErrorDialog {
    error: Option<Error>,
}

impl ErrorDialog {
    pub fn display_error(&mut self, error: Error) {
        tracing::error!(?error);
        self.error = Some(error);
    }

    pub fn clear(&mut self) {
        self.error = None;
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if let Some(error) = &self.error {
            let mut open1 = true;
            let mut open2 = true;

            egui::Window::new("Error")
                .movable(true)
                .open(&mut open1)
                .collapsible(false)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("error_message")
                        .show(ui, |ui| {
                            egui::Frame::new().inner_margin(5).show(ui, |ui| {
                                ui.label(format!("{error:#}"));
                            });
                        });

                    ui.separator();

                    ui.with_layout(egui::Layout::right_to_left(Default::default()), |ui| {
                        if ui.button("Close").clicked() {
                            open2 = false;
                        }
                    });
                });

            if !open1 || !open2 {
                self.clear();
            }
        }
    }
}
