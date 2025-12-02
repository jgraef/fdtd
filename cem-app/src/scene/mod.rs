pub mod buffer;
//pub mod components;
pub mod resources;
//pub mod serialize;
//pub mod spatial;
//pub mod transform;

use std::{
    any::type_name,
    borrow::Cow,
    fmt::{
        Debug,
        Display,
    },
    marker::PhantomData,
};

use bevy_ecs::{
    component::Component,
    world::EntityWorldMut,
};
use cem_scene::spatial::Collider;
pub use cem_scene::transform;
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
        material::Material,
        mesh::{
            IntoGenerateMesh,
            LoadMesh,
        },
    },
    scene::{
        resources::Resources,
        transform::{
            GlobalTransform,
            LocalTransform,
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

    tick: Tick,

    pub resources: Resources,
}

impl Default for Scene {
    fn default() -> Self {
        let mut resources = Resources::default();

        // this and their calls to update should be handled as plugins that register
        // resources and systems.
        //resources.insert(SpatialQueries::default());
        //resources.insert(TransformHierarchyUpdater::default());

        // not sure whether this should be a resource.
        //let mut component_registry = ComponentRegistry::default();
        //component_registry.register_builtin();
        //resources.insert(component_registry);

        Self {
            entities: Default::default(),
            command_buffer: Default::default(),
            tick: Tick { tick: 0 },
            resources,
        }
    }
}

pub trait SceneExt {
    fn add_object<S>(
        &mut self,
        transform: impl Into<LocalTransform>,
        shape: S,
    ) -> EntityWorldMut<'_>
    where
        S: ShapeName + Clone + IntoGenerateMesh,
        Collider: From<S>,
        S::Config: Default,
        S::GenerateMesh: Debug + Send + Sync + 'static;
}

impl SceneExt for cem_scene::Scene {
    fn add_object<S>(
        &mut self,
        transform: impl Into<LocalTransform>,
        shape: S,
    ) -> EntityWorldMut<'_>
    where
        S: ShapeName + Clone + IntoGenerateMesh,
        Collider: From<S>,
        S::Config: Default,
        S::GenerateMesh: Debug + Send + Sync + 'static,
    {
        let label = format!("object.{}", shape.shape_name());
        let collider = Collider::from(shape.clone());
        let mesh = LoadMesh::from_shape(shape, Default::default());

        self.world
            .spawn_empty()
            .label(label)
            .transform(transform)
            .collider(collider)
            .mesh(mesh)
            .tagged::<ShowInTree>(true)
            .tagged::<Selectable>(true)
    }
}

impl Scene {
    /*pub fn add_object<S>(&mut self, transform: impl Into<LocalTransform>, shape: S) -> EntityBuilder
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
    }*/

    /// This needs to be called every frame to update internal state.
    ///
    /// E.g. this updates the internal octtree used for spatial queries, and the
    /// transform hierarrchy
    pub fn prepare(&mut self) {
        self.apply_deferred();

        self.tick.tick += 1;

        //self.resources
        //    .expect_mut::<TransformHierarchyUpdater>()
        //    .update(&mut self.entities, &mut self.command_buffer);

        //self.resources
        //    .expect_mut::<SpatialQueries>()
        //    .update(&mut self.entities, &mut self.command_buffer);

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

    /*pub fn take(&mut self, entity: hecs::Entity) -> Option<hecs::TakenEntity<'_>> {
        self.resources.expect_mut::<SpatialQueries>().remove(
            entity,
            &mut self.entities,
            &mut self.command_buffer,
        );
        self.entities.take(entity).ok()
    }*/

    /*pub fn serialize(&self, entity: hecs::Entity) -> Option<SerializeEntity<'_>> {
        self.entities.entity(entity).ok().map(SerializeEntity::new)
    }*/

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

#[derive(Clone, Debug, Serialize, Deserialize, Component)]
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

    fn populate_scene(&self, scene: &mut cem_scene::Scene) -> Result<(), Self::Error>;
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

pub trait EntityBuilderExt {
    fn transform(self, transform: impl Into<LocalTransform>) -> Self;
    fn material(self, material: impl Into<Material>) -> Self;
    fn mesh(self, mesh: impl Into<LoadMesh>) -> Self;
    fn collider(self, collider: impl Into<Collider>) -> Self;
    fn label(self, label: impl Display) -> Self;
    fn tagged<T>(self, on: bool) -> Self
    where
        T: Default + Component;
}

impl<'a> EntityBuilderExt for EntityWorldMut<'a> {
    fn transform(mut self, transform: impl Into<LocalTransform>) -> Self {
        self.insert(transform.into());
        self
    }

    fn material(mut self, material: impl Into<Material>) -> Self {
        self.insert(material.into());
        self
    }

    fn mesh(mut self, mesh: impl Into<LoadMesh>) -> Self {
        self.insert(mesh.into());
        self
    }

    fn collider(mut self, collider: impl Into<Collider>) -> Self {
        self.insert(collider.into());
        self
    }

    fn label(mut self, label: impl Display) -> Self {
        self.insert(Label::new(label));
        self
    }

    fn tagged<T>(mut self, on: bool) -> Self
    where
        T: Default + Component,
    {
        if on {
            self.insert(T::default());
        }
        self
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

        //self.resources.expect::<SpatialQueries>().show_debug(ui);
    }
}

// todo bevy-migration remove this
#[macro_export]
macro_rules! impl_register_component {
    ($($tt:tt)*) => {
        // nop
    };
}
