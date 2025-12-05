use nalgebra::Vector2;
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GridPlane {
    pub line_spacing: Vector2<f32>,
}
