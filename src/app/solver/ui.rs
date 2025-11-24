use nalgebra::Vector3;

use crate::{
    app::{
        composer::properties::{
            HasChangeValue,
            PropertiesUi,
            PropertiesUiExt,
            TrackChanges,
            label_and_value,
        },
        solver::{
            config::{
                FixedVolume,
                SceneAabbVolume,
                SolverConfig,
                SolverConfigCommon,
                SolverConfigFdtd,
                SolverConfigSpecifics,
                StopCondition,
                Volume,
            },
            fdtd,
        },
    },
    physics::PhysicalConstants,
};

impl PropertiesUi for SolverConfig {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(ui, "Label", &mut changes, &mut self.label);

                ui.label("Volume");
                ui.indent("volume_ui", |ui| {
                    ui.properties(&mut self.common.volume);
                });

                ui.label("Physical Constants");
                ui.indent("volume_ui", |ui| {
                    ui.properties(&mut self.common.physical_constants);
                });

                // todo
                match &mut self.specifics {
                    SolverConfigSpecifics::Fdtd(_fdtd_config) => {}
                    SolverConfigSpecifics::Feec(_feec_config) => {}
                }
            })
            .response
    }
}

impl PropertiesUi for Volume {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                let id = egui::Id::new("volume_ui");

                let mut volume_type = VolumeType::from(&*self);
                ui.horizontal(|ui| {
                    changes.track(ui.selectable_value(
                        &mut volume_type,
                        VolumeType::Fixed,
                        "Fixed",
                    ));
                    changes.track(ui.selectable_value(
                        &mut volume_type,
                        VolumeType::SceneAabb,
                        "AABB",
                    ));
                });

                // if volume type changed, load in stored specifics
                if changes.changed() {
                    *self = match volume_type {
                        VolumeType::Fixed => {
                            Volume::Fixed(
                                ui.data(|data| data.get_temp::<FixedVolume>(id))
                                    .unwrap_or_default(),
                            )
                        }
                        VolumeType::SceneAabb => {
                            Volume::SceneAabb(
                                ui.data(|data| data.get_temp::<SceneAabbVolume>(id))
                                    .unwrap_or_default(),
                            )
                        }
                    }
                }

                // render specific ui
                match self {
                    Volume::Fixed(fixed_volume) => {
                        label_and_value(
                            ui,
                            "Volume Isometry",
                            &mut changes,
                            &mut fixed_volume.isometry,
                        );
                        label_and_value(
                            ui,
                            "Volume Half Extents",
                            &mut changes,
                            &mut fixed_volume.half_extents,
                        );
                    }
                    Volume::SceneAabb(scene_aabb_volume) => {
                        label_and_value(
                            ui,
                            "Orientation",
                            &mut changes,
                            &mut scene_aabb_volume.rotation,
                        );
                        label_and_value(ui, "Margin", &mut changes, &mut scene_aabb_volume.margin);
                    }
                }

                // if anything changed, store current specifics
                if changes.changed() {
                    ui.data_mut(|data| {
                        match self {
                            Volume::Fixed(fixed_volume) => data.insert_temp(id, *fixed_volume),
                            Volume::SceneAabb(scene_aabb_volume) => {
                                data.insert_temp(id, *scene_aabb_volume)
                            }
                        }
                    });
                }
            })
            .response;

        changes.propagated(response)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VolumeType {
    Fixed,
    SceneAabb,
}

impl From<&Volume> for VolumeType {
    fn from(value: &Volume) -> Self {
        match value {
            Volume::Fixed(_) => Self::Fixed,
            Volume::SceneAabb(_) => Self::SceneAabb,
        }
    }
}

impl PropertiesUi for PhysicalConstants {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        // todo: combo box with predefined defaults (e.g. REDUCED, SI)
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("SI").clicked() {
                        *self = Self::SI;
                    }
                    if ui.small_button("Reduced").clicked() {
                        *self = Self::REDUCED;
                    }
                });

                label_and_value(ui, "eps_0", &mut changes, &mut self.vacuum_permittivity);
                label_and_value(ui, "mu_0", &mut changes, &mut self.vacuum_permeability);
                ui.label(format!("c: {:e}", self.speed_of_light()));
            })
            .response;

        changes.propagated(response)
    }
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
                common: SolverConfigCommon {
                    volume: Default::default(),
                    physical_constants: PhysicalConstants::REDUCED,
                    default_material: Default::default(),
                    parallelization: None,
                    memory_limit: None,
                },
                specifics: SolverConfigSpecifics::Fdtd(SolverConfigFdtd {
                    resolution: fdtd::Resolution {
                        spatial: Vector3::repeat(1.0),
                        temporal: 0.25,
                    },
                    stop_condition: StopCondition::StepLimit { limit: 1000 },
                }),
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
                    .is_some_and(|selection| selection >= solver_configs.len())
                {
                    self.selection = solver_configs.len().checked_sub(1);
                }

                // dropdown menu for solver selection, and add/delete buttons
                // combine with label edit?
                ui.horizontal(|ui| {
                    let id = id.with("selection");

                    // fixme: doesn't show label for selection
                    self.selection
                        .map_or_else(
                            || egui::ComboBox::from_id_salt(id),
                            |selection| egui::ComboBox::new(id, &solver_configs[selection].label),
                        )
                        .show_ui(ui, |ui| {
                            for (i, solver_config) in solver_configs.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selection,
                                    Some(i),
                                    &solver_config.label,
                                );
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
