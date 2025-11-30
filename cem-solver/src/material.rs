use std::fmt::Debug;

use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Copy, Serialize, Deserialize)]
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

// todo: good cc-0 database: https://github.com/polyanskiy/refractiveindex.info-database/
// todo: this belongs into a crate shared by solvers
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
