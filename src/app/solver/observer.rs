use std::path::PathBuf;

use nalgebra::{
    Matrix4,
    UnitVector3,
};

use crate::app::solver::FieldComponent;

#[derive(Clone, Debug)]
pub struct Observer {
    pub write_to_gif: Option<PathBuf>,
    pub display_as_texture: bool,
    pub field: FieldComponent,
    pub color_map: Matrix4<f32>,
}

pub fn test_color_map(scale: f32, axis: UnitVector3<f32>) -> Matrix4<f32> {
    let mut m = Matrix4::zeros();

    // scale axis, add a 0 (affine coordinates), and turn into row-vector
    let x = scale * axis.into_inner().to_homogeneous().transpose();

    // red (row 0) will be positive
    m.set_row(0, &x);

    // blue (row 2) will be negative
    m.set_row(0, &(-x));

    m
}
