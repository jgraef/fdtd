use std::{
    collections::VecDeque,
    path::{
        Path,
        PathBuf,
    },
};

use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    app::{
        App,
        FileDialogAction,
        GithubUrls,
        error_dialog::ResultExt,
    },
    util::format_path,
};

pub struct MenuBar<'a> {
    app: &'a mut App,
}

impl<'a> MenuBar<'a> {
    pub fn new(app: &'a mut App) -> Self {
        Self { app }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.file_menu(ui);
                self.edit_menu(ui);
                self.selection_menu(ui);
                self.view_menu(ui);
                self.run_menu(ui);
                self.help_menu(ui);
            });
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            setup_menu(ui);

            if ui.button("New File").clicked() {
                tracing::debug!("new file");
                self.app.composer.new_file(&self.app.config);
            }

            ui.separator();

            if ui.button("Open File").clicked() {
                self.app.file_dialog.set_user_data(FileDialogAction::Open);
                self.app.file_dialog.pick_file();
            }
            ui.menu_button("Open Recent", |ui| {
                let recently_open = RecentlyOpenedFiles::get(ui.ctx());

                if !recently_open.files.is_empty() {
                    for path in recently_open.files {
                        if ui.button(format_path(&path)).clicked() {
                            RecentlyOpenedFiles::move_to_top(ui.ctx(), &path);

                            self.app
                                .composer
                                .open_file(&self.app.config, &path)
                                .ok_or_handle(&*ui);
                        }
                    }
                }
                else {
                    ui.label("No recently open files");
                }
            });

            ui.separator();

            if ui
                .add_enabled(self.app.composer.has_file_open(), egui::Button::new("Save"))
                .clicked()
            {
                tracing::debug!("todo: save");
            }
            if ui
                .add_enabled(
                    self.app.composer.has_file_open(),
                    egui::Button::new("Save As"),
                )
                .clicked()
            {
                self.app.file_dialog.set_user_data(FileDialogAction::SaveAs);
                self.app.file_dialog.pick_file();
            }

            ui.separator();

            if ui.button("Preferences").clicked() {
                tracing::debug!("todo: preferences");
            }

            ui.separator();

            if ui
                .add_enabled(
                    self.app.composer.has_file_open(),
                    egui::Button::new("Close File"),
                )
                .clicked()
            {
                self.app.composer.close_file();
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
            self.app.composer.menu_elements().edit_menu_buttons(ui);
        });
    }

    fn selection_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Selection", |ui| {
            setup_menu(ui);
            self.app.composer.menu_elements().selection_menu_buttons(ui);
        });
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("View", |ui| {
            setup_menu(ui);
            self.app.composer.menu_elements().camera_submenu_button(ui);
        });
    }

    fn run_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Run", |ui| {
            setup_menu(ui);
            let mut composer_menu_elements = self.app.composer.menu_elements();

            composer_menu_elements.configure_solver_button(ui);
            ui.separator();
            composer_menu_elements.solver_run_buttons(ui);
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
                self.app.show_about = true;
            }
            if ui.button("Debug").clicked() {
                // this needs improvement, but we want the open state be persisted
                let debug_open_id = egui::Id::new("debug_open");
                ui.data_mut(|data| data.insert_persisted(debug_open_id, true));
            }
        });
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

/// To configure menus to our liking. Call from inside the menu.
pub fn setup_menu(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    egui::containers::menu::menu_style(style);
    ui.set_min_width(150.0);
}
