use std::{
    collections::VecDeque,
    path::{
        Path,
        PathBuf,
    },
};

use nalgebra::{
    Vector2,
    Vector3,
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
        composer::{
            ComposerState,
            renderer::camera::CameraConfig,
        },
        solver::SolverConfig,
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

    /// To configure menus to our liking. Call from inside the menu.
    fn setup_menu(&self, ui: &mut egui::Ui) {
        let style = ui.style_mut();
        egui::containers::menu::menu_style(style);
        ui.set_min_width(150.0);
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            self.setup_menu(ui);

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
                                .unwrap_or_else(|error| self.app.error_dialog.display_error(error));
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
            self.setup_menu(ui);

            let (has_file_open, can_undo, can_redo, has_selected) = self
                .app
                .composer
                .state
                .as_ref()
                .map(|state| {
                    (
                        true,
                        state.has_undos(),
                        state.has_redos(),
                        !state.selection().is_empty(),
                    )
                })
                .unwrap_or_default();

            if ui
                .add_enabled(can_undo, egui::Button::new("Undo"))
                .clicked()
            {
                self.app.composer.state.as_mut().unwrap().undo();
            }
            if ui
                .add_enabled(can_redo, egui::Button::new("Redo"))
                .clicked()
            {
                self.app.composer.state.as_mut().unwrap().redo();
            }

            ui.separator();

            if ui
                .add_enabled(has_selected, egui::Button::new("Cut"))
                .clicked()
            {
                self.app.composer.with_selected(|state, entities| {
                    state.copy(ui.ctx(), entities.iter().copied());
                    state.delete(entities);
                });
            }

            if ui
                .add_enabled(has_selected, egui::Button::new("Copy"))
                .clicked()
            {
                self.app
                    .composer
                    .with_selected(|state, entities| state.copy(ui.ctx(), entities));
            }

            // todo: we should really use our own clipboard buffer for entity copy/paste and
            // check if there's anything in it.
            if ui
                .add_enabled(has_file_open, egui::Button::new("Paste"))
                .clicked()
            {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }

            ui.separator();

            if ui
                .add_enabled(has_selected, egui::Button::new("Delete"))
                .clicked()
            {
                self.app.composer.with_selected(ComposerState::delete);
            }
        });
    }

    fn selection_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Selection", |ui| {
            self.setup_menu(ui);

            let mut selection = self
                .app
                .composer
                .state
                .as_mut()
                .map(|state| state.selection_mut());

            let has_file_open = selection.is_some();
            let has_anything_selected = selection
                .as_ref()
                .map(|selection| !selection.is_empty())
                .unwrap_or_default();

            if ui
                .add_enabled(
                    has_file_open && has_anything_selected,
                    egui::Button::new("Clear Selection"),
                )
                .clicked()
            {
                selection.as_mut().unwrap().clear();
            }

            if ui
                .add_enabled(has_file_open, egui::Button::new("Select All"))
                .clicked()
            {
                selection.as_mut().unwrap().select_all();
            }
        });
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("View", |ui| {
            self.setup_menu(ui);

            // note: right now this could all live directly in the view menu, but we will
            // eventually have multiple views/cameras.
            // todo: can we disable the whole menu
            ui.menu_button("Camera", |ui| {
                self.setup_menu(ui);

                let mut camera = self
                    .app
                    .composer
                    .state
                    .as_mut()
                    .map(|state| state.camera_mut());
                let has_file_open = camera.is_some();
                let fit_camera_margin = Vector2::zeros();

                if ui
                    .add_enabled(has_file_open, egui::Button::new("Point Camera to Center"))
                    .on_hover_text("Turn camera towards center of scene")
                    .clicked()
                {
                    camera.as_mut().unwrap().point_to_scene_center();
                }

                if ui
                    .add_enabled(has_file_open, egui::Button::new("Fit Camera"))
                    .on_hover_text("Move camera forward/back until it fits the scene.")
                    .clicked()
                {
                    camera.as_mut().unwrap().fit_to_scene(&fit_camera_margin);
                }

                let mut fit_camera_along_axis_button = |axis, up, axis_label, tooltip| {
                    if ui
                        .add_enabled(
                            has_file_open,
                            egui::Button::new(("Fit Camera to ", axis_label)),
                        )
                        .on_hover_text(tooltip)
                        .clicked()
                    {
                        camera.as_mut().unwrap().fit_to_scene_looking_along_axis(
                            &axis,
                            &up,
                            &fit_camera_margin,
                        );
                    }
                };

                fit_camera_along_axis_button(
                    Vector3::x(),
                    Vector3::y(),
                    "+X",
                    "Look at YZ plane from left.",
                );
                fit_camera_along_axis_button(
                    -Vector3::x(),
                    Vector3::y(),
                    "-X",
                    "Look at YZ plane from right.",
                );
                fit_camera_along_axis_button(
                    Vector3::y(),
                    -Vector3::z(),
                    "+Y",
                    "Look at XZ plane from bottom.",
                );
                fit_camera_along_axis_button(
                    -Vector3::y(),
                    Vector3::z(),
                    "-Y",
                    "Look at XZ plane from top.",
                );
                fit_camera_along_axis_button(
                    Vector3::z(),
                    Vector3::y(),
                    "+Z",
                    "Look at XY plane from front.",
                );
                fit_camera_along_axis_button(
                    -Vector3::z(),
                    Vector3::y(),
                    "-Z",
                    "Look at XY plane from back.",
                );

                ui.separator();

                let mut dummy = CameraConfig::default();
                let camera_config = camera
                    .as_mut()
                    .and_then(|camera| camera.query::<&mut CameraConfig>())
                    .unwrap_or(&mut dummy);

                ui.add_enabled(
                    has_file_open,
                    egui::Checkbox::new(&mut camera_config.show_solid, "Show Solid"),
                );
                ui.add_enabled(
                    has_file_open,
                    egui::Checkbox::new(&mut camera_config.show_outline, "Show Outline"),
                );
                ui.add_enabled(
                    has_file_open,
                    egui::Checkbox::new(&mut camera_config.show_wireframe, "Show Wireframe"),
                );

                if ui
                    .add_enabled(has_file_open, egui::Button::new("Configure"))
                    .clicked()
                {
                    self.app
                        .composer
                        .state
                        .as_mut()
                        .unwrap()
                        .open_camera_window();
                }
            });
        });
    }

    fn run_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Run", |ui| {
            self.setup_menu(ui);

            let has_file_open = self.app.composer.has_file_open();

            if ui
                .add_enabled(has_file_open, egui::Button::new("Configure Solvers"))
                .clicked()
            {
                self.app
                    .composer
                    .state
                    .as_mut()
                    .unwrap()
                    .open_solver_config_window();
            }

            ui.separator();

            let solver_button =
                |solver: &SolverConfig| egui::Button::new(("Run ", &solver.label, " Solver"));

            let mut i = 0;
            if has_file_open {
                // show solvers configured in the composer state
                for solver in self.app.composer.solver_configurations() {
                    if ui.add(solver_button(solver)).clicked() {
                        self.app.composer.run_solver(i);
                    }
                    i += 1;
                }
            }

            if i == 0 {
                ui.label("No Solvers configured");
            }
        });
    }

    fn help_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Help", |ui| {
            self.setup_menu(ui);

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
                self.app.show_debug = true;
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
