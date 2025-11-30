use std::f32::consts::TAU;

use nalgebra::{
    Isometry3,
    Point3,
    Translation3,
    UnitQuaternion,
    Vector3,
};

use crate::composer::properties::{
    PropertiesUi,
    TrackChanges,
    std::DragAngle,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Isometry3UiConfig {
    pub translation: Translation3UiConfig,
    pub rotation: UnitQuaternionUiConfig,
}

impl PropertiesUi for Isometry3<f32> {
    type Config = Isometry3UiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let mut changed = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                changed.track(self.translation.properties_ui(ui, &config.translation));
                changed.track(self.rotation.properties_ui(ui, &config.rotation));
            })
            .response;

        changed.propagated(response)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Translation3UiConfig {
    pub vector: Vector3UiConfig,
}

impl PropertiesUi for Translation3<f32> {
    type Config = Translation3UiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        self.vector.properties_ui(ui, &config.vector)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Vector3UiConfig {
    pub speed: Vector3<f32>,
}

impl Default for Vector3UiConfig {
    fn default() -> Self {
        Self {
            speed: Vector3::repeat(0.1),
        }
    }
}

impl PropertiesUi for Vector3<f32> {
    type Config = Vector3UiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let mut changed = TrackChanges::default();

        let response = ui
            .horizontal(|ui| {
                ui.label("X");
                changed.track(ui.add(egui::DragValue::new(&mut self.x).speed(config.speed.x)));
                ui.label("Y");
                changed.track(ui.add(egui::DragValue::new(&mut self.y).speed(config.speed.y)));
                ui.label("Z");
                changed.track(ui.add(egui::DragValue::new(&mut self.z).speed(config.speed.z)));
            })
            .response;

        changed.propagated(response)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UnitQuaternionUiConfig {
    pub speed: EulerAngles,
}

impl UnitQuaternionUiConfig {
    pub fn from_angular_speed(angular_speed: f32) -> Self {
        Self {
            speed: EulerAngles {
                roll: angular_speed,
                pitch: angular_speed,
                yaw: angular_speed,
            },
        }
    }
}

impl Default for UnitQuaternionUiConfig {
    fn default() -> Self {
        Self::from_angular_speed(TAU / 36.)
    }
}

impl PropertiesUi for UnitQuaternion<f32> {
    type Config = UnitQuaternionUiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let mut euler = EulerAngles::from(*self);
        let mut changed = TrackChanges::default();

        let response = ui
            .horizontal(|ui| {
                ui.label("Roll");
                changed.track(ui.add(DragAngle::new(&mut euler.roll).speed(config.speed.roll)));

                ui.label("Pitch");
                changed.track(ui.add(DragAngle::new(&mut euler.pitch).speed(config.speed.pitch)));

                ui.label("Yaw");
                changed.track(ui.add(DragAngle::new(&mut euler.yaw).speed(config.speed.yaw)));
            })
            .response;

        changed.propagated_and(response, || {
            *self = euler.into();
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EulerAngles {
    pub roll: f32,
    pub pitch: f32,
    pub yaw: f32,
}

impl From<UnitQuaternion<f32>> for EulerAngles {
    fn from(value: UnitQuaternion<f32>) -> Self {
        let (roll, pitch, yaw) = value.euler_angles();
        Self { roll, pitch, yaw }
    }
}

impl From<EulerAngles> for UnitQuaternion<f32> {
    fn from(value: EulerAngles) -> Self {
        Self::from_euler_angles(value.roll, value.pitch, value.yaw)
    }
}

impl PropertiesUi for Point3<f32> {
    type Config = Vector3UiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        self.coords.properties_ui(ui, config)
    }
}
