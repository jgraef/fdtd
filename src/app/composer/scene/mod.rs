pub mod collisions;
pub mod serialize;
pub mod shape;
pub mod transform;
pub mod ui;
pub mod undo;

use std::{
    borrow::Cow,
    fmt::{
        Debug,
        Display,
    },
    marker::PhantomData,
};

use nalgebra::{
    Isometry3,
    Point3,
    Vector2,
    Vector3,
};
use parry3d::{
    bounding_volume::Aabb,
    query::{
        Contact,
        Ray,
    },
    shape::{
        HalfSpace,
        Shape,
    },
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::app::composer::{
    Selectable,
    renderer::{
        Render,
        grid::GridPlane,
        light::Material,
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
        shape::SharedShape,
        transform::Transform,
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

    /// General-purpose command buffer.
    ///
    /// This can be used to defer modifications temporarily, wihout the need to
    /// allocate your own command buffer.
    ///
    /// You should run the command buffer on the world yourself when you want
    /// the changes become visible. Otherwise it's run in [`Self::prepare`].
    #[debug("hecs::CommandBuffer {{ ... }}")]
    pub command_buffer: hecs::CommandBuffer,
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
            Selectable,
            // todo: added for testing for now.
            crate::physics::material::Material {
                relative_permittivity: 3.9,
                ..crate::physics::material::Material::VACUUM
            },
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
        filter: impl Fn(hecs::Entity) -> bool,
    ) -> Option<RayHit> {
        self.octtree
            .cast_ray(ray, max_time_of_impact, &self.entities, filter)
    }

    pub fn point_query(&self, point: &Point3<f32>) -> impl Iterator<Item = hecs::Entity> {
        self.octtree.point_query(*point, &self.entities)
    }

    pub fn contact_query(
        &self,
        shape: &dyn Shape,
        transform: &Isometry3<f32>,
    ) -> impl Iterator<Item = (hecs::Entity, Contact)> {
        self.octtree.contact_query(shape, transform, &self.entities)
    }

    /// This needs to be called every frame to update internal state.
    ///
    /// E.g. this updates the internal octtree used for spatial queries
    pub fn prepare(&mut self) {
        self.apply_deferred();
        self.octtree
            .update(&mut self.entities, &mut self.command_buffer);

        // todo: who is responsible for this?
        // the octtree is definitely interested in these, but maybe other's as well?
        // there are definitely other things that are marked with `Changed<_>` (i think
        // in the renderer, but they clean that up).
        for (entity, ()) in self
            .entities
            .query_mut::<()>()
            .with::<&Changed<Transform>>()
        {
            self.command_buffer.remove_one::<Changed<Transform>>(entity);
        }
        self.apply_deferred();
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

    pub fn apply_deferred(&mut self) {
        self.command_buffer.run_on(&mut self.entities);
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
