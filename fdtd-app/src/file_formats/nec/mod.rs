#![allow(clippy::todo)]
//! [NEC][1] file format
//!
//! The [xnec2c implementation] for reference.
//!
//! [1]: https://www.radio-bip.qc.ca/NEC2/nec2prt3.pdf
//! [2]: https://github.com/KJ7LNW/xnec2c/blob/70e3922c477d11294742ac05a1f17428fc9b658a/src/input.c

pub mod card;
pub mod interpreter;
pub mod parser;

use std::convert::Infallible;

use nalgebra::{
    Translation3,
    UnitQuaternion,
    UnitVector3,
    Vector4,
};
use parry3d::shape::Cylinder;

pub use crate::file_formats::nec::interpreter::NecFile;
use crate::{
    app::composer::{
        renderer::material::Material,
        scene::{
            PopulateScene,
            Scene,
            Spawn,
            transform::LocalTransform,
        },
    },
    file_formats::nec::{
        card::{
            GroundPlaneFlag,
            SurfacePatchSpecification,
            WireSegmentDimensions,
            WireSegments,
        },
        interpreter::GeometrySpecification,
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

                                scene
                                    .add_object(transform, shape)
                                    .material(self.material)
                                    .spawn(scene);
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
