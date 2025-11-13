pub mod collisions;
pub mod serialize;
pub mod undo;

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

use nalgebra::{
    Isometry3,
    Point3,
    Translation3,
    UnitQuaternion,
    Vector2,
    Vector3,
};
use parry3d::{
    bounding_volume::Aabb,
    math::UnitVector,
    query::Ray,
    shape::{
        Ball,
        Cuboid,
        Cylinder,
        HalfSpace,
        TriMesh,
    },
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::app::composer::{
    renderer::{
        Render,
        grid::GridPlane,
        light::Material,
        mesh::{
            SurfaceMesh,
            WindingOrder,
        },
    },
    scene::{
        collisions::{
            BoundingBox,
            Collides,
            OctTree,
            RayHit,
            merge_aabbs,
        },
        serialize::SerializeEntity,
    },
    tree::ShowInTree,
};

/// # TODO
///
/// - The `add_*` methods could be bundles. But I don't think bundles support
///   optional components. We could make our own trait that inserts an entity
///   (which would be identical to `PopulateScene`).
#[derive(derive_more::Debug, Default)]
pub struct Scene {
    #[debug("hecs::World {{ ... }}")]
    pub entities: hecs::World,

    pub octtree: OctTree,
}

impl Scene {
    pub fn add_object(
        &mut self,
        transform: impl Into<Transform>,
        shape: impl Into<SharedShape>,
        material: impl Into<Material>,
    ) -> hecs::Entity {
        let shape = shape.into();
        let label = Label::from(format!("object.{:?}", shape.shape_type()));
        self.entities.spawn((
            transform.into(),
            shape,
            material.into(),
            Render,
            label,
            Collides,
            ShowInTree,
        ))
    }

    pub fn add_grid_plane(
        &mut self,
        transform: impl Into<Transform>,
        line_spacing: Vector2<f32>,
    ) -> hecs::Entity {
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

    /// This needs to be called every frame to update internal state.
    ///
    /// E.g. this updates the internal octtree used for spatial queries
    pub fn prepare(&mut self) {
        self.octtree.update(&mut self.entities);
    }

    /// Computes the scene's AABB relative to an observer.
    ///
    /// # Arguments
    /// - `relative_to`: The individual AABBs of objects in the scene will be
    ///   relative to this, i.e. they wll be transformed by its inverse.
    /// - `approximate_relative_aabbs`: Compute the individual AABBs by
    ///   transforming the pre-computed AABB
    pub fn compute_aabb_relative_to_observer(
        &self,
        relative_to: &Transform,
        approximate_relative_aabbs: bool,
    ) -> Option<Aabb> {
        let relative_to_inv = relative_to.transform.inverse();

        if approximate_relative_aabbs {
            let mut query = self.entities.query::<&BoundingBox>();
            let aabbs = query
                .iter()
                .map(|(_entity, bounding_box)| bounding_box.aabb.transform_by(&relative_to_inv));
            merge_aabbs(aabbs)
        }
        else {
            let mut query = self.entities.query::<(&Transform, &SharedShape)>();
            let aabbs = query.iter().map(|(_entity, (transform, shape))| {
                let transform = relative_to_inv * transform.transform;
                shape.compute_aabb(&transform)
            });
            merge_aabbs(aabbs)
        }
    }

    pub fn entity_debug_label(&self, entity: hecs::Entity) -> EntityDebugLabel {
        let exists = self.entities.contains(entity);

        let label = exists
            .then(|| {
                self.entities
                    .query_one::<Option<&Label>>(entity)
                    .ok()
                    .and_then(|mut query| query.get().flatten().cloned())
            })
            .flatten();

        EntityDebugLabel {
            entity,
            label,
            invalid: !exists,
        }
    }

    pub fn delete(&mut self, entity: hecs::Entity) -> Option<hecs::TakenEntity<'_>> {
        self.octtree.remove(entity, &mut self.entities);
        self.entities.take(entity).ok()
    }

    pub fn serialize(&self, entity: hecs::Entity) -> Option<SerializeEntity<'_>> {
        self.entities.entity(entity).ok().map(SerializeEntity::new)
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

impl Serialize for SharedShape {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.as_typed_shape().serialize(serializer)
    }
}

// todo: add a method to use parry's to_outline methods. these generate outlines
// that are probably nicer for our "wiremesh" rendering
//
// we also need a way to control parameters for the to_trimesh functions. i
// think we should put a config with these parameters into the renderer, and
// it'll pass the whole config to the trait method. the trait impls can then
// choose what to use.
pub trait Shape: Debug + Send + Sync + parry3d::shape::Shape + 'static {
    /// Generate surface mesh.
    ///
    /// At the moment this is only used for rendering. If an entity has the
    /// [`Render`] tag and a [`SharedShape`], the renderer will generate a mesh
    /// for it and send it to the GPU. If this method returns `None` though, the
    /// [`Render`] tag will be removed.
    fn to_surface_mesh(&self) -> Option<SurfaceMesh>;
}

/// according to the [documentation][1] the tri mesh should be wound
/// counter-clockwise.
///
/// [1]: https://docs.rs/parry3d/latest/parry3d/shape/struct.TriMesh.html#method.new
pub const PARRY_WINDING_ORDER: WindingOrder = WindingOrder::CounterClockwise;

impl Shape for Ball {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        let (vertices, indices) = self.to_trimesh(20, 20);
        Some(SurfaceMesh {
            vertices,
            indices,
            winding_order: PARRY_WINDING_ORDER,
        })
    }
}

impl Shape for Cuboid {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        let (vertices, indices) = self.to_trimesh();
        Some(SurfaceMesh {
            vertices,
            indices,
            winding_order: PARRY_WINDING_ORDER,
        })
    }
}

impl Shape for HalfSpace {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        None
    }
}

impl Shape for Cylinder {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        let (vertices, indices) = self.to_trimesh(20);
        Some(SurfaceMesh {
            vertices,
            indices,
            winding_order: PARRY_WINDING_ORDER,
        })
    }
}

impl Shape for TriMesh {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        Some(SurfaceMesh {
            vertices: self.vertices().to_owned(),
            indices: self.indices().to_owned(),
            winding_order: PARRY_WINDING_ORDER,
        })
    }
}

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
            UnitVector::new_normalize(self.transform.rotation.inverse_transform_vector(up));
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
        *self
    }
}

impl<T> Copy for Changed<T> {}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

impl From<&Label> for egui::WidgetText {
    fn from(value: &Label) -> Self {
        egui::WidgetText::Text(value.label.as_ref().to_owned())
    }
}

impl From<Label> for egui::WidgetText {
    fn from(value: Label) -> Self {
        egui::WidgetText::Text(value.label.as_ref().to_owned())
    }
}

pub trait PopulateScene {
    type Error;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error>;
}

#[derive(Clone, Debug)]
pub struct EntityDebugLabel {
    pub entity: hecs::Entity,
    pub label: Option<Label>,
    pub invalid: bool,
}

impl Display for EntityDebugLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.entity)?;

        if let Some(label) = &self.label {
            write!(f, ":{}", label.label)?;
        }

        if self.invalid {
            write!(f, ":invalid-ref")?;
        }

        Ok(())
    }
}

impl From<&EntityDebugLabel> for egui::WidgetText {
    fn from(value: &EntityDebugLabel) -> Self {
        egui::WidgetText::Text(value.to_string())
    }
}

impl From<EntityDebugLabel> for egui::WidgetText {
    fn from(value: EntityDebugLabel) -> Self {
        egui::WidgetText::Text(value.to_string())
    }
}
