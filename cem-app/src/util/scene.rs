use std::fmt::{
    Debug,
    Display,
};

use bevy_ecs::{
    component::Component,
    name::Name,
    world::EntityWorldMut,
};
use cem_scene::{
    spatial::Collider,
    transform::LocalTransform,
};

use crate::{
    composer::{
        selection::Selectable,
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
        let name = shape.shape_name().to_owned();
        let collider = Collider::from(shape.clone());
        let mesh = LoadMesh::from_shape(shape, Default::default());

        self.world
            .spawn_empty()
            .name(name)
            .transform(transform)
            .collider(collider)
            .mesh(mesh)
            .tagged::<ShowInTree>(true)
            .tagged::<Selectable>(true)
    }
}

pub trait EntityBuilderExt {
    fn transform(self, transform: impl Into<LocalTransform>) -> Self;
    fn material(self, material: impl Into<Material>) -> Self;
    fn mesh(self, mesh: impl Into<LoadMesh>) -> Self;
    fn collider(self, collider: impl Into<Collider>) -> Self;
    fn name(self, label: impl Display) -> Self;
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

    fn name(mut self, label: impl Display) -> Self {
        self.insert(Name::new(label.to_string()));
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
// todo: bevy-migrate: remove. might add an optional name getter on mesh
// generators
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
