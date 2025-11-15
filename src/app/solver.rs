//! TODO: still not sure where this belongs

use nalgebra::{
    Isometry3,
    Vector3,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::fdtd;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolverConfig {
    pub label: String,

    pub volume: Option<Volume>,

    pub specifics: SolverConfigSpecifics,
}

impl SolverConfig {
    pub fn solver_type(&self) -> SolverType {
        self.specifics.solver_type()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SolverConfigSpecifics {
    Fdtd {
        resolution: fdtd::simulation::Resolution,
        physical_constants: fdtd::simulation::PhysicalConstants,
        // todo
    },
    Feec {
        // todo
    },
}

impl SolverConfigSpecifics {
    pub fn solver_type(&self) -> SolverType {
        match self {
            Self::Fdtd { .. } => SolverType::Fdtd,
            Self::Feec { .. } => SolverType::Feec,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SolverType {
    Fdtd,
    Feec,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Volume {
    pub isometry: Isometry3<f32>,
    pub half_extents: Vector3<f32>,
}

impl Default for Volume {
    fn default() -> Self {
        Volume {
            isometry: Isometry3::identity(),
            half_extents: Vector3::repeat(1.0),
        }
    }
}
