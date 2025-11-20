use nalgebra::{
    Matrix3,
    Vector3,
};

use crate::{
    app::solver::fdtd::Resolution,
    physics::PhysicalConstants,
};

#[derive(Clone, Copy, Debug)]
pub struct PmlCell {
    /// psi auxiliary vector fields for cpml
    /// todo: the diagonal is 0, and this might be symmetric, so we can save
    /// some space.
    pub psi_e: Matrix3<f64>,
    pub psi_h: Matrix3<f64>,
    pub b: f64,
    pub c: f64,
}

impl PmlCell {
    pub fn new(
        resolution: &Resolution,
        physical_constants: &PhysicalConstants,
        sigma: f64,
        kappa: f64,
        a: f64,
    ) -> Self {
        let b = (-(sigma / kappa + a) * resolution.temporal
            / physical_constants.vacuum_permittivity)
            .exp();
        let c = sigma * (b - 1.0) / (sigma * kappa + kappa.powi(2) * a);
        Self {
            psi_e: Matrix3::zeros(),
            psi_h: Matrix3::zeros(),
            b,
            c,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_graded(
        resolution: &Resolution,
        physical_constants: &PhysicalConstants,
        m: f64,
        m_a: f64,
        sigma_max: f64,
        kappa_max: f64,
        a_max: f64,
        depth: f64,
    ) -> Self {
        // from: https://www.youtube.com/watch?v=fg6_YFzCXGk&t=3386s
        // m ~ 3-5
        // m_a ~ 1-5
        // sigma_max ~ ???
        // kappa_max ~ 1-5
        // a_max ~ 0.1
        let g1 = depth.powf(m);
        let g2 = (1.0 - depth).powf(m_a);
        let sigma = sigma_max * g1;
        let kappa = 1.0 + (kappa_max - 1.0) * g1;
        let a = a_max * g2;
        Self::new(resolution, physical_constants, sigma, kappa, a)
    }

    pub fn psi_e_total(&self) -> Vector3<f64> {
        psi_total(&self.psi_e)
    }

    pub fn psi_h_total(&self) -> Vector3<f64> {
        psi_total(&self.psi_h)
    }
}

fn psi_total(psi: &Matrix3<f64>) -> Vector3<f64> {
    Vector3::new(psi.m12 - psi.m13, psi.m23 - psi.m21, psi.m31 - psi.m32)
}
