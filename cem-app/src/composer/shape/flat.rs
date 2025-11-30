use std::{
    convert::Infallible,
    sync::Arc,
};

use nalgebra::{
    Isometry3,
    Point2,
    Point3,
    Vector2,
    Vector3,
};
use parry3d::{
    bounding_volume::Aabb,
    query::{
        Ray,
        RayCast as _,
    },
};

use crate::composer::{
    renderer::mesh::{
        GenerateMesh,
        IntoGenerateMesh,
        MeshBuilder,
        WindingOrder,
    },
    scene::spatial::{
        Collider,
        ComputeAabb,
        PointQuery,
        RayCast,
    },
};

#[derive(Clone, Copy, Debug, Default)]
pub struct QuadMeshConfig {
    // todo: config how uvs should behave with back face
    pub back_face: bool,
}

#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Copy, Debug)]
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
    fn compute_aabb(&self, transform: &nalgebra::Isometry3<f32>) -> Aabb {
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
