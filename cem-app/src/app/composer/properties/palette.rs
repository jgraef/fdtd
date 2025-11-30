use palette::{
    LinSrgb,
    LinSrgba,
    rgb::{
        Srgb,
        Srgba,
    },
};

use crate::app::composer::properties::PropertiesUi;

impl PropertiesUi for Srgba {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        ui.horizontal(|ui| {
            let mut color = srgba_to_egui(*self);

            let response = egui::widgets::color_picker::color_edit_button_rgba(
                ui,
                &mut color,
                egui::widgets::color_picker::Alpha::OnlyBlend,
            );

            ui.code(format!(
                "[{:.02}, {:.02}, {:.02}, {:.02}]",
                self.red, self.green, self.blue, self.alpha
            ));

            if response.changed() {
                *self = egui_to_rgba(color);
            }

            response
        })
        .inner
    }
}

impl PropertiesUi for Srgb {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        ui.horizontal(|ui| {
            let mut color = srgb_to_egui(*self);

            let response = egui::widgets::color_picker::color_edit_button_rgb(ui, &mut color);

            ui.code(format!(
                "[{:.02}, {:.02}, {:.02}]",
                self.red, self.green, self.blue
            ));

            if response.changed() {
                *self = egui_to_rgb(color);
            }

            response
        })
        .inner
    }
}

fn srgba_to_egui(color: Srgba) -> egui::Rgba {
    let color = color.into_linear();
    egui::Rgba::from_rgba_unmultiplied(color.red, color.green, color.blue, color.alpha)
}

fn egui_to_rgba(color: egui::Rgba) -> Srgba {
    let color = color.to_rgba_unmultiplied();
    Srgba::from_linear(LinSrgba::from(color))
}

fn srgb_to_egui(color: Srgb) -> [f32; 3] {
    let color = color.into_linear();
    color.into()
}

fn egui_to_rgb(color: [f32; 3]) -> Srgb {
    Srgb::from_linear(LinSrgb::from(color))
}
