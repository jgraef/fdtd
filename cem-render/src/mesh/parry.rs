use std::convert::Infallible;

use nalgebra::Point3;
use parry3d::shape::{
    Ball,
    Cuboid,
    Cylinder,
};

use crate::mesh::{
    GenerateMesh,
    IntoGenerateMesh,
    MeshBuilder,
    WindingOrder,
};

/// according to the [documentation][1] the tri mesh should be wound
/// counter-clockwise.
///
/// [1]: https://docs.rs/parry3d/latest/parry3d/shape/struct.TriMesh.html#method.new
const PARRY_WINDING_ORDER: WindingOrder = WindingOrder::CounterClockwise;

fn write_parry_to_trimesh_output_into_mesh_builder(
    mesh_builder: &mut dyn MeshBuilder,
    (vertices, indices): (Vec<Point3<f32>>, Vec<[u32; 3]>),
) {
    for face in indices {
        mesh_builder.push_face(face, PARRY_WINDING_ORDER);
    }
    for vertex in vertices {
        // todo: normals, uvs
        mesh_builder.push_vertex(vertex, None, None);
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BallMeshConfig {
    Uv {
        inclination_subdivisions: u32,
        azimuth_subdivisions: u32,
    },
}

impl Default for BallMeshConfig {
    fn default() -> Self {
        Self::Uv {
            inclination_subdivisions: 10,
            azimuth_subdivisions: 20,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BallMeshGenerator {
    pub ball: Ball,
    pub config: BallMeshConfig,
}

impl GenerateMesh for BallMeshGenerator {
    fn generate(&self, mesh_builder: &mut dyn MeshBuilder, normals: bool, uvs: bool) {
        let _ = uvs;

        let (vertices, indices) = match self.config {
            BallMeshConfig::Uv {
                azimuth_subdivisions: aximuth_subdivisions,
                inclination_subdivisions,
            } => {
                self.ball
                    .to_trimesh(aximuth_subdivisions, inclination_subdivisions)
            }
        };

        for face in indices {
            mesh_builder.push_face(face, PARRY_WINDING_ORDER);
        }
        for vertex in vertices {
            let normal = normals.then(|| vertex.coords.normalize());
            // todo: uvs
            mesh_builder.push_vertex(vertex, normal, None);
        }
    }
}

impl IntoGenerateMesh for Ball {
    type Config = BallMeshConfig;
    type GenerateMesh = BallMeshGenerator;
    type Error = Infallible;

    fn into_generate_mesh(self, config: Self::Config) -> Result<Self::GenerateMesh, Self::Error> {
        Ok(BallMeshGenerator { ball: self, config })
    }
}

impl GenerateMesh for Cuboid {
    fn generate(&self, mesh_builder: &mut dyn MeshBuilder, normals: bool, uvs: bool) {
        let _ = (normals, uvs);
        write_parry_to_trimesh_output_into_mesh_builder(mesh_builder, self.to_trimesh());
    }
}

impl IntoGenerateMesh for Cuboid {
    type Config = ();
    type GenerateMesh = Self;
    type Error = Infallible;

    fn into_generate_mesh(self, config: Self::Config) -> Result<Self::GenerateMesh, Self::Error> {
        // clippy! this is obviously on purrrrpooossse
        #[allow(clippy::let_unit_value)]
        let _ = config;
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CylinderMeshConfig {
    pub subdivisions: u32,
}

impl Default for CylinderMeshConfig {
    fn default() -> Self {
        Self { subdivisions: 20 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CylinderMeshGenerator {
    pub cylinder: Cylinder,
    pub config: CylinderMeshConfig,
}

impl GenerateMesh for CylinderMeshGenerator {
    fn generate(&self, mesh_builder: &mut dyn MeshBuilder, normals: bool, uvs: bool) {
        let _ = (normals, uvs);
        write_parry_to_trimesh_output_into_mesh_builder(
            mesh_builder,
            self.cylinder.to_trimesh(self.config.subdivisions),
        );
    }
}

impl IntoGenerateMesh for Cylinder {
    type Config = CylinderMeshConfig;
    type GenerateMesh = CylinderMeshGenerator;
    type Error = Infallible;

    fn into_generate_mesh(self, config: Self::Config) -> Result<Self::GenerateMesh, Self::Error> {
        Ok(CylinderMeshGenerator {
            cylinder: self,
            config,
        })
    }
}
