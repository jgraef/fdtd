use palette::{
    rgb::{
        Srgb,
        Srgba,
    },
    stimulus::IntoStimulus,
};

use crate::app::composer::properties::PropertiesUi;

impl<T> PropertiesUi for Srgba<T>
where
    T: IntoStimulus<u8> + Copy,
    u8: IntoStimulus<T>,
{
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut color32 = rgba_to_egui(*self);

        let response = egui::widgets::color_picker::color_edit_button_srgba(
            ui,
            &mut color32,
            egui::widgets::color_picker::Alpha::OnlyBlend,
        );

        if response.changed() {
            *self = egui_to_rgba(color32);
        }

        response
    }
}

impl<T> PropertiesUi for Srgb<T>
where
    T: IntoStimulus<u8> + Copy,
    u8: IntoStimulus<T>,
{
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut color32 = rgb_to_egui(*self);

        let response = egui::widgets::color_picker::color_edit_button_srgb(ui, &mut color32);

        if response.changed() {
            *self = egui_to_rgb(color32);
        }

        response
    }
}

fn rgba_to_egui<T>(color: Srgba<T>) -> egui::Color32
where
    T: IntoStimulus<u8> + Copy,
{
    let color_u8 = color.into_format::<u8, u8>();
    egui::Color32::from_rgba_premultiplied(
        color_u8.red,
        color_u8.green,
        color_u8.blue,
        color_u8.alpha,
    )
}

fn egui_to_rgba<T>(color: egui::Color32) -> Srgba<T>
where
    u8: IntoStimulus<T>,
{
    Srgba::new(color.r(), color.g(), color.b(), color.a()).into_format::<T, T>()
}

fn rgb_to_egui<T>(color: Srgb<T>) -> [u8; 3]
where
    T: IntoStimulus<u8> + Copy,
{
    let color_u8 = color.into_format::<u8>();
    color_u8.into()
}

fn egui_to_rgb<T>(color: [u8; 3]) -> Srgb<T>
where
    u8: IntoStimulus<T>,
{
    Srgb::from(color).into_format()
}
