mod boundary_condition;
pub mod cpu;
pub mod pml;
mod strider;
mod util;
pub mod wgpu;

use nalgebra::Vector3;
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    app::solver::fdtd::strider::Strider,
    physics::PhysicalConstants,
};

#[derive(Clone, Copy, Debug)]
pub struct FdtdSolverConfig {
    pub resolution: Resolution,
    pub physical_constants: PhysicalConstants,
    pub size: Vector3<f64>,
}

impl FdtdSolverConfig {
    pub fn size(&self) -> Vector3<usize> {
        self.size
            .component_div(&self.resolution.spatial)
            .map(|x| (x.ceil() as usize).max(1))
    }

    pub fn strider(&self) -> Strider {
        Strider::new(&self.size())
    }

    pub fn num_cells(&self) -> usize {
        self.size().product()
    }
}

pub fn estimate_temporal_from_spatial_resolution(
    speed_of_light: f64,
    spatial_resolution: &Vector3<f64>,
) -> f64 {
    spatial_resolution.min() / (speed_of_light * 3.0f64.sqrt())
}

pub fn estimate_spatial_from_temporal_resolution(
    speed_of_light: f64,
    temporal_resolution: f64,
) -> Vector3<f64> {
    Vector3::repeat(temporal_resolution * speed_of_light * 3.0f64.sqrt())
}

pub fn estimate_spatial_resolution_from_min_wavelength(min_wavelength: f64) -> Vector3<f64> {
    Vector3::repeat(min_wavelength / (9.0f64 * 3.0f64.sqrt()))
}

pub fn estimate_temporal_resolution_from_max_frequency(max_frequency: f64) -> f64 {
    1.0f64 / (9.0f64 * 3.0f64 * max_frequency)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Resolution {
    pub spatial: Vector3<f64>,
    pub temporal: f64,
}

impl Resolution {
    pub fn estimate_from_min_wavelength(
        physical_constants: &PhysicalConstants,
        min_wavelength: f64,
    ) -> Self {
        let spatial = estimate_spatial_resolution_from_min_wavelength(min_wavelength);
        let temporal = estimate_temporal_from_spatial_resolution(
            physical_constants.speed_of_light(),
            &spatial,
        );
        Self { spatial, temporal }
    }

    pub fn estimate_from_max_frequency(
        physical_constants: &PhysicalConstants,
        max_frequency: f64,
    ) -> Self {
        let temporal = estimate_temporal_resolution_from_max_frequency(max_frequency);
        let spatial = estimate_spatial_from_temporal_resolution(
            physical_constants.speed_of_light(),
            temporal,
        );
        Self { spatial, temporal }
    }
}
