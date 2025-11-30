pub mod buffer;
pub mod components;
pub mod serialize;
pub mod spatial;
pub mod transform;

use std::{
    any::type_name,
    borrow::Cow,
    fmt::{
        Debug,
        Display,
    },
    marker::PhantomData,
    ops::{
        Deref,
        DerefMut,
    },
};

use nalgebra::{
    Isometry3,
    Point3,
    Vector2,
};
use parry3d::{
    bounding_volume::Aabb,
    query::Ray,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    composer::{
        Selectable,
        tree::ShowInTree,
    },
    debug::DebugUi,
    renderer::{
        grid::GridPlane,
        material::Material,
        mesh::{
            IntoGenerateMesh,
            LoadMesh,
        },
    },
    scene::{
        components::ComponentRegistry,
        serialize::SerializeEntity,
        spatial::{
            Collider,
            RayHit,
            SpatialQueries,
            merge_aabbs,
        },
        transform::{
            GlobalTransform,
            LocalTransform,
            TransformHierarchyUpdater,
        },
    },
};

/// # TODO
///
/// - The `add_*` methods could be bundles. But I don't think bundles support
///   optional components. We could make our own trait that inserts an entity
///   (which would be identical to `PopulateScene`).
#[derive(derive_more::Debug)]
pub struct Scene {
    #[debug("hecs::World {{ ... }}")]
    pub entities: hecs::World,

    /// General-purpose command buffer.
    ///
    /// This can be used to defer modifications temporarily, wihout the need to
    /// allocate your own command buffer.
    ///
    /// You should run the command buffer on the world yourself when you want
    /// the changes become visible. Otherwise it's run in [`Self::prepare`].
    #[debug("hecs::CommandBuffer {{ ... }}")]
    pub command_buffer: hecs::CommandBuffer,

    pub spatial_queries: SpatialQueries,

    tick: Tick,

    transform_hierarchy_updater: TransformHierarchyUpdater,

    pub component_registry: ComponentRegistry,
}

impl Default for Scene {
    fn default() -> Self {
        let mut component_registry = ComponentRegistry::default();
        component_registry.register_builtin();

        Self {
            entities: Default::default(),
            command_buffer: Default::default(),
            spatial_queries: Default::default(),
            tick: Tick { tick: 0 },
            transform_hierarchy_updater: TransformHierarchyUpdater::default(),
            component_registry,
        }
    }
}

impl Scene {
    pub fn add_object<S>(&mut self, transform: impl Into<LocalTransform>, shape: S) -> EntityBuilder
    where
        S: ShapeName + Clone + IntoGenerateMesh,
        Collider: From<S>,
        S::Config: Default,
        S::GenerateMesh: Debug + Send + Sync + 'static,
    {
        let builder = EntityBuilder::default();

        let label = format!("object.{}", shape.shape_name());
        let collider = Collider::from(shape.clone());
        let mesh = LoadMesh::from_shape(shape, Default::default());

        builder
            .label(label)
            .transform(transform)
            .collider(collider)
            .mesh(mesh)
            .tagged::<ShowInTree>(true)
            .tagged::<Selectable>(true)
    }

