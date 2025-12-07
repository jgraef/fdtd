use bevy_ecs::{
    component::Component,
    reflect::ReflectComponent,
};
use bevy_reflect::{
    Reflect,
    ReflectSerialize,
    prelude::ReflectDefault,
};
use cem_probe::PropertiesUi;
use cem_scene::probe::{
    ComponentName,
    ReflectComponentUi,
};
use cem_util::egui::EguiUtilUiExt;
use serde::{
    Deserialize,
    Serialize,
};

/// Tag for entities that should be rendered
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, Component, Reflect)]
#[reflect(Component, ComponentUi, @ComponentName::new("Hidden"), Default, Serialize)]
pub struct Hidden;

impl PropertiesUi for Hidden {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        ui.noop()
    }
}
