use bevy_ecs::{
    component::Component,
    reflect::ReflectComponent,
};
use bevy_reflect::{
    Reflect,
    prelude::ReflectDefault,
};
use cem_probe::{
    PropertiesUi,
    TrackChanges,
    label_and_value,
    label_and_value_with_config,
    std::NumericPropertyUiConfig,
};
use cem_scene::probe::{
    ComponentName,
    ReflectComponentUi,
};
use cem_util::egui::EguiUtilUiExt;
use palette::{
    Srgb,
    Srgba,
};
use serde::{
    Deserialize,
    Serialize,
};

/// Tag for entities that should be rendered
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, Component, Reflect)]
#[reflect(Component, ComponentUi, @ComponentName::new("Hidden"), Default)]
pub struct Hidden;

impl PropertiesUi for Hidden {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        ui.noop()
    }
}

// todo: respect eguis theme. we might just pass this in from the view when
// rendering and remove this component.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Component, Reflect)]
#[reflect(Component, ComponentUi, @ComponentName::new("Clear Color"), Default)]
pub struct ClearColor {
    #[serde(with = "crate::util::serde::palette")]
    #[reflect(ignore)]
    pub clear_color: Srgb,
}

impl From<Srgb> for ClearColor {
    fn from(value: Srgb) -> Self {
        Self { clear_color: value }
    }
}

impl From<Srgb<u8>> for ClearColor {
    fn from(value: Srgb<u8>) -> Self {
        Self::from(value.into_format::<f32>())
    }
}

impl PropertiesUi for ClearColor {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        self.clear_color.properties_ui(ui, &())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Component, Reflect)]
#[reflect(Component, ComponentUi, @ComponentName::new("Outline"), Default)]
pub struct Outline {
    #[serde(with = "crate::util::serde::palette")]
    #[reflect(ignore)]
    pub color: Srgba,

    pub thickness: f32,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            color: Srgba::new(1.0, 1.0, 1.0, 0.75),
            thickness: 0.1,
        }
    }
}

impl PropertiesUi for Outline {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(ui, "Color", &mut changes, &mut self.color);
                label_and_value_with_config(
                    ui,
                    "Thickness",
                    &mut changes,
                    &mut self.thickness,
                    &NumericPropertyUiConfig::Slider { range: 0.0..=10.0 },
                );
            })
            .response;

        changes.propagated(response)
    }
}