    pub fn add_grid_plane(
        &mut self,
        transform: impl Into<LocalTransform>,
        line_spacing: Vector2<f32>,
    ) -> hecs::Entity {
        self.entities.spawn((
            transform.into(),
            //MeshFromShape::from(HalfSpace::new(Vector3::y_axis())),
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
        self.spatial_queries
            .cast_ray(ray, max_time_of_impact, &self.entities, filter)
    }

    pub fn point_query(&self, point: &Point3<f32>) -> impl Iterator<Item = hecs::Entity> {
        self.spatial_queries.point_query(*point, &self.entities)
    }

    /*pub fn contact_query(
        &self,
        shape: &dyn Shape,
        transform: &Isometry3<f32>,
    ) -> impl Iterator<Item = (hecs::Entity, Contact)> {
        self.spatial_queries
            .contact_query(shape, transform, &self.entities)
    }*/

    /// This needs to be called every frame to update internal state.
    ///
    /// E.g. this updates the internal octtree used for spatial queries
    pub fn prepare(&mut self) {
        self.apply_deferred();

        self.tick.tick += 1;

        self.transform_hierarchy_updater
            .update(&mut self.entities, &mut self.command_buffer);

        self.spatial_queries
            .update(&mut self.entities, &mut self.command_buffer);

        // todo: who is responsible for this?
        // the octtree is definitely interested in these, but maybe other's as well?
        // there are definitely other things that are marked with `Changed<_>` (i think
        // in the renderer, but they clean that up).
        for (entity, ()) in self
            .entities
            .query_mut::<()>()
            .with::<&Changed<GlobalTransform>>()
        {
            self.command_buffer
                .remove_one::<Changed<GlobalTransform>>(entity);
        }
        self.apply_deferred();
    }

    pub fn aabb(&self) -> Aabb {
        self.spatial_queries.root_aabb()
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
        relative_to: &Isometry3<f32>,
        approximate_relative_aabbs: bool,
    ) -> Option<Aabb> {
        let relative_to_inv = relative_to.inverse();

        if approximate_relative_aabbs {
            let mut query = self.entities.query::<&Aabb>();
            let aabbs = query
                .iter()
                .map(|(_entity, aabb)| aabb.transform_by(&relative_to_inv));
            merge_aabbs(aabbs)
        }
        else {
            let mut query = self.entities.query::<(&GlobalTransform, &Collider)>();
            let aabbs = query.iter().map(|(_entity, (transform, collider))| {
                let transform = relative_to_inv * transform.isometry();
                collider.compute_aabb(&transform)
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
        self.spatial_queries
            .remove(entity, &mut self.entities, &mut self.command_buffer);
        self.entities.take(entity).ok()
    }

    pub fn serialize(&self, entity: hecs::Entity) -> Option<SerializeEntity<'_>> {
        self.entities.entity(entity).ok().map(SerializeEntity::new)
    }

    pub fn apply_deferred(&mut self) {
        self.command_buffer.run_on(&mut self.entities);
    }

    pub fn tick(&self) -> Tick {
        self.tick
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    derive_more::Display,
)]
#[display("{tick}")]
pub struct Tick {
    tick: u64,
}

impl Tick {
    #[cfg(test)]
    pub fn new_test(tick: u64) -> Self {
        Self { tick }
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

impl<T> Debug for Changed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Changed<{}>", type_name::<T>())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Label {
    pub label: Cow<'static, str>,
}

impl Label {
    pub fn new(label: impl Display) -> Self {
        Self {
            label: label.to_string().into(),
        }
    }

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

#[must_use]
#[derive(derive_more::Debug, Default)]
pub struct EntityBuilder {
    #[debug("hecs::EntityBuilder {{ ... }}")]
    builder: hecs::EntityBuilder,
}

impl EntityBuilder {
    pub fn component<T>(mut self, component: T) -> Self
    where
        T: hecs::Component,
    {
        self.builder.add(component);
        self
    }

    pub fn bundle<B>(mut self, bundle: B) -> Self
    where
        B: hecs::DynamicBundle,
    {
        self.builder.add_bundle(bundle);
        self
    }

    pub fn transform(mut self, transform: impl Into<LocalTransform>) -> Self {
        self.builder.add(transform.into());
        self
    }

    pub fn material(mut self, material: impl Into<Material>) -> Self {
        self.builder.add(material.into());
        self
    }

    pub fn mesh(mut self, mesh: impl Into<LoadMesh>) -> Self {
        self.builder.add(mesh.into());
        self
    }

    pub fn collider(mut self, collider: impl Into<Collider>) -> Self {
        self.builder.add(collider.into());
        self
    }

    pub fn label(mut self, label: impl Display) -> Self {
        self.builder.add(Label::new(label));
        self
    }

    pub fn tagged<T>(mut self, on: bool) -> Self
    where
        T: Default + hecs::Component,
    {
        if on {
            self.builder.add(T::default());
        }
        self
    }
}

#[derive(Clone, Debug)]
pub struct SpawnOnDrop<E, W>
where
    E: Spawn,
    W: Spawner,
{
    entity: Option<E>,
    spawner: W,
}

impl<E, W> SpawnOnDrop<E, W>
where
    E: Spawn,
    W: Spawner,
{
    pub fn new(entity: E, world: W) -> Self {
        Self {
            entity: Some(entity),
            spawner: world,
        }
    }
}

impl<E, W> Deref for SpawnOnDrop<E, W>
where
    E: Spawn,
    W: Spawner,
{
    type Target = E;

    fn deref(&self) -> &Self::Target {
        self.entity.as_ref().unwrap()
    }
}

impl<E, W> DerefMut for SpawnOnDrop<E, W>
where
    E: Spawn,
    W: Spawner,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.entity.as_mut().unwrap()
    }
}

impl<E, W> AsRef<E> for SpawnOnDrop<E, W>
where
    E: Spawn,
    W: Spawner,
{
    fn as_ref(&self) -> &E {
        self
    }
}

impl<E, W> AsMut<E> for SpawnOnDrop<E, W>
where
    E: Spawn,
    W: Spawner,
{
    fn as_mut(&mut self) -> &mut E {
        &mut *self
    }
}

impl<E, W> Drop for SpawnOnDrop<E, W>
where
    E: Spawn,
    W: Spawner,
{
    fn drop(&mut self) {
        self.entity.take().unwrap().spawn(&mut self.spawner);
    }
}

pub trait Spawn {
    fn spawn<S>(self, spawner: &mut S) -> S::Output
    where
        S: Spawner;

    fn spawn_on_drop<S>(self, spawner: S) -> SpawnOnDrop<Self, S>
    where
        S: Spawner,
        Self: Sized,
    {
        SpawnOnDrop::new(self, spawner)
    }
}

impl Spawn for hecs::EntityBuilder {
    fn spawn<S>(mut self, spawner: &mut S) -> S::Output
    where
        S: Spawner,
    {
        spawner.spawn(self.build())
    }
}

impl Spawn for EntityBuilder {
    fn spawn<S>(mut self, spawner: &mut S) -> S::Output
    where
        S: Spawner,
    {
        spawner.spawn(self.builder.build())
    }
}

pub trait Spawner {
    type Output;

    fn spawn<B>(&mut self, bundle: B) -> Self::Output
    where
        B: hecs::DynamicBundle;
}

impl Spawner for hecs::World {
    type Output = hecs::Entity;

    fn spawn<B>(&mut self, bundle: B) -> Self::Output
    where
        B: hecs::DynamicBundle,
    {
        hecs::World::spawn(self, bundle)
    }
}

impl Spawner for hecs::CommandBuffer {
    type Output = ();

    fn spawn<B>(&mut self, bundle: B) -> Self::Output
    where
        B: hecs::DynamicBundle,
    {
        hecs::CommandBuffer::spawn(self, bundle);
    }
}

impl Spawner for Scene {
    type Output = hecs::Entity;
    fn spawn<B>(&mut self, bundle: B) -> Self::Output
    where
        B: hecs::DynamicBundle,
    {
        self.entities.spawn(bundle)
    }
}

// todo: implement a proper way of naming things and remove this
pub trait ShapeName {
    fn shape_name(&self) -> &str;
}

mod shape_names {
    use parry3d::shape::*;

    use crate::composer::shape::flat::Quad;

    macro_rules! shape_name {
        {$($ty:ty,)*} => {
            $(
                impl super::ShapeName for $ty {
                    fn shape_name(&self) -> &str {
                        stringify!($ty)
                    }
                }
            )*
        };
    }

    shape_name! {
        Ball,
        Cuboid,
        Cylinder,
        Quad,
    }
}

impl DebugUi for Scene {
    fn show_debug(&self, ui: &mut egui::Ui) {
        ui.label(format!("Tick: {}", self.tick));
        ui.label(format!("Entities: {}", self.entities.len()));

        self.spatial_queries.show_debug(ui);
    }
}
