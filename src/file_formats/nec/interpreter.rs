use std::{
    collections::BTreeMap,
    f32::consts::TAU,
    io::BufRead,
    ops::Bound,
};

use nalgebra::{
    Isometry3,
    Matrix4,
    Translation3,
    UnitQuaternion,
    UnitVector3,
    Vector3,
};

use crate::file_formats::nec::{
    card::{
        CardHandler,
        GroundPlaneFlag,
        ReflectionAxis,
        Section,
        SurfacePatchSpecification,
        Tag,
        WireSegments,
    },
    parser::NecParser,
};

#[derive(Debug, thiserror::Error)]
#[error("NEC error")]
pub enum Error {
    #[error("NEC parser error")]
    Parser(#[from] super::parser::Error),
    #[error("Invalid card type {card_type} in {section:?} section")]
    InvalidCardType { section: Section, card_type: String },
}

#[derive(Clone, Debug, Default)]
pub struct NecFile {
    pub comments: Vec<String>,
    pub geometry: Vec<(Tag, Geometry)>,
    pub ground_plane_flag: GroundPlaneFlag,
    pub symmetry_flag: SymmetryFlag,
    pub ignored_decks: Vec<String>,
}

impl NecFile {
    pub fn from_reader(reader: impl BufRead) -> Result<Self, Error> {
        let mut interpreter = CardInterpreter::default();

        let mut parser = NecParser::default();
        parser.read_file(reader, &mut interpreter)?;
        Ok(interpreter.finish())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub specification: GeometrySpecification,
    pub transform: Matrix4<f32>,
}

impl Geometry {
    pub fn append_transform(&mut self, transform: &Matrix4<f32>) {
        self.transform = transform * self.transform;
    }
}

#[derive(Clone, Copy, Debug)]
pub enum GeometrySpecification {
    /// GA card
    WireArc {
        num_segments: u32,
        arc_radius: f32,
        arc_angles: [f32; 2],
        wire_radius: f32,
    },
    /// GW card
    Wire {
        length: f32,
        num_segments: u32,
        segments: WireSegments,
    },
    // todo: this should contain Point3's, but then nalgebra isn't isolated to the geometry buffer
    // anymore, since SurfacePatchSpecification is emitted by the parser itself.
    // we could also just have the variants with points in them here.
    SurfacePatch(SurfacePatchSpecification),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SymmetryFlag {
    #[default]
    None,
    /// Rotational symmetry around Z axis
    Rotational,
    /// Planar symmetry around the given axis
    Planar(ReflectionAxis),
}

// todo: symmetry isn't properly implemented
#[derive(Clone, Debug, Default)]
pub struct CardInterpreter {
    comments: Vec<String>,
    ignored_decks: Option<Vec<String>>,
    geometry: BTreeMap<Tag, Geometry>,
    deferred_insertions: Vec<(Tag, Geometry)>,
    deferred_removals: Vec<Tag>,
    symmetry_flag: SymmetryFlag,
    ground_plane_flag: GroundPlaneFlag,
}

impl CardInterpreter {
    pub fn finish(self) -> NecFile {
        NecFile {
            comments: self.comments,
            geometry: self.geometry.into_iter().collect(),
            ground_plane_flag: self.ground_plane_flag,
            symmetry_flag: self.symmetry_flag,
            ignored_decks: self.ignored_decks.unwrap_or_default(),
        }
    }

    /// Helper to implement GM and GR card
    fn modify_impl(
        &mut self,
        tag_increment: u32,
        num_new: u32,
        base_transform: Isometry3<f32>,
        start_bound: Bound<Tag>,
        dont_increment_tag_0: bool,
    ) {
        let new_tag = |mut tag: Tag, i: u32| {
            // why don't we increment tag 0?
            if tag != 0 && !dont_increment_tag_0 {
                tag += tag_increment * i;
            }
            tag
        };

        let base_transform = base_transform.to_homogeneous();

        for (tag, geometry) in self.geometry.range_mut((start_bound, Bound::Unbounded)) {
            if num_new == 0 {
                // move structure
                geometry.append_transform(&base_transform);

                if tag_increment != 0 {
                    // move to new tag
                    self.deferred_removals.push(*tag);
                    self.deferred_insertions.push((new_tag(*tag, 1), *geometry));
                }
            }
            else {
                // duplicate and move structure
                let mut geometry = *geometry;

                for i in 0..num_new {
                    geometry.append_transform(&base_transform);
                    self.deferred_insertions
                        .push((new_tag(*tag, i + 1), geometry));
                }
            }
        }

        self.apply_deferred();
    }

    fn apply_deferred(&mut self) {
        for tag in self.deferred_removals.drain(..) {
            self.geometry.remove(&tag);
        }
        for (tag, geometry) in self.deferred_insertions.drain(..) {
            self.geometry.insert(tag, geometry);
        }
    }
}

impl CardHandler for CardInterpreter {
    /// CM and CE cards
    fn comment(&mut self, comment: &str) {
        self.comments.push(comment.to_owned());
    }

    fn unknown_card(&mut self, _section: Section, card: &str) {
        if let Some(ignored_decks) = &mut self.ignored_decks {
            ignored_decks.push(card.to_owned());
        }
        else {
            // todo: return error
            /*return Err(Error::InvalidCardType {
                section: Section::Geometry,
                card_type: card_type.to_owned(),
            });*/
        }
    }

    /// GA card
    fn wire_arc_specification(
        &mut self,
        tag: Tag,
        num_segments: u32,
        arc_radius: f32,
        arc_angles: [f32; 2],
        wire_radius: f32,
    ) {
        self.geometry.insert(
            tag,
            Geometry {
                specification: GeometrySpecification::WireArc {
                    num_segments,
                    arc_radius,
                    arc_angles,
                    wire_radius,
                },
                transform: Matrix4::identity(),
            },
        );
        self.symmetry_flag = Default::default();
    }

