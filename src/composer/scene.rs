use std::{
    borrow::Cow,
    fmt::{
        Debug,
        Display,
    },
    marker::PhantomData,
    ops::Deref,
    sync::Arc,
};

use hecs::Entity;
use nalgebra::{
    Isometry3,
    Point3,
    Translation3,
    UnitQuaternion,
    UnitVector3,
    Vector2,
    Vector3,
};
use palette::{
    Srgb,
    Srgba,
    WithAlpha,
};
use parry3d::{
    query::Ray,
    shape::{
        Ball,
        Cuboid,
        HalfSpace,
    },
};
use type_map::concurrent::TypeMap;

use crate::composer::{
    collisions::{
        Collides,
        OctTree,
        RayHit,
    },
    renderer::{
        ClearColor,
        Render,
        camera::{
            CameraConfig,
            CameraProjection,
        },
        grid::GridPlane,
        mesh::{
            SurfaceMesh,
            WindingOrder,
        },
    },
};

#[derive(derive_more::Debug, Default)]
pub struct Scene {
    #[debug("hecs::World {{ ... }}")]
    pub entities: hecs::World,

    pub octtree: OctTree,

    // todo: we don't use this at all right now
    pub resources: TypeMap,
}

impl Scene {
    pub fn add_object(
        &mut self,
        transform: impl Into<Transform>,
        shape: impl Into<SharedShape>,
        color: impl Into<VisualColor>,
    ) -> Entity {
        let shape = shape.into();
        let label = Label::from(format!("object.{:?}", shape.shape_type()));
        self.entities.spawn((
            transform.into(),
            shape,
            color.into(),
            Render,
            label,
            Collides,
        ))
    }

    pub fn add_camera(&mut self, transform: impl Into<Transform>) -> Entity {
        self.entities.spawn((
            transform.into(),
            CameraProjection::default(),
            ClearColor::default(),
            CameraConfig::default(),
            Label::new_static("camera"),
        ))
    }

    pub fn add_grid_plane(
        &mut self,
        transform: impl Into<Transform>,
        line_spacing: Vector2<f32>,
    ) -> Entity {
        self.entities.spawn((
            transform.into(),
            SharedShape::from(HalfSpace::new(Vector3::y_axis())),
            GridPlane { line_spacing },
            Label::new_static("grid-plane"),
        ))
    }

    pub fn cast_ray(
        &self,
        ray: &Ray,
        max_time_of_impact: impl Into<Option<f32>>,
    ) -> Option<RayHit> {
        self.octtree
            .cast_ray(ray, max_time_of_impact, &self.entities)
    }

    pub fn update_octtree(&mut self) {
        self.octtree.update(&mut self.entities);
    }
}

#[derive(Clone, Debug)]
pub struct SharedShape(pub Arc<dyn Shape>);

impl<S: Shape> From<S> for SharedShape {
    fn from(value: S) -> Self {
        Self(Arc::new(value))
    }
}

impl Deref for SharedShape {
    type Target = dyn Shape;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

pub trait Shape: Debug + Send + Sync + parry3d::shape::Shape + 'static {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh>;
}

impl Shape for Ball {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        let (vertices, indices) = self.to_trimesh(10, 20);
        Some(SurfaceMesh {
            vertices,
            indices,
            winding_order: WindingOrder::Clockwise,
        })
    }
}

impl Shape for Cuboid {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        let (vertices, indices) = self.to_trimesh();
        Some(SurfaceMesh {
            vertices,
            indices,
            winding_order: WindingOrder::Clockwise,
        })
    }
}

impl Shape for HalfSpace {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        None
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Transform {
    /// Rotation followed by translation that transforms points from the
    /// object's local frame to the global frame.
    pub transform: Isometry3<f32>,
}

impl Transform {
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
    pub fn pan_tilt(&mut self, pan: f32, tilt: f32, up: &UnitVector3<f32>) {
        let local_up = self.transform.rotation.inverse_transform_unit_vector(&up);
        let local_right = Vector3::x_axis();

        let rotation = UnitQuaternion::from_axis_angle(&local_up, -pan)
            * UnitQuaternion::from_axis_angle(&local_right, tilt);

        self.transform.rotation *= rotation;
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

#[derive(Clone, Copy, Debug)]
pub struct VisualColor {
    pub solid_color: Srgba,
    pub wireframe_color: Srgba,
}

impl From<Srgba> for VisualColor {
    fn from(value: Srgba) -> Self {
        Self {
            solid_color: value,
            wireframe_color: Default::default(),
        }
    }
}

impl From<Srgb> for VisualColor {
    fn from(value: Srgb) -> Self {
        Self::from(value.with_alpha(1.0))
    }
}

impl From<Srgb<u8>> for VisualColor {
    fn from(value: Srgb<u8>) -> Self {
        Self::from(value.into_format::<f32>())
    }
}

pub struct Changed<T> {
    _phantom: PhantomData<T>,
}

impl<T> Default for Changed<T> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<T> Clone for Changed<T> {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl<T> Copy for Changed<T> {}

#[derive(Clone, Debug)]
pub struct Label {
    pub label: Cow<'static, str>,
}

impl Label {
    pub fn new_static(label: &'static str) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

impl From<&str> for Label {
    fn from(value: &str) -> Self {
        Self {
            label: value.to_owned().into(),
        }
    }
}

impl From<String> for Label {
    fn from(value: String) -> Self {
        Self {
            label: value.into(),
        }
    }
}
