use bevy_ecs::component::Component;
use nalgebra::{
    Isometry3,
    Point3,
};

use crate::transform::LocalTransform;

#[derive(Clone, Copy, Debug, Component, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlobalTransform {
    isometry: Isometry3<f32>,
}

impl GlobalTransform {
    pub(crate) fn from_local(local: LocalTransform) -> Self {
        Self {
            isometry: local.isometry,
        }
    }

    pub(crate) fn with_local(self, local: &LocalTransform) -> Self {
        Self {
            isometry: &self.isometry * local.isometry,
        }
    }

    #[cfg(test)]
    pub fn new_test(isometry: Isometry3<f32>) -> Self {
        Self { isometry }
    }

    pub fn isometry(&self) -> &Isometry3<f32> {
        &self.isometry
    }

    pub fn position(&self) -> Point3<f32> {
        self.isometry.translation.vector.into()
    }
}
