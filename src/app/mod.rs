pub mod args;
pub mod composer;
pub mod config;
pub mod files;
pub mod start;

use std::{
    borrow::Cow,
    collections::VecDeque,
    path::{
        Path,
        PathBuf,
    },
};

use color_eyre::eyre::Error;
use egui::{
    Button,
    Layout,
};
use egui_file_dialog::FileDialog;
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    app::{
        args::Args,
        composer::Composer,
        config::AppConfig,
        files::AppFiles,
        start::CreateAppContext,
    },
    util::format_path,
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
    pub fn new(context: CreateAppContext, args: Args) -> Self {
        tracing::info!(?context.app_files);

        let mut error_dialog = ErrorDialog::default();

        // read config
        let config = context
            .app_files
            .read_config_or_create::<AppConfig>()
            .unwrap_or_else(|error| {
                error_dialog.display_error(error);
                Default::default()
            });

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

        if args.new_file {
            // command line telling us to directly go to a new file
            composer.new_file();
        }
        else if let Some(path) = &args.file {
            // if a file was passed via command line argument, open it

            RecentlyOpenedFiles::insert(
                &context.egui_context,
                path,
                config.recently_opened_files_limit,
            );

            composer
                .open_file(path)
                .unwrap_or_else(|error| error_dialog.display_error(error));
        }

        Self {
            app_files: context.app_files,
            config,
            composer,
            file_dialog,
            show_about: false,
            show_debug: false,
            error_dialog,
        }
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            setup_menu(ui);

            if ui.button("New File").clicked() {
                tracing::debug!("new file");
                self.composer.new_file();
            }

            ui.separator();

            if ui.button("Open File").clicked() {
                self.file_dialog.set_user_data(FileDialogAction::Open);
                self.file_dialog.pick_file();
            }
            ui.menu_button("Open Recent", |ui| {
                let recently_open = RecentlyOpenedFiles::get(ui.ctx());

                if !recently_open.files.is_empty() {
                    for path in recently_open.files {
                        if ui.button(format_path(&path)).clicked() {
                            RecentlyOpenedFiles::move_to_top(ui.ctx(), &path);

                            self.composer
                                .open_file(&path)
                                .unwrap_or_else(|error| self.error_dialog.display_error(error));
                        }
                    }
                }
                else {
                    ui.label("No recently open files");
                }
            });

            ui.separator();

            if ui
                .add_enabled(self.composer.has_open_file(), Button::new("Save"))
                .clicked()
            {
                tracing::debug!("todo: save");
            }
            if ui
                .add_enabled(self.composer.has_open_file(), Button::new("Save As"))
                .clicked()
            {
                self.file_dialog.set_user_data(FileDialogAction::SaveAs);
                self.file_dialog.pick_file();
            }

            ui.separator();

            if ui.button("Preferences").clicked() {
                tracing::debug!("todo: preferences");
            }

            ui.separator();

            if ui
                .add_enabled(self.composer.has_open_file(), Button::new("Close File"))
                .clicked()
            {
                self.composer.close_file();
            }

            ui.separator();

            if ui.button("Exit").clicked() {
                tracing::info!("App close requested by user");
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Edit", |ui| {
            setup_menu(ui);

            if ui.button("Undo").clicked() {
                tracing::debug!("todo: undo");
            }
            if ui.button("Redo").clicked() {
                tracing::debug!("todo: redo");
            }

            if ui.button("Cut").clicked() {
                tracing::debug!("todo: cut");
            }
            if ui.button("Copy").clicked() {
                tracing::debug!("todo: copy");
            }
            if ui.button("Past").clicked() {
                tracing::debug!("todo: paste");
            }
        });
    }

    fn help_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Help", |ui| {
            setup_menu(ui);

            if ui.button("Welcome").clicked() {
                tracing::debug!("todo: welcome");
            }
            if ui.button("Documentation").clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(GithubUrls::PACKAGE.documentation()));
            }
            if ui.button("Release Notes").clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(GithubUrls::PACKAGE.release_notes()));
            }
            if ui.button("Report Issue").clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(GithubUrls::PACKAGE.issues()));
            }
            if ui.button("View License").clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(GithubUrls::PACKAGE.license()));
            }
            if ui.button("About").clicked() {
                self.show_about = true;
            }
            if ui.button("Debug").clicked() {
                self.show_debug = true;
            }
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.file_menu(ui);
                self.edit_menu(ui);
                ui.menu_button("Selection", |ui| {
                    todo_label(ui);
                });
                ui.menu_button("View", |ui| {
                    todo_label(ui);
                });
                ui.menu_button("Run", |ui| {
                    todo_label(ui);
                });
                self.help_menu(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(&mut self.composer);
        });

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
            .open(&mut self.show_debug)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("debug_panel")
                    .show(ui, |ui| {
                        ui.collapsing("Settings", |ui| {
                            ctx.settings_ui(ui);
                        });

                        ui.collapsing("Inspection", |ui| {
                            ctx.inspection_ui(ui);
                        });

                        ui.collapsing("Memory", |ui| {
                            ctx.memory_ui(ui);
                        });
                    });
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
                            .open_file(path)
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

fn setup_menu(ui: &mut egui::Ui) {
    ui.set_min_width(150.0);
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

                    ui.with_layout(Layout::right_to_left(Default::default()), |ui| {
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

/// Container to store recently opened files in egui's memory
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RecentlyOpenedFiles {
    pub files: VecDeque<PathBuf>,
}

impl RecentlyOpenedFiles {
    pub fn get(ctx: &egui::Context) -> Self {
        ctx.memory_mut(|memory| {
            memory
                .data
                .get_persisted_mut_or_default::<Self>(egui::Id::NULL)
                .clone()
        })
    }

    pub fn insert(ctx: &egui::Context, path: impl AsRef<Path>, limit: usize) {
        ctx.memory_mut(|memory| {
            let this = memory
                .data
                .get_persisted_mut_or_default::<Self>(egui::Id::NULL);

            this.files.push_front(path.as_ref().to_owned());

            if this.files.len() > limit {
                this.files.pop_back();
            }
        });
    }

    pub fn move_to_top(ctx: &egui::Context, path: impl AsRef<Path>) {
        ctx.memory_mut(|memory| {
            let this = memory
                .data
                .get_persisted_mut_or_default::<Self>(egui::Id::NULL);

            let path = path.as_ref().to_owned();
            let files = std::mem::take(&mut this.files);

            // i think there's more efficient ways to do this, but meh
            this.files = files.into_iter().filter(|x: &PathBuf| x != &path).collect();

            this.files.push_front(path);
        });
    }
}
