use std::{
    ops::Index,
    path::PathBuf,
};

use nalgebra::{
    Matrix4,
    UnitVector3,
    Vector2,
};

use crate::{
    app::{
        composer::properties::{
            PropertiesUi,
            TrackChanges,
            label_and_value,
            label_and_value_with_config,
        },
        solver::FieldComponent,
    },
    impl_register_component,
    util::egui::FilePickerConfig,
};

#[derive(Clone, Debug)]
pub struct Observer {
    pub write_to_gif: Option<PathBuf>,
    pub display_as_texture: bool,
    pub field: FieldComponent,
    pub color_map: Matrix4<f32>,
    pub half_extents: Vector2<f32>,
}

impl PropertiesUi for Observer {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                egui::ComboBox::from_id_salt(ui.id().with("field"))
                    .selected_text(FieldNames[self.field])
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.field,
                            FieldComponent::E,
                            FieldNames[FieldComponent::E],
                        );
                        ui.selectable_value(
                            &mut self.field,
                            FieldComponent::H,
                            FieldNames[FieldComponent::H],
                        );
                    });

                label_and_value_with_config(
                    ui,
                    "File",
                    &mut changes,
                    &mut self.write_to_gif,
                    &FilePickerConfig::Save,
                );
                label_and_value(ui, "Live", &mut changes, &mut self.display_as_texture);
            })
            .response;

        changes.propagated(response)
    }
}

impl_register_component!(Observer where ComponentUi);

pub fn test_color_map(scale: f32, axis: UnitVector3<f32>) -> Matrix4<f32> {
    let mut m = Matrix4::zeros();

    // scale axis, add a 0 (affine coordinates), and turn into row-vector
    let x = scale * axis.into_inner().to_homogeneous().transpose();

    // red (row 0) will be positive
    m.set_row(0, &x);

    // blue (row 2) will be negative
    m.set_row(2, &(-x));

    // alpha will be 1
    m[(3, 3)] = 1.0;

    m
}

struct FieldNames;

impl Index<FieldComponent> for FieldNames {
    type Output = &'static str;

    fn index(&self, index: FieldComponent) -> &Self::Output {
        match index {
            FieldComponent::E => &"Electric Field",
            FieldComponent::H => &"Magnetic Field",
        }
    }
}
