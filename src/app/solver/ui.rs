use nalgebra::Vector3;

use crate::{
    app::{
        composer::properties::{
            PropertiesUi,
            PropertiesUiExt,
            TrackChanges,
            label_and_value,
        },
        solver::config::{
            SolverConfig,
            SolverConfigSpecifics,
            Volume,
        },
    },
    fdtd,
};

impl PropertiesUi for SolverConfig {
    type Config = SolverConfigUiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(ui, "Label", &mut changes, &mut self.label);

                let mut volume_used = self.volume.is_some();
                ui.checkbox(&mut volume_used, "Volume");
                if volume_used && self.volume.is_none() {
                    self.volume = Some(config.default_volume);
                }
                if let Some(volume) = &mut self.volume {
                    label_and_value(ui, "Volume Isometry", &mut changes, &mut volume.isometry);
                    label_and_value(
                        ui,
                        "Volume Half Extents",
                        &mut changes,
                        &mut volume.half_extents,
                    );
                }

                // todo
                match self.specifics {
                    SolverConfigSpecifics::Fdtd { .. } => {}
                    SolverConfigSpecifics::Feec {} => {}
                }
            })
            .response
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SolverConfigUiConfig {
    pub default_volume: Volume,
}

#[derive(Debug)]
pub struct SolverConfigUiWindow {
    pub selection: Option<usize>,
    pub is_open: bool,
    pub default_solver_config: SolverConfig,
}

impl Default for SolverConfigUiWindow {
    fn default() -> Self {
        Self {
            // default to the first one. this will be sanitized to None later, if it doesn't eixst
            selection: Some(0),
            is_open: false,
            default_solver_config: SolverConfig {
                label: "New solver".to_owned(),
                volume: None,
                physical_constants: fdtd::PhysicalConstants::REDUCED,
                specifics: SolverConfigSpecifics::Fdtd {
                    resolution: fdtd::Resolution {
                        spatial: Vector3::repeat(1.0),
                        temporal: 0.25,
                    },
                },
            },
        }
    }
}

impl SolverConfigUiWindow {
    pub fn with_default_solver_config(mut self, default_solver_config: SolverConfig) -> Self {
        self.default_solver_config = default_solver_config;
        self
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn show(&mut self, ctx: &egui::Context, solver_configs: &mut Vec<SolverConfig>) {
        let id = egui::Id::new("solver_config_ui_window");

        egui::Window::new("Configure Solver")
            .id(id)
            .movable(true)
            .collapsible(true)
            .open(&mut self.is_open)
            .show(ctx, |ui| {
                // sanity check if selection is out of bounds
                if self
                    .selection
                    .map_or(false, |selection| selection >= solver_configs.len())
                {
                    self.selection = solver_configs.len().checked_sub(1);
                }

                // dropdown menu for solver selection, and add/delete buttons
                ui.horizontal(|ui| {
                    let id = id.with("selection");

                    // fixme: doesn't show label for selection
                    let combo_box = self.selection.map_or_else(
                        || egui::ComboBox::from_id_salt(id),
                        |selection| egui::ComboBox::new(id, &solver_configs[selection].label),
                    );

                    combo_box.show_ui(ui, |ui| {
                        for (i, solver_config) in solver_configs.iter().enumerate() {
                            ui.selectable_value(&mut self.selection, Some(i), &solver_config.label);
                        }
                    });

                    let has_selection = self.selection.is_some();

                    if ui.add(egui::Button::new("+")).clicked() {
                        self.selection = Some(solver_configs.len());
                        solver_configs.push(self.default_solver_config.clone());
                    }

                    if ui
                        .add_enabled(has_selection, egui::Button::new("-"))
                        .clicked()
                    {
                        // todo: ask for confirmation
                    }
                });

                // property ui for selected solver
                if let Some(selection) = self.selection {
                    ui.properties(&mut solver_configs[selection]);
                }
                else {
                    ui.label("No solver selected");
                }
            });
    }
}
