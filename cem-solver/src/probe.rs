use cem_probe::{
    PropertiesUi,
    TrackChanges,
    label_and_value,
};

use crate::material::{
    Material,
    PhysicalConstants,
};

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

impl PropertiesUi for Material {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(
                    ui,
                    "Relative Permeability",
                    &mut changes,
                    &mut self.relative_permeability,
                );
                label_and_value(
                    ui,
                    "Magnetic Conductivity",
                    &mut changes,
                    &mut self.magnetic_conductivity,
                );
                label_and_value(
                    ui,
                    "Relative Permittivity",
                    &mut changes,
                    &mut self.relative_permittivity,
                );
                label_and_value(
                    ui,
                    "Electrical Conductivity",
                    &mut changes,
                    &mut self.eletrical_conductivity,
                );
            })
            .response;

        changes.propagated(response)
    }
}
