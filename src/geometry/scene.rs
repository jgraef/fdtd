use std::{
    fmt::Debug,
    sync::Arc,
};

use hecs::Entity;
use nalgebra::{
    Isometry3,
    Point3,
    Translation3,
    Vector3,
};
use palette::{
    Srgb,
    Srgba,
    WithAlpha,
};
use parking_lot::{
    RwLock,
    RwLockReadGuard,
    RwLockWriteGuard,
};
use parry3d::shape::{
    Ball,
    Cuboid,
};
use type_map::concurrent::TypeMap;

use crate::ui::{
    Camera,
    Render,
};

#[derive(derive_more::Debug, Default)]
pub struct World {
    #[debug("hecs::World {{ ... }}")]
    pub entities: hecs::World,
    pub resources: TypeMap,
}

impl World {
    pub fn add_object(
        &mut self,
        transform: impl Into<Transform>,
        shape: impl Into<SharedShape>,
        color: impl Into<VisualColor>,
    ) -> Entity {
        self.entities
            .spawn((transform.into(), shape.into(), color.into(), Render))
    }

    pub fn add_camera(&mut self, transform: impl Into<Transform>) -> Entity {
        self.entities.spawn((transform.into(), Camera::default()))
    }
}

#[derive(Clone, Debug)]
pub struct SharedWorld(pub Arc<RwLock<World>>);

impl SharedWorld {
    pub fn world(&self) -> RwLockReadGuard<'_, World> {
        self.0.read()
    }

    pub fn world_mut(&self) -> RwLockWriteGuard<'_, World> {
        self.0.write()
    }
}

impl From<World> for SharedWorld {
    fn from(value: World) -> Self {
        Self(Arc::new(RwLock::new(value)))
    }
}

pub struct SharedShape(pub Arc<dyn Shape>);

impl<S: Shape> From<S> for SharedShape {
    fn from(value: S) -> Self {
        Self(Arc::new(value))
    }
}

pub trait Shape: Debug + Send + Sync + 'static {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh>;
}

impl Shape for Ball {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        let (vertices, indices) = self.to_trimesh(10, 20);
        Some(SurfaceMesh { vertices, indices })
    }
}

impl Shape for Cuboid {
    fn to_surface_mesh(&self) -> Option<SurfaceMesh> {
        let (vertices, indices) = self.to_trimesh();
        Some(SurfaceMesh { vertices, indices })
    }
}

#[derive(Clone, Debug)]
pub struct SurfaceMesh {
    pub vertices: Vec<Point3<f32>>,
    pub indices: Vec<[u32; 3]>,
}

#[derive(Clone, Copy, Debug)]
pub struct Transform {
    pub transform: Isometry3<f32>,
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
