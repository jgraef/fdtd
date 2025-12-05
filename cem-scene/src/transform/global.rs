use bevy_ecs::{
    component::Component,
    reflect::ReflectComponent,
};
use bevy_reflect::Reflect;
use nalgebra::{
    Isometry3,
    Point3,
};

#[cfg(feature = "probe")]
use crate::probe::ReflectComponentUi;
use crate::transform::LocalTransform;

#[derive(Clone, Copy, Debug, PartialEq, Component, Reflect)]
#[reflect(Component)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "probe", reflect(ComponentUi, @crate::probe::ComponentName::new("Global Transform")))]
pub struct GlobalTransform {
    #[reflect(ignore)]
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
            isometry: self.isometry * local.isometry,
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

#[cfg(feature = "probe")]
impl cem_probe::PropertiesUi for GlobalTransform {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        self.isometry.properties_ui(ui, &Default::default())
    }
}
