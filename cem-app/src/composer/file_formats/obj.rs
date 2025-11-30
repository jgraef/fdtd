use std::{
    convert::Infallible,
    path::Path,
};

use nalgebra::Point3;
use parry3d::shape::TriMesh;
use tobj::LoadOptions;

use crate::composer::{
    renderer::material::Material,
    scene::{
        Label,
        PopulateScene,
        Scene,
        transform::LocalTransform,
    },
};

pub type Error = tobj::LoadError;

#[derive(Clone, Debug)]
pub struct ObjFile {
    pub models: Vec<tobj::Model>,
    pub materials: Vec<tobj::Material>,
}

impl ObjFile {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Error> {
        let options = LoadOptions {
            // we are only interested in vertex positions, so we don't need a single index. face
            // normals are reconstructed by our vertex shader anyway.
            single_index: false,
            // triangulate all faces that are not triangles already
            triangulate: true,
            // ignore points. we don't have a use for them right now
            ignore_points: true,
            // same as ignore_points
            ignore_lines: true,
        };

        let (models, material_load_result) = tobj::load_obj(path.as_ref(), &options)?;
        let materials = material_load_result?;

        Ok(Self { models, materials })
    }
}

pub struct PopulateSceneWithObjFile<'a> {
    obj_file: &'a ObjFile,
    transform: LocalTransform,
    material: Material,
}

impl<'a> PopulateScene for PopulateSceneWithObjFile<'a> {
    type Error = Infallible;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error> {
        // todo: does parry expect a windong order? we don't know the winding order of
        // the model. parry uses counter-clockwise (relative to their coordinate system
        // with z axis inverted).

        for model in &self.obj_file.models {
            let label = Label::from(format!("tobj.{}", model.name));

            assert!(model.mesh.face_arities.is_empty(), "non-triangular mesh");
            assert!(
                model.mesh.positions.len() % 3 == 0,
                "number of points not a multiple of 3"
            );
            assert!(
                model.mesh.indices.len() % 3 == 0,
                "number of indices not a multiple of 3"
            );

            // could use bytemuck here, but we need ownership anyway.
            let vertices = model
                .mesh
                .positions
                .chunks_exact(3)
                .map(|point| Point3::new(point[0], point[1], point[2]))
                .collect();

            let indices = model
                .mesh
                .indices
                .chunks_exact(3)
                .map(|face| [face[0], face[1], face[2]])
                .collect();

            // fixme: this should be integrated with the loader API
            let _tri_mesh = TriMesh::new(vertices, indices).expect("invalid triangle mesh");

            scene.entities.spawn((
                self.transform,
                self.material,
                //MeshFromShape::from(tri_mesh),
                label,
            ));
        }

        Ok(())
    }
}
