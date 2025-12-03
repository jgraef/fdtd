use std::fmt::{
    Debug,
    Display,
};

use bevy_ecs::{
    component::Component,
    entity::Entity,
    query::QueryData,
    world::EntityWorldMut,
};
use cem_scene::{
    Label,
    spatial::Collider,
    transform::LocalTransform,
};

use crate::{
    composer::{
        Selectable,
        tree::ShowInTree,
    },
    renderer::{
        material::Material,
        mesh::{
            IntoGenerateMesh,
            LoadMesh,
        },
    },
};

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

// todo: bevy-migrate: split this up into the crates where the actual components
// are defined
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

#[derive(Clone, Debug, QueryData)]
pub struct EntityDebugLabel {
    pub entity: Entity,
    pub label: Option<&'static Label>,
}

impl Display for EntityDebugLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.entity)?;

        if let Some(label) = &self.label {
            write!(f, ":{}", label.label)?;
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
