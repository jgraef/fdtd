//! TODO: move this. definitely doesn't belong into the composer

use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolverConfig {
    pub name: String,

    pub specificis: SolverConfigSpecifics,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SolverConfigSpecifics {
    Fdtd {
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
