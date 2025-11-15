use std::ops::RangeInclusive;

use crate::{
    app::composer::properties::PropertiesUi,
    util::Moo,
};

#[derive(Clone, Debug)]
pub enum NumericPropertyUiConfig {
    DragValue { speed: f32 },
    Slider { range: RangeInclusive<f32> },
}

impl Default for NumericPropertyUiConfig {
    fn default() -> Self {
        Self::DragValue { speed: 0.1 }
    }
}

impl PropertiesUi for f32 {
    type Config = NumericPropertyUiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        match config {
            NumericPropertyUiConfig::DragValue { speed } => {
                ui.add(egui::DragValue::new(self).speed(*speed))
            }
            NumericPropertyUiConfig::Slider { range } => {
                ui.add(egui::Slider::new(self, range.clone()))
            }
        }
    }
}

#[derive(Debug)]
pub struct DragAngle<'a> {
    pub radians: &'a mut f32,
    pub speed: f32,
}

impl<'a> DragAngle<'a> {
    pub fn new(radians: &'a mut f32) -> Self {
        Self {
            radians,
            speed: 1.0,
        }
    }

    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }
}

impl<'a> egui::Widget for DragAngle<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let mut degrees = self.radians.to_degrees();
        let response = ui.add(
            egui::DragValue::new(&mut degrees)
                .speed(self.speed)
                .suffix("°"),
        );

        if response.changed() {
            *self.radians = degrees.to_radians();
        }

        response
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TextPropertiesUiConfig {
    pub multiline: bool,
}

impl PropertiesUi for String {
    type Config = TextPropertiesUiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        if config.multiline {
            ui.text_edit_multiline(self)
        }
        else {
            ui.text_edit_singleline(self)
        }
    }
}

impl PropertiesUi for Option<String> {
    type Config = TextPropertiesUiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let mut moo_string: Moo<String> = self
            .as_mut()
            .map_or_else(|| String::new().into(), Into::into);

        let response = moo_string.properties_ui(ui, config);

        if moo_string.is_empty() {
            *self = None;
        }
        else if let Moo::Owned(string) = moo_string {
            *self = Some(string);
        }

        response
    }
}
