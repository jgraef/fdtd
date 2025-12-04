use nalgebra::{
    Vector2,
    Vector3,
};

use crate::{
    composer::{
        ComposerState,
        Composers,
    },
    error::ResultExt,
    menubar::setup_menu,
    solver::{
        config::SolverConfig,
        runner::SolverRunner,
    },
};

/// Composer proxy to build menubar.
#[derive(Debug)]
pub struct ComposerMenuElements<'a> {
    pub composers: &'a mut Composers,
    pub solver_runner: &'a mut SolverRunner,
}

impl<'a> ComposerMenuElements<'a> {
    /// TODO: We might want to split the edit menu into several methods.
    pub fn edit_menu_buttons(&mut self, ui: &mut egui::Ui) {
        let (has_file_open, can_undo, can_redo, has_selected) = self
            .composers
            .with_active(|composer| {
                (
                    true,
                    composer.has_undos(),
                    composer.has_redos(),
                    !composer.selection().is_empty(),
                )
            })
            .unwrap_or_default();

        if ui
            .add_enabled(can_undo, egui::Button::new("Undo"))
            .clicked()
        {
            self.composers.with_active(|composer| composer.undo());
        }
        if ui
            .add_enabled(can_redo, egui::Button::new("Redo"))
            .clicked()
        {
            self.composers.with_active(|composer| composer.redo());
        }

        ui.separator();

        if ui
            .add_enabled(has_selected, egui::Button::new("Cut"))
            .clicked()
        {
            self.composers.with_selected(|state, entities| {
                state.copy(ui.ctx(), entities.iter().copied());
                state.delete(entities);
            });
        }

        if ui
            .add_enabled(has_selected, egui::Button::new("Copy"))
            .clicked()
        {
            self.composers
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
            self.composers.with_selected(ComposerState::delete);
        }
    }

    pub fn selection_menu_buttons(&mut self, ui: &mut egui::Ui) {
        let mut selection = self.composers.with_active(|composer| composer.selection());

        let has_file_open = selection.is_some();
        let has_anything_selected = selection
            .as_mut()
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

            let mut camera = self.composers.with_active(|composer| composer.camera());
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

            if ui
                .add_enabled(has_file_open, egui::Button::new("Configure"))
                .clicked()
            {
                self.composers
                    .with_active(|composer| composer.open_camera_window());
            }
        });
    }

    pub fn configure_solver_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                self.composers.has_file_open(),
                egui::Button::new("Configure Solvers"),
            )
            .clicked()
        {
            self.composers
                .with_active(|composer| composer.open_solver_config_window());
        }
    }

    pub fn solver_run_buttons(&mut self, ui: &mut egui::Ui) {
        let solver_button =
            |solver: &SolverConfig| egui::Button::new(("Run ", &solver.label, " Solver"));

        let mut i = 0;

        self.composers.with_active(|composer| {
            for solver_config in composer.solver_configs.iter() {
                if ui.add(solver_button(solver_config)).clicked() {
                    tracing::debug!(
                        index = i,
                        label = solver_config.label,
                        ty = ?solver_config.solver_type(),
                        "run solver"
                    );
                    // for now we'll just send the config and scene to the runner to run it. but
                    // we'll need an intermediate step to rasterize/tesselate the scene
                    self.solver_runner
                        .run(solver_config, &mut composer.scene)
                        .ok_or_handle(&*ui);
                }
                i += 1;
            }
        });

        if i == 0 {
            ui.label("No Solvers configured");
        }
    }
}
