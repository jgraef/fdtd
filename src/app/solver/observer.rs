use std::path::PathBuf;

use nalgebra::Matrix4;

use crate::app::solver::FieldComponent;

#[derive(Clone, Debug)]
pub struct Observer {
    pub write_to_gif: Option<PathBuf>,
    pub display_as_texture: bool,
    pub field: FieldComponent,
    pub color_map: Matrix4<f32>,
}
