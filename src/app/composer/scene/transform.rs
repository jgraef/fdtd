use nalgebra::{
    Isometry3,
    Point3,
    Translation3,
    UnitQuaternion,
    UnitVector3,
    Vector3,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::app::composer::properties::{
    PropertiesUi,
    nalgebra::Isometry3UiConfig,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Transform {
    /// Rotation followed by translation that transforms points from the
    /// object's local frame to the global frame.
    pub transform: Isometry3<f32>,
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform {
    pub fn identity() -> Self {
        Self {
            transform: Isometry3::identity(),
        }
    }

    pub fn new(
        translation: impl Into<Translation3<f32>>,
        rotation: impl Into<UnitQuaternion<f32>>,
    ) -> Self {
        Self {
            transform: Isometry3::from_parts(translation.into(), rotation.into()),
        }
    }

    pub fn translate_local(&mut self, translation: &Translation3<f32>) {
        self.transform.translation.vector += self
            .transform
            .rotation
            .transform_vector(&translation.vector);
    }

    pub fn translate_global(&mut self, translation: &Translation3<f32>) {
        self.transform.translation.vector += &translation.vector;
    }

    pub fn rotate_local(&mut self, rotation: &UnitQuaternion<f32>) {
        self.transform.rotation *= rotation;
    }

    pub fn rotate_global(&mut self, rotation: &UnitQuaternion<f32>) {
        self.transform.append_rotation_mut(rotation);
    }

    pub fn rotate_around(&mut self, anchor: &Point3<f32>, rotation: &UnitQuaternion<f32>) {
        self.transform
            .append_rotation_wrt_point_mut(rotation, anchor);
    }

    pub fn look_at(eye: &Point3<f32>, target: &Point3<f32>, up: &Vector3<f32>) -> Self {
        Self {
            transform: Isometry3::face_towards(eye, target, up),
        }
    }

    /// Pan and tilt object (e.g. a camera) with a given `up` vector.
    ///
    /// Pan is the horizontal turning. Tilt is the vertical turning.
    pub fn pan_tilt(&mut self, pan: f32, tilt: f32, up: &Vector3<f32>) {
        let local_up =
            UnitVector3::new_normalize(self.transform.rotation.inverse_transform_vector(up));
        let local_right = Vector3::x_axis();

        let rotation = UnitQuaternion::from_axis_angle(&local_up, -pan)
            * UnitQuaternion::from_axis_angle(&local_right, tilt);

        self.transform.rotation *= rotation;
    }

    pub fn position(&self) -> Point3<f32> {
        self.transform.translation.vector.into()
    }
}

impl From<Isometry3<f32>> for Transform {
    fn from(value: Isometry3<f32>) -> Self {
        Self { transform: value }
    }
}

impl From<Translation3<f32>> for Transform {
    fn from(value: Translation3<f32>) -> Self {
        Self::from(Isometry3::from(value))
    }
}

impl From<Vector3<f32>> for Transform {
    fn from(value: Vector3<f32>) -> Self {
        Self::from(Isometry3::from(value))
    }
}

impl From<Point3<f32>> for Transform {
    fn from(value: Point3<f32>) -> Self {
        Self::from(value.coords)
    }
}

impl From<UnitQuaternion<f32>> for Transform {
    fn from(value: UnitQuaternion<f32>) -> Self {
        Self::from(Isometry3::from_parts(Default::default(), value))
    }
}

impl PropertiesUi for Transform {
    type Config = Isometry3UiConfig;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        self.transform.properties_ui(ui, config)
    }
}
