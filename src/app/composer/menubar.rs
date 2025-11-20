use nalgebra::{
    Vector2,
    Vector3,
};

use crate::app::{
    ErrorDialog,
    composer::{
        Composer,
        ComposerState,
        renderer::camera::CameraConfig,
    },
    menubar::setup_menu,
    solver::config::SolverConfig,
};

/// Composer proxy to build menubar.
#[derive(Debug)]
pub struct ComposerMenuElements<'a> {
    pub composer: &'a mut Composer,
    pub error_dialog: &'a mut ErrorDialog,
}

impl<'a> ComposerMenuElements<'a> {
    fn has_file_open(&self) -> bool {
        self.composer.state.is_some()
    }

    /// TODO: We might want to split the edit menu into several methods.
    pub fn edit_menu_buttons(&mut self, ui: &mut egui::Ui) {
        let (has_file_open, can_undo, can_redo, has_selected) = self
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
            self.composer.state.as_mut().unwrap().undo();
        }
        if ui
            .add_enabled(can_redo, egui::Button::new("Redo"))
            .clicked()
        {
            self.composer.state.as_mut().unwrap().redo();
        }

        ui.separator();

        if ui
            .add_enabled(has_selected, egui::Button::new("Cut"))
            .clicked()
        {
            self.composer.with_selected(|state, entities| {
                state.copy(ui.ctx(), entities.iter().copied());
                state.delete(entities);
            });
        }

        if ui
            .add_enabled(has_selected, egui::Button::new("Copy"))
            .clicked()
        {
            self.composer
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
            self.composer.with_selected(ComposerState::delete);
        }
    }

    pub fn selection_menu_buttons(&mut self, ui: &mut egui::Ui) {
        let mut selection = self
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
    }

    pub fn camera_submenu_button(&mut self, ui: &mut egui::Ui) {
        // note: right now this could all live directly in the view menu, but we will
        // eventually have multiple views/cameras.
        // todo: can we disable the whole menu
        ui.menu_button("Camera", |ui| {
            setup_menu(ui);

            let mut camera = self.composer.state.as_mut().map(|state| state.camera_mut());
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
                self.composer.state.as_mut().unwrap().open_camera_window();
            }
        });
    }

    pub fn configure_solver_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(self.has_file_open(), egui::Button::new("Configure Solvers"))
            .clicked()
        {
            self.composer
                .state
                .as_mut()
                .unwrap()
                .open_solver_config_window();
        }
    }

    pub fn solver_run_buttons(&mut self, ui: &mut egui::Ui) {
        let solver_button =
            |solver: &SolverConfig| egui::Button::new(("Run ", &solver.label, " Solver"));

        let mut i = 0;
        if let Some(state) = &mut self.composer.state {
            for solver_config in state.solver_configs.iter() {
                if ui.add(solver_button(solver_config)).clicked() {
                    tracing::debug!(
                        index = i,
                        label = solver_config.label,
                        ty = ?solver_config.solver_type(),
                        "run solver"
                    );
                    // for now we'll just send the config and scene to the runner to run it. but
                    // we'll need an intermediate step to rasterize/tesselate the scene
                    self.error_dialog.ok_or_show(
                        self.composer
                            .solver_runner
                            .run(solver_config, &mut state.scene),
                    );
                }
                i += 1;
            }
        }

        if i == 0 {
            ui.label("No Solvers configured");
        }
    }
}
