use nalgebra::{
    UnitVector3,
    Vector3,
};

use crate::{
    fdtd::Resolution,
    material::PhysicalConstants,
};

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "bevy_ecs", derive(bevy_ecs::component::Component))]
pub struct GradedPml {
    pub m: f64,
    pub m_a: f64,
    pub sigma_max: f64,
    pub kappa_max: f64,
    pub a_max: f64,
    pub normal: UnitVector3<f32>,
}

/// Coefficients for pml
///
/// See CE p304
///
/// These are only non-zero in the PML regions
#[derive(Clone, Copy, Debug, Default)]
pub struct PmlCoefficients {
    pub b: Vector3<f64>,
    pub c: Vector3<f64>,
}

impl PmlCoefficients {
    pub fn new(
        resolution: &Resolution,
        physical_constants: &PhysicalConstants,
        sigma: f64,
        kappa: f64,
        a: f64,
        normal: UnitVector3<f64>,
    ) -> Self {
        // see CE p304

        // 7.102
        let b = (-((sigma / (physical_constants.vacuum_permittivity * kappa)
            + a / physical_constants.vacuum_permittivity)
            * resolution.temporal))
            .exp();

        // 7.99
        let c = sigma * (b - 1.0) / (sigma * kappa + kappa.powi(2) * a);

        Self {
            b: b * normal.into_inner(),
            c: c * normal.into_inner(),
        }
    }

    /// normal points into the material (depth is its length)
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
        normal: UnitVector3<f64>,
    ) -> Self {
        // from: https://www.youtube.com/watch?v=fg6_YFzCXGk&t=3386s
        // m ~ 3-5
        // m_a ~ 1-5
        // sigma_max ~ ???
        // kappa_max ~ 1-5
        // a_max ~ 0.1
        //
        // there might be better formulas on p294
        let g1 = depth.powf(m);
        let g2 = (1.0 - depth).powf(m_a);
        let sigma = sigma_max * g1;
        let kappa = 1.0 + (kappa_max - 1.0) * g1;
        let a = a_max * g2;
        Self::new(resolution, physical_constants, sigma, kappa, a, normal)
    }
}

// this was used for something but I don't remember what
//fn psi_total(psi: &Matrix3<f64>) -> Vector3<f64> {
//    Vector3::new(psi.m12 - psi.m13, psi.m23 - psi.m21, psi.m31 - psi.m32)
//}
