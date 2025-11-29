// todo: good cc-0 database: https://github.com/polyanskiy/refractiveindex.info-database/

use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    app::composer::properties::{
        PropertiesUi,
        TrackChanges,
        label_and_value,
    },
    impl_register_component,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Material {
    /// mu_r
    pub relative_permeability: f64,
    /// sigma_m
    pub magnetic_conductivity: f64,

    /// epsilon_r
    pub relative_permittivity: f64,
    /// sigma
    pub eletrical_conductivity: f64,
}

impl Material {
    pub const VACUUM: Self = Self {
        relative_permeability: 1.0,
        magnetic_conductivity: 0.0,
        relative_permittivity: 1.0,
        eletrical_conductivity: 0.0,
    };
}

impl Default for Material {
    fn default() -> Self {
        Self::VACUUM
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

impl_register_component!(Material where ComponentUi, default);
