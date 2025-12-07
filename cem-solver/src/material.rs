use std::fmt::Debug;

#[cfg(feature = "bevy_ecs")]
use bevy_ecs::reflect::ReflectComponent;
#[cfg(all(feature = "serde", feature = "bevy_ecs"))]
use bevy_reflect::ReflectSerialize;
#[cfg(feature = "probe")]
use cem_probe::{
    PropertiesUi,
    TrackChanges,
    label_and_value,
};
#[cfg(all(feature = "probe", feature = "bevy_ecs"))]
use cem_scene::probe::{
    ComponentName,
    ReflectComponentUi,
};

#[derive(Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PhysicalConstants {
    pub vacuum_permittivity: f64,
    pub vacuum_permeability: f64,
}

impl Debug for PhysicalConstants {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicalConstants")
            .field("vacuum_permittivity", &self.vacuum_permittivity)
            .field("vacuum_permeability", &self.vacuum_permeability)
            .field("speed_of_light", &self.speed_of_light())
            .finish()
    }
}

impl Default for PhysicalConstants {
    fn default() -> Self {
        Self::SI
    }
}

impl PhysicalConstants {
    pub const SI: Self = Self {
        vacuum_permittivity: 8.8541878188e-12,
        vacuum_permeability: 1.25663706127e-6,
    };

    pub const REDUCED: Self = Self {
        vacuum_permittivity: 1.0,
        vacuum_permeability: 1.0,
    };

    pub fn speed_of_light(&self) -> f64 {
        (self.vacuum_permittivity * self.vacuum_permeability).powf(-0.5)
    }

    pub fn frequency_to_wavelength(&self, frequency: f64) -> f64 {
        self.speed_of_light() / frequency
    }

    pub fn wavelength_to_frequency(&self, wavelength: f64) -> f64 {
        self.speed_of_light() / wavelength
    }
}

#[cfg(feature = "probe")]
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

// todo: good cc-0 database: https://github.com/polyanskiy/refractiveindex.info-database/
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "bevy_ecs",
    derive(bevy_ecs::component::Component, bevy_reflect::Reflect),
    reflect(Component)
)]
#[cfg_attr(all(feature = "probe", feature = "bevy_ecs"), reflect(ComponentUi, @ComponentName::new("Material")))]
#[cfg_attr(all(feature = "serde", feature = "bevy_ecs"), reflect(Serialize))]
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

#[cfg(feature = "probe")]
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
