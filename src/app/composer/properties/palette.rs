use palette::stimulus::IntoStimulus;

use crate::app::composer::properties::PropertiesUi;

impl<C> PropertiesUi for C
where
    C: EguiPaletteConversions,
{
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut color32 = self.palette_to_egui();

        let response = egui::widgets::color_picker::color_edit_button_srgba(
            ui,
            &mut color32,
            egui::widgets::color_picker::Alpha::OnlyBlend,
        );

        if response.changed() {
            *self = C::egui_to_palette(color32);
        }

        response
    }
}

pub trait EguiPaletteConversions {
    fn palette_to_egui(&self) -> egui::Color32;
    fn egui_to_palette(color: egui::Color32) -> Self;
}

impl<S, T> EguiPaletteConversions for palette::rgb::Rgba<S, T>
where
    T: IntoStimulus<u8> + Copy,
    u8: IntoStimulus<T>,
{
    fn palette_to_egui(&self) -> egui::Color32 {
        let color_u8 = self.into_format::<u8, u8>();
        egui::Color32::from_rgba_premultiplied(
            color_u8.red,
            color_u8.green,
            color_u8.blue,
            color_u8.alpha,
        )
    }

    fn egui_to_palette(color: egui::Color32) -> Self {
        palette::rgb::Rgba::new(color.r(), color.g(), color.b(), color.a()).into_format::<T, T>()
    }
}

impl<S, T> EguiPaletteConversions for palette::rgb::Rgb<S, T>
where
    T: IntoStimulus<u8> + Copy,
    u8: IntoStimulus<T>,
{
    fn palette_to_egui(&self) -> egui::Color32 {
        let color_u8 = self.into_format::<u8>();
        egui::Color32::from_rgb(color_u8.red, color_u8.green, color_u8.blue)
    }

    fn egui_to_palette(color: egui::Color32) -> Self {
        palette::rgb::Rgb::new(color.r(), color.g(), color.b()).into_format::<T>()
    }
}
