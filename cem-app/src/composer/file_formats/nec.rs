#![allow(clippy::todo)]

use std::convert::Infallible;

use cem_scene::Scene;
use nalgebra::{
    Translation3,
    UnitQuaternion,
    UnitVector3,
    Vector4,
};
use nec_file::{
    NecFile,
    card::WireSegmentDimensions,
    interpreter::GeometrySpecification,
};
use parry3d::shape::Cylinder;

use crate::{
    renderer::material::Material,
    scene::{
        EntityBuilderExt,
        PopulateScene,
        SceneExt,
        transform::LocalTransform,
    },
};

#[derive(Clone, Copy, Debug)]
pub struct PopulateWithNec<'a> {
    pub nec_file: &'a NecFile,
    pub material: Material,
}

impl<'a> PopulateScene for PopulateWithNec<'a> {
    type Error = Infallible;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error> {
        for (_tag, geometry) in &self.nec_file.geometry {
            match geometry.specification {
                GeometrySpecification::WireArc { .. } => todo!("populate scene: wire-arc"),
                GeometrySpecification::Wire {
                    length,
                    num_segments,
                    segments,
                } => {
                    for (i, wire_segment) in segments.dimensions(num_segments, length).enumerate() {
                        match wire_segment {
                            WireSegmentDimensions::Flat { length, radius } => {
                                let shape = Cylinder::new(0.5 * length, radius);

                                let transform = LocalTransform::new(
                                    // get the translation by applying the origin point + length
                                    // along the wire to the transform
                                    Translation3::from(
                                        (geometry.transform
                                            * (Vector4::w() + i as f32 * length * Vector4::y()))
                                        .xyz(),
                                    ),
                                    // get the rotation by applying a y-vector (parry's cone is
                                    // aligned along the y axis)
                                    UnitQuaternion::from_axis_angle(
                                        &UnitVector3::new_normalize(
                                            (geometry.transform * Vector4::y()).xyz(),
                                        ),
                                        0.0,
                                    ),
                                );

                                scene.add_object(transform, shape).material(self.material);
                            }
                            WireSegmentDimensions::Tapered { .. } => todo!("truncated cone shape"),
                        }
                    }
                }
                GeometrySpecification::SurfacePatch(_) => todo!("populate scene: surface patch"),
            }
        }

        Ok(())
    }
}