    /// GE card
    fn end_geometry_input(&mut self, ground_plane_flag: GroundPlaneFlag) {
        self.ground_plane_flag = ground_plane_flag;
        if let (GroundPlaneFlag::Present { .. }, SymmetryFlag::Planar(axis)) =
            (ground_plane_flag, &mut self.symmetry_flag)
        {
            axis.remove(ReflectionAxis::Z);
        }
    }

    /// GM card
    fn coordinate_transformation(
        &mut self,
        tag_increment: u32,
        num_new: u32,
        rotation: [f32; 3],
        translation: [f32; 3],
        tag_start: Option<Tag>,
    ) {
        if num_new > 0 || tag_start.is_some() {
            // destroys symmetry
            self.symmetry_flag = Default::default();
        }

        let start_bound = tag_start.map_or(Bound::Unbounded, Bound::Included);

        let base_rotation =
            UnitQuaternion::from_axis_angle(&Vector3::z_axis(), rotation[2].to_radians())
                * UnitQuaternion::from_axis_angle(&Vector3::y_axis(), rotation[1].to_radians())
                * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), rotation[0].to_radians());
        let base_translation = Translation3::from(translation);
        let base_transform = Isometry3::from_parts(base_translation, base_rotation);

        self.modify_impl(tag_increment, num_new, base_transform, start_bound, false)
    }

    /// GR card
    fn generate_cylindrical_structure(&mut self, tag_increment: u32, num_copies: u32) {
        // todo: we should probably return an error if num_copies is 0
        let num_copies = num_copies.min(1);

        let angle_increment = TAU / (num_copies as f32);

        let base_transform = Isometry3::from_parts(
            Translation3::identity(),
            UnitQuaternion::from_axis_angle(&Vector3::z_axis(), angle_increment),
        );

        self.symmetry_flag = SymmetryFlag::Rotational;
        self.modify_impl(
            tag_increment,
            num_copies - 1,
            base_transform,
            Bound::Unbounded,
            true,
        )
    }

    /// GS card
    fn scale_structure(&mut self, scaling: f32) {
        let mut scaling_matrix = Matrix4::zeros();
        scaling_matrix.m11 = scaling;
        scaling_matrix.m22 = scaling;
        scaling_matrix.m33 = scaling;
        scaling_matrix.m44 = 1.0;

        for geometry in &mut self.geometry.values_mut() {
            geometry.transform = scaling_matrix * geometry.transform;
            //geometry.transform.translation.vector *= scaling;

            match &mut geometry.specification {
                GeometrySpecification::WireArc {
                    arc_radius,
                    wire_radius,
                    ..
                } => {
                    *arc_radius *= scaling;
                    *wire_radius *= scaling;
                }
                GeometrySpecification::Wire {
                    length, segments, ..
                } => {
                    *length *= scaling;
                    segments.scale(scaling);
                }
                GeometrySpecification::SurfacePatch(_surface_patch_specification) => {
                    todo!("scale surface patch");
                }
            }
        }
    }

    /// GW card
    fn wire_specification(
        &mut self,
        tag: Tag,
        num_segments: u32,
        wire_ends: [[f32; 3]; 2],
        wire_segments: WireSegments,
    ) {
        let wire_ends = wire_ends.map(Vector3::from);
        let wire_delta = wire_ends[0] - wire_ends[1];

        self.geometry.insert(
            tag,
            Geometry {
                specification: GeometrySpecification::Wire {
                    length: wire_delta.norm(),
                    num_segments,
                    segments: wire_segments,
                },
                transform: Isometry3::from_parts(
                    Translation3::from(wire_ends[0]),
                    UnitQuaternion::from_axis_angle(&UnitVector3::new_normalize(wire_delta), 0.0),
                )
                .to_homogeneous(),
            },
        );
        self.symmetry_flag = Default::default();
    }

    /// GX card
    fn reflection_in_coordinate_planes(
        &mut self,
        mut tag_increment: u32,
        reflection_axis: ReflectionAxis,
    ) {
        // performs reflections of all geometry in the buffer around a plane normal to
        // the specified axis. the axis is specified as x=0, y=1, z=2 (i.e. the index
        // for the diagonal element of a reflection matrix that is set to -1.0).
        let mut reflect = |axis| {
            let mut reflection_matrix = Matrix4::<f32>::identity();
            reflection_matrix[(axis, axis)] = -1.0;

            for (tag, geometry) in &self.geometry {
                let new_tag = *tag + tag_increment;

                let new_geometry = Geometry {
                    specification: geometry.specification,
                    transform: reflection_matrix * geometry.transform,
                };

                self.deferred_insertions.push((new_tag, new_geometry));
            }

            tag_increment *= 2;
        };

        if reflection_axis.contains(ReflectionAxis::Z) {
            reflect(2);
        }
        if reflection_axis.contains(ReflectionAxis::Y) {
            reflect(1);
        }
        if reflection_axis.contains(ReflectionAxis::X) {
            reflect(0);
        }

        self.symmetry_flag = SymmetryFlag::Planar(reflection_axis);
    }

    /// SP card
    fn surface_patch(&mut self, _surface_patch_specification: SurfacePatchSpecification) {
        // should we derive a transform from the vertices and normalize the vertices
        // such that they're centered around a local origin (e.g. their barycenter)?
        //self.geometry.insert(key, value)
        // note: these don't use tags!
        todo!("surface patch");
    }
}
