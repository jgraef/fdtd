// todo: good cc-0 database: https://github.com/polyanskiy/refractiveindex.info-database/

#[derive(Clone, Copy, Debug)]
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
