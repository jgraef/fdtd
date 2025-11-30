use bytemuck::{
    Pod,
    Zeroable,
};
use palette::{
    LinSrgba,
    Srgb,
    WithAlpha,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    app::composer::properties::{
        PropertiesUi,
        TrackChanges,
        label_and_value,
    },
    impl_register_component,
};

/// A point light source.
///
/// This defines the color of the light that can be reflected diffusely or
/// specularly.
///
/// Note that we intend to only support one light source at a time, since there
/// isn't much need for more.
///
/// # TODO
///
/// This is currently not used. At the moment we only want one point light
/// specific for a camera, colocated with it. The camera position (and thus the
/// light's) is already sent to the shader. The diffuse and specular light
/// components can be modulated by the camera as well. So there is no need for
/// this right now.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PointLight {
    #[serde(with = "crate::util::serde::palette")]
    pub color: Srgb,
}

impl PointLight {
    pub const fn white_light(intensity: f32) -> Self {
        Self {
            color: Srgb::new(intensity, intensity, intensity),
        }
    }
}

impl Default for PointLight {
    fn default() -> Self {
        Self::white_light(1.0)
    }
}

impl From<Srgb> for PointLight {
    fn from(value: Srgb) -> Self {
        Self { color: value }
    }
}

impl From<Srgb<u8>> for PointLight {
    fn from(value: Srgb<u8>) -> Self {
        Self::from(value.into_format::<f32>())
    }
}

impl PropertiesUi for PointLight {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui: &mut egui::Ui| {
                label_and_value(ui, "Color", &mut changes, &mut self.color);
            })
            .response;

        changes.propagated(response)
    }
}

impl_register_component!(PointLight where ComponentUi, default);

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct AmbientLight {
    #[serde(with = "crate::util::serde::palette")]
    pub color: Srgb,
}

impl AmbientLight {
    pub fn white_light(intensity: f32) -> Self {
        Self {
            color: Srgb::new(intensity, intensity, intensity),
        }
    }
}

impl PropertiesUi for AmbientLight {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(ui, "Color", &mut changes, &mut self.color);
            })
            .response;

        changes.propagated(response)
    }
}

impl_register_component!(AmbientLight where ComponentUi, default);

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub(super) struct PointLightData {
    pub color: LinSrgba,
}

impl PointLightData {
    pub fn new(point_light: &PointLight) -> Self {
        Self {
            color: point_light.color.into_linear().with_alpha(1.0),
        }
    }
}
