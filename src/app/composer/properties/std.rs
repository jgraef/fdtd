use std::ops::RangeInclusive;

use egui::emath;

use crate::{
    app::composer::properties::PropertiesUi,
    util::Moo,
};

#[derive(Clone, Debug)]
pub enum NumericPropertyUiConfig<T> {
    DragValue { speed: T },
    Slider { range: RangeInclusive<T> },
}

macro_rules! impl_numeric_properties_ui {
    ($ty:ty, $default_speed:expr) => {
        impl Default for NumericPropertyUiConfig<$ty> {
            fn default() -> Self {
                Self::DragValue {
                    speed: $default_speed,
                }
            }
        }

        impl PropertiesUi for $ty {
            type Config = NumericPropertyUiConfig<Self>;

            fn properties_ui(
                &mut self,
                ui: &mut egui::Ui,
                config: &Self::Config,
            ) -> egui::Response {
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
    };
}

impl_numeric_properties_ui!(f32, 0.1);
impl_numeric_properties_ui!(f64, 0.1);

#[derive(Debug)]
pub struct DragAngle<'a, T> {
    pub radians: &'a mut T,
    pub speed: f64,
}

impl<'a, T> DragAngle<'a, T> {
    pub fn new(radians: &'a mut T) -> Self {
        Self {
            radians,
            speed: 1.0,
        }
    }
}

impl<'a, T> DragAngle<'a, T>
where
    f64: From<T>,
{
    pub fn speed(mut self, speed: T) -> Self {
        self.speed = speed.into();
        self
    }
}

impl<'a, T> egui::Widget for DragAngle<'a, T>
where
    T: emath::Numeric,
{
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let mut degrees = self.radians.to_f64().to_degrees();
        let response = ui.add(
            egui::DragValue::new(&mut degrees)
                .speed(self.speed)
                .suffix("°"),
        );

        if response.changed() {
            *self.radians = T::from_f64(degrees.to_radians());
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
