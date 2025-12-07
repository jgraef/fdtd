use std::{
    convert::Infallible,
    sync::Arc,
};

use cem_render::mesh::{
    GenerateMesh,
    IntoGenerateMesh,
    MeshBuilder,
    WindingOrder,
};
use cem_scene::spatial::{
    Collider,
    traits::{
        ComputeAabb,
        PointQuery,
        RayCast,
    },
};
use nalgebra::{
    Isometry3,
    Point2,
    Point3,
    UnitVector3,
    Vector2,
    Vector3,
    Vector4,
};
use num::Bounded;
use parry3d::{
    bounding_volume::Aabb,
    query::{
        Ray,
        RayCast as _,
    },
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::util::scene::ShapeName;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct QuadMeshConfig {
    // todo: config how uvs should behave with back face
    pub back_face: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct QuadMeshGenerator {
    pub quad: Quad,
    pub config: QuadMeshConfig,
}

impl GenerateMesh for QuadMeshGenerator {
    fn generate(&self, mesh_builder: &mut dyn MeshBuilder, normals: bool, uvs: bool) {
        const VERTICES: [(f32, f32); 4] = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
        const INDICES: [[u32; 3]; 2] = [[0, 1, 2], [0, 2, 3]];

        for face in INDICES {
            mesh_builder.push_face(face, WindingOrder::CounterClockwise);
        }
        if self.config.back_face {
            for face in INDICES {
                mesh_builder.push_face(
                    [face[2] + 4, face[1] + 4, face[0] + 4],
                    WindingOrder::CounterClockwise,
                );
            }
        }

        let mut emit_vertices = |normal| {
            for (x, y) in VERTICES {
                mesh_builder.push_vertex(
                    Point3::new(
                        self.quad.half_extents.x * (2.0 * x - 1.0),
                        self.quad.half_extents.y * (2.0 * y - 1.0),
                        0.0,
                    ),
                    normals.then_some(normal),
                    uvs.then(|| Point2::new(x, 1.0 - y)),
                );
            }
        };
        emit_vertices(-Vector3::z());
        emit_vertices(Vector3::z());
    }
}

impl IntoGenerateMesh for Quad {
    type Config = QuadMeshConfig;
    type GenerateMesh = QuadMeshGenerator;
    type Error = Infallible;

    fn into_generate_mesh(self, config: Self::Config) -> Result<Self::GenerateMesh, Self::Error> {
        Ok(QuadMeshGenerator { quad: self, config })
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Quad {
    pub half_extents: Vector2<f32>,
}

impl Quad {
    pub fn new(half_extents: impl Into<Vector2<f32>>) -> Self {
        Self {
            half_extents: half_extents.into(),
        }
    }
}

impl ComputeAabb for Quad {
    fn compute_aabb(&self, transform: &Isometry3<f32>) -> Aabb {
        Aabb::from_half_extents(
            Point3::origin(),
            Vector3::new(self.half_extents.x, self.half_extents.y, 0.0),
        )
        .transform_by(transform)
    }
}

impl RayCast for Quad {
    fn cast_ray(
        &self,
        transform: &Isometry3<f32>,
        ray: &Ray,
        max_time_of_impact: f32,
        solid: bool,
    ) -> Option<f32> {
        self.compute_aabb(transform)
            .cast_local_ray(ray, max_time_of_impact, solid)
    }
}

impl PointQuery for Quad {
    fn supported(&self) -> bool {
        false
    }

    fn contains_point(&self, transform: &Isometry3<f32>, point: &Point3<f32>) -> bool {
        let _ = (transform, point);
        false
    }
}

impl From<Quad> for Collider {
    fn from(value: Quad) -> Self {
        Collider::new(Arc::new(value))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Plane;

impl ShapeName for Plane {
    fn shape_name(&self) -> &str {
        "Plane"
    }
}

impl IntoGenerateMesh for Plane {
    type Config = ();
    type GenerateMesh = PlaneMeshGenerator;
    type Error = Infallible;

    fn into_generate_mesh(self, config: Self::Config) -> Result<Self::GenerateMesh, Self::Error> {
        let _ = config;
        Ok(PlaneMeshGenerator)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PlaneMeshGenerator;

impl GenerateMesh for PlaneMeshGenerator {
    fn generate(&self, mesh_builder: &mut dyn MeshBuilder, normals: bool, uvs: bool) {
        // https://stackoverflow.com/a/12965697

        const VERTICES: [Vector4<f32>; 5] = [
            Vector4::new(0.0, 0.0, 0.0, 1.0),
            Vector4::new(1.0, 0.0, 0.0, 0.0),
            Vector4::new(0.0, 1.0, 0.0, 0.0),
            Vector4::new(-1.0, 0.0, 0.0, 0.0),
            Vector4::new(0.0, -1.0, 0.0, 0.0),
        ];
        const UVS: [Point2<f32>; 5] = [
            Point2::new(0.5, 0.5),
            Point2::new(1.0, 0.5),
            Point2::new(0.5, 1.0),
            Point2::new(0.0, 0.5),
            Point2::new(0.5, 0.0),
        ];
        const INDICES: [[u32; 3]; 4] = [[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 1]];

        let normal = normals.then(|| Vector3::z().to_homogeneous());

        for i in 0..5 {
            mesh_builder.push_vertex_homogeneous(VERTICES[i], normal, uvs.then(|| UVS[i]));
        }
        for face in INDICES {
            mesh_builder.push_face(face, WindingOrder::CounterClockwise);
        }
    }
}

impl From<Plane> for Collider {
    fn from(value: Plane) -> Self {
        Collider::new(Arc::new(value))
    }
}

impl ComputeAabb for Plane {
    fn compute_aabb(&self, transform: &Isometry3<f32>) -> Aabb {
        // this is how parry handles the half-space. technically the AABB isn't infinite
        // if the plane is aligned with some axis https://docs.rs/parry3d/latest/src/parry3d/bounding_volume/aabb_halfspace.rs.html#17
        // We divide by 2.0  so that we can still make some operations with it (like
        // loosening) without breaking the box.
        let _ = transform;
        let max = Point3::max_value() * 0.5;
        Aabb::new(-max, max)
    }
}

impl RayCast for Plane {
    fn cast_ray(
        &self,
        transform: &Isometry3<f32>,
        ray: &Ray,
        max_time_of_impact: f32,
        solid: bool,
    ) -> Option<f32> {
        // parry's code for half-space which is almost identical. we fix our plane to
        // have a normal vector +z though, so the dot products just project the z axis
        //
        // https://docs.rs/parry3d/latest/src/parry3d/query/ray/ray_halfspace.rs.html#49

        let _ = solid;

        let ray = ray.inverse_transform_by(transform);
        let t = -ray.origin.z / ray.dir.z;

        (t >= 0.0 && t <= max_time_of_impact).then(|| {
            let _normal = UnitVector3::new_unchecked(ray.origin.z.signum() * Vector3::z());
            t
        })
    }
}

impl PointQuery for Plane {
    fn supported(&self) -> bool {
        false
    }

    fn contains_point(&self, transform: &Isometry3<f32>, point: &Point3<f32>) -> bool {
        let _ = (transform, point);
        false
    }
}
