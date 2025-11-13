use std::{
    fmt::Debug,
    ops::Deref,
    sync::Arc,
};

use parry3d::shape::{
    Ball,
    Cuboid,
    Cylinder,
    HalfSpace,
    TriMesh,
};
use serde::Serialize;

use crate::app::composer::renderer::mesh::{
    SurfaceMesh,
    WindingOrder,
};

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
