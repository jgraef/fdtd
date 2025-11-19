use std::time::Duration;

use nalgebra::{
    Isometry3,
    UnitQuaternion,
    Vector3,
};
use parry3d::bounding_volume::Aabb;
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    app::{
        composer::scene::{
            Scene,
            transform::Transform,
        },
        solver::{
            fdtd,
            traits::SolverInstance,
        },
    },
    physics::{
        PhysicalConstants,
        material::Material,
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolverConfig {
    pub label: String,

    pub common: SolverConfigCommon,

    pub specifics: SolverConfigSpecifics,
}

impl SolverConfig {
    pub fn solver_type(&self) -> SolverType {
        self.specifics.solver_type()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolverConfigCommon {
    pub volume: Volume,

    pub physical_constants: PhysicalConstants,

    pub default_material: Material,

    pub parallelization: Option<Parallelization>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SolverConfigSpecifics {
    Fdtd(SolverConfigFdtd),
    Feec(SolverConfigFeec),
}

impl SolverConfigSpecifics {
    pub fn solver_type(&self) -> SolverType {
        match self {
            Self::Fdtd(_) => SolverType::Fdtd,
            Self::Feec(_) => SolverType::Feec,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SolverConfigFdtd {
    pub resolution: fdtd::Resolution,
    pub stop_condition: StopCondition,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum StopCondition {
    Never,
    StepLimit { limit: usize },
    SimulatedTimeLimit { limit: f32 },
    RealtimeLimit { limit: Duration },
}

pub trait EvaluateStopCondition: SolverInstance {
    fn evaluate_stop_condition(
        &self,
        state: &Self::State,
        stop_condition: &StopCondition,
        time_elapsed: Duration,
    ) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Parallelization {
    MultiThreaded { num_threads: Option<usize> },
    Wgpu,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SolverConfigFeec {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SolverType {
    Fdtd,
    Feec,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Volume {
    Fixed(FixedVolume),
    SceneAabb(SceneAabbVolume),
}

impl Default for Volume {
    fn default() -> Self {
        Self::SceneAabb(Default::default())
    }
}

impl Volume {
    pub fn aabb(&self, scene: &Scene) -> Aabb {
        match self {
            Volume::Fixed(fixed_volume) => {
                Aabb::from_half_extents(
                    fixed_volume.isometry.translation.vector.into(),
                    fixed_volume.half_extents,
                )
            }
            Volume::SceneAabb(scene_aabb_volume) => {
                scene
                    .compute_aabb_relative_to_observer(
                        &Transform::from(scene_aabb_volume.rotation),
                        false,
                    )
                    .unwrap_or_else(|| {
                        // todo: or should we return None instead?
                        Aabb::new_invalid()
                    })
            }
        }
    }

    pub fn rotation(&self) -> UnitQuaternion<f32> {
        match self {
            Volume::Fixed(fixed_volume) => fixed_volume.isometry.rotation,
            Volume::SceneAabb(scene_aabb_volume) => scene_aabb_volume.rotation,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FixedVolume {
    pub isometry: Isometry3<f32>,
    pub half_extents: Vector3<f32>,
}

impl Default for FixedVolume {
    fn default() -> Self {
        Self {
            isometry: Isometry3::identity(),
            half_extents: Vector3::repeat(1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct SceneAabbVolume {
    pub rotation: UnitQuaternion<f32>,
    pub margin: Vector3<f32>,
}
