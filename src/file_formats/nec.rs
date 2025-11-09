//! [NEC][1] file format
//!
//! The [xnec2c implementation] for reference.
//!
//! [1]: https://www.radio-bip.qc.ca/NEC2/nec2prt3.pdf
//! [2]: https://github.com/KJ7LNW/xnec2c/blob/70e3922c477d11294742ac05a1f17428fc9b658a/src/input.c

use std::{
    collections::BTreeMap,
    convert::Infallible,
    f32::consts::TAU,
    io::BufRead,
    ops::Bound,
    str::{
        FromStr,
        SplitAsciiWhitespace,
    },
};

use arrayvec::ArrayVec;
use bitflags::bitflags;
use nalgebra::{
    Isometry3,
    Matrix4,
    Translation3,
    UnitQuaternion,
    UnitVector3,
    Vector3,
    Vector4,
};
use palette::Srgba;
use parry3d::shape::Cylinder;

use crate::composer::scene::{
    PopulateScene,
    Scene,
    Transform,
};

#[derive(Debug, thiserror::Error)]
#[error("NEC error")]
pub enum Error {
    Io(#[from] std::io::Error),
    #[error("Unexpected end of file")]
    UnexpectedEnd {
        section: Section,
    },
    #[error("Invalid card type {card_type} in {section:?} section")]
    InvalidCardType {
        section: Section,
        card_type: String,
    },

    #[error("Invalid parameters")]
    InvalidParameter {
        // todo some info?
    },
    #[error("Invalid patch shape: {value}")]
    InvalidPatchShape {
        value: u32,
    },
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
        let mut parser = NecParser::default();
        parser.read_file(reader)?;
        Ok(parser.finish())
    }
}

#[derive(Clone, Debug, Default)]
pub struct NecParser {
    comments: Vec<String>,
    geometry_buffer: GeometryBuffer,
    state: ReaderState,
    pub ignore_unknown: bool,
    ignored_decks: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
enum ReaderState {
    #[default]
    ReadComments,
    ReadGeometry,
    ReadGcCard {
        tag: Tag,
        num_segments: u32,
        wire_ends: [[f32; 3]; 2],
    },
    ReadScCard {
        patch_shape: u32,
        vertices: [[f32; 3]; 2],
    },
    End,
}

impl ReaderState {
    fn section(&self) -> Option<Section> {
        match self {
            Self::ReadComments => Some(Section::Comments),
            Self::ReadGeometry | Self::ReadGcCard { .. } | Self::ReadScCard { .. } => {
                Some(Section::Geometry)
            }
            Self::End => None,
        }
    }
}

impl NecParser {
    pub fn finish(self) -> NecFile {
        NecFile {
            comments: self.comments,
            geometry: self.geometry_buffer.geometry.into_iter().collect(),
            ground_plane_flag: self.geometry_buffer.ground_plane_flag,
            symmetry_flag: self.geometry_buffer.symmetry_flag,
            ignored_decks: self.ignored_decks,
        }
    }

    pub fn read_file<R>(&mut self, reader: R) -> Result<(), Error>
    where
        R: BufRead,
    {
        let mut lines = reader.lines();

        while let Some(section) = self.state.section() {
            let Some(card) = lines.next().transpose()?
            else {
                return Err(Error::UnexpectedEnd { section });
            };
            self.parse_card(&card)?;
        }

        Ok(())
    }

    pub fn parse_card(&mut self, line: &str) -> Result<(), Error> {
        let mut tokens = line.split_ascii_whitespace();

        if let Some(card_type) = tokens.next() {
            match &self.state {
                ReaderState::ReadComments => {
                    let mut push_remainder = || {
                        let remainder = line.get(3..).unwrap_or_default();
                        if !remainder.is_empty() {
                            self.comments.push(remainder.to_owned());
                        }
                    };

                    match card_type {
                        "CM" => {
                            push_remainder();
                        }
                        "CE" => {
                            push_remainder();
                            self.state = ReaderState::ReadGeometry;
                        }
                        _ => {
                            return Err(Error::InvalidCardType {
                                section: Section::Comments,
                                card_type: card_type.to_owned(),
                            });
                        }
                    }
                }
                ReaderState::ReadGeometry => {
                    let mut token_reader = TokenReader {
                        tokens,
                        section: Section::Geometry,
                    };

                    match card_type {
                        "GA" => {
                            self.geometry_buffer.wire_arc_specification(
                                token_reader.read()?,
                                token_reader.read()?,
                                token_reader.read()?,
                                token_reader.read_array()?,
                                token_reader.read()?,
                            );
                        }
                        "GE" => {
                            self.geometry_buffer.end_geometry_input(
                                token_reader
                                    .read::<GroundPlaneFlag>()
                                    .ok()
                                    .unwrap_or_default(),
                            );
                            self.state = ReaderState::End;
                        }
                        "GF" => todo!("Read NGF file"),
                        "GM" => {
                            self.geometry_buffer.coordinate_transformation(
                                token_reader.read()?,
                                token_reader.read()?,
                                token_reader.read_array()?,
                                token_reader.read_array()?,
                                token_reader
                                    .read::<f32>()
                                    .ok()
                                    .and_then(|x| (x.round() as i32).try_into().ok())
                                    .filter(|x| *x != 0),
                            );
                        }
                        "GR" => {
                            self.geometry_buffer.generate_cylindrical_structure(
                                token_reader.read()?,
                                token_reader.read()?,
                            );
                        }
                        "GS" => {
                            self.geometry_buffer.scale_structure(token_reader.read()?);
                        }
                        "GW" => {
                            let tag = token_reader.read()?;
                            let num_segments = token_reader.read()?;
                            let wire_ends =
                                [token_reader.read_array()?, token_reader.read_array()?];
                            let wire_radius = token_reader.read::<f32>()?;

                            if wire_radius == 0.0 {
                                self.state = ReaderState::ReadGcCard {
                                    tag,
                                    num_segments,
                                    wire_ends,
                                };
                            }
                            else {
                                self.geometry_buffer.wire_specification(
                                    tag,
                                    num_segments,
                                    wire_ends,
                                    WireSegments::Fixed {
                                        radius: wire_radius,
                                    },
                                );
                            }
                        }
                        "GX" => {
                            self.geometry_buffer.reflection_in_coordinate_planes(
                                token_reader.read()?,
                                token_reader.read()?,
                            );
                        }
                        "SP" => {
                            let patch_shape = token_reader.read::<u32>()?;

                            match patch_shape {
                                0 => {
                                    self.geometry_buffer.surface_patch(
                                        SurfacePatchSpecification::Arbitrary {
                                            position: token_reader.read_array()?,
                                            elevation_angle: token_reader.read::<f32>()?,
                                            azimuth_angle: token_reader.read::<f32>()?,
                                            patch_area: token_reader.read::<f32>()?,
                                        },
                                    );
                                }
                                1..=3 => {
                                    self.state = ReaderState::ReadScCard {
                                        patch_shape,
                                        vertices: [
                                            token_reader.read_array()?,
                                            token_reader.read_array()?,
                                        ],
                                    };
                                }
                                _ => return Err(Error::InvalidPatchShape { value: patch_shape }),
                            }
                        }
                        /*"SY" => {
                            // even xnec2 doesn't implement this
                            // https://github.com/KJ7LNW/xnec2c/blob/70e3922c477d11294742ac05a1f17428fc9b658a/src/input.c#L1265
                            todo!("deck: {line}")
                        }*/
                        _ if self.ignore_unknown => {
                            self.ignored_decks.push(line.to_owned());
                        }
                        _ => {
                            return Err(Error::InvalidCardType {
                                section: Section::Geometry,
                                card_type: card_type.to_owned(),
                            });
                        }
                    }
                }
                ReaderState::ReadGcCard {
                    tag,
                    num_segments,
                    wire_ends,
                } => {
                    let mut token_reader = TokenReader {
                        tokens,
                        section: Section::Geometry,
                    };

                    let wire_segments = WireSegments::Tapered {
                        length_ratio: token_reader.read()?,
                        first_radius: token_reader.read()?,
                        last_radius: token_reader.read()?,
                    };

                    self.geometry_buffer.wire_specification(
                        *tag,
                        *num_segments,
                        *wire_ends,
                        wire_segments,
                    );

                    self.state = ReaderState::ReadGeometry;
                }
                ReaderState::ReadScCard {
                    patch_shape,
                    vertices,
                } => {
                    let mut token_reader = TokenReader {
                        tokens,
                        section: Section::Geometry,
                    };

                    let surface_patch_specification = match patch_shape {
                        1 => {
                            SurfacePatchSpecification::Rectangular {
                                vertices: [vertices[0], vertices[1], token_reader.read_array()?],
                            }
                        }
                        2 => {
                            SurfacePatchSpecification::Triangular {
                                vertices: [vertices[0], vertices[1], token_reader.read_array()?],
                            }
                        }
                        3 => {
                            SurfacePatchSpecification::Quadrilateral {
                                vertices: [
                                    vertices[0],
                                    vertices[1],
                                    token_reader.read_array()?,
                                    token_reader.read_array()?,
                                ],
                            }
                        }
                        _ => unreachable!(),
                    };

                    self.geometry_buffer
                        .surface_patch(surface_patch_specification);
                }
                ReaderState::End => {}
            }
        }

        Ok(())
    }
}

pub type Tag = u32;

#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub specification: GeometrySpecification,
    pub transform: Matrix4<f32>,
}

impl Geometry {
    pub fn append_transform(&mut self, transform: &Matrix4<f32>) {
        self.transform = transform * &self.transform;
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

#[derive(Clone, Copy, Debug)]
pub enum WireSegments {
    Fixed {
        radius: f32,
    },
    Tapered {
        length_ratio: f32,
        first_radius: f32,
        last_radius: f32,
    },
}

impl WireSegments {
    pub fn dimensions(&self, num_segments: u32, total_length: f32) -> WireSegmentDimensionsIter {
        let ns = num_segments as f32;

        match self {
            WireSegments::Fixed { radius } => {
                let segment_length = total_length / ns;
                WireSegmentDimensionsIter::Fixed {
                    segment: 0,
                    num_segments,
                    next_length: segment_length,
                    next_radius: *radius,
                }
            }
            WireSegments::Tapered {
                length_ratio,
                first_radius,
                last_radius,
            } => {
                let radius_ratio = (*last_radius / *first_radius).powf(1.0 / (ns - 1.0));

                let first_length = if *length_ratio == 1.0 {
                    total_length / ns
                }
                else {
                    total_length * (1.0 - *length_ratio) / (1.0 - length_ratio.powf(ns))
                };

                WireSegmentDimensionsIter::Tapered {
                    segment: 0,
                    num_segments,
                    length_ratio: *length_ratio,
                    radius_ratio,
                    next_length: first_length,
                    next_start_radius: *first_radius,
                }
            }
        }
    }

    pub fn scale(&mut self, scale: f32) {
        match self {
            WireSegments::Fixed { radius } => {
                *radius *= scale;
            }
            WireSegments::Tapered {
                first_radius,
                last_radius,
                ..
            } => {
                *first_radius *= scale;
                *last_radius *= scale
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum WireSegmentDimensionsIter {
    Fixed {
        segment: u32,
        num_segments: u32,
        next_length: f32,
        next_radius: f32,
    },
    Tapered {
        segment: u32,
        num_segments: u32,
        length_ratio: f32,
        radius_ratio: f32,
        next_length: f32,
        next_start_radius: f32,
    },
}

impl Iterator for WireSegmentDimensionsIter {
    type Item = WireSegmentDimensions;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            WireSegmentDimensionsIter::Fixed {
                segment,
                num_segments,
                next_length,
                next_radius,
            } => {
                (segment < num_segments).then(|| {
                    *segment += 1;
                    WireSegmentDimensions::Flat {
                        length: *next_length,
                        radius: *next_radius,
                    }
                })
            }
            WireSegmentDimensionsIter::Tapered {
                segment,
                num_segments,
                length_ratio,
                radius_ratio,
                next_length,
                next_start_radius,
            } => {
                (segment < num_segments).then(|| {
                    *segment += 1;

                    let length = *next_length;
                    let start_radius = *next_start_radius;

                    *next_length *= *length_ratio;
                    *next_start_radius *= *radius_ratio;

                    WireSegmentDimensions::Tapered {
                        length,
                        start_radius,
                        end_radius: *next_start_radius,
                    }
                })
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = match self {
            WireSegmentDimensionsIter::Fixed {
                segment,
                num_segments,
                ..
            } => num_segments - segment,
            WireSegmentDimensionsIter::Tapered {
                segment,
                num_segments,
                ..
            } => num_segments - segment,
        } as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for WireSegmentDimensionsIter {}

#[derive(Clone, Copy, Debug)]
pub enum WireSegmentDimensions {
    Flat {
        length: f32,
        radius: f32,
    },
    Tapered {
        length: f32,
        start_radius: f32,
        end_radius: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GroundPlaneFlag {
    #[default]
    NotPresent,
    Present {
        current_modified: bool,
    },
}

impl FromStr for GroundPlaneFlag {
    type Err = InvalidGroundPlaneFlag;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<i32>()
            .ok()
            .and_then(|n| {
                match n {
                    0 => Some(Self::NotPresent),
                    1 => {
                        Some(Self::Present {
                            current_modified: true,
                        })
                    }
                    -1 => {
                        Some(Self::Present {
                            current_modified: false,
                        })
                    }
                    _ => None,
                }
            })
            .ok_or_else(|| {
                InvalidGroundPlaneFlag {
                    value: s.to_owned(),
                }
            })
    }
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

#[derive(Clone, Debug, thiserror::Error)]
#[error("Invalid ground plane flag: {value}")]
pub struct InvalidGroundPlaneFlag {
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Comments,
    Geometry,
}

#[derive(Clone, Copy, Debug)]
pub enum SurfacePatchSpecification {
    Arbitrary {
        position: [f32; 3],
        elevation_angle: f32,
        azimuth_angle: f32,
        patch_area: f32,
    },
    Rectangular {
        vertices: [[f32; 3]; 3],
    },
    Triangular {
        vertices: [[f32; 3]; 3],
    },
    Quadrilateral {
        vertices: [[f32; 3]; 4],
    },
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ReflectionAxis: u8 {
        const X = 0b001;
        const Y = 0b010;
        const Z = 0b100;
    }
}

impl FromStr for ReflectionAxis {
    type Err = InvalidReflectionAxis;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || {
            InvalidReflectionAxis {
                value: s.to_owned(),
            }
        };

        let mut chars = s.chars();
        let mut total_axis = Self::empty();

        let mut next = |axis| {
            match chars.next() {
                Some('0') => {}
                Some('1') => total_axis |= axis,
                _ => {
                    return Err(err());
                }
            }
            Ok(())
        };

        next(Self::X)?;
        next(Self::Y)?;
        next(Self::Z)?;

        chars.next().is_none().then_some(total_axis).ok_or_else(err)
    }
}

#[derive(Clone, Debug, thiserror::Error)]
#[error("Invalid reflection axis: {value}")]
pub struct InvalidReflectionAxis {
    pub value: String,
}

#[derive(Debug)]
struct TokenReader<'a> {
    tokens: SplitAsciiWhitespace<'a>,
    section: Section,
}

impl<'a> TokenReader<'a> {
    fn read<T>(&mut self) -> Result<T, Error>
    where
        T: FromStr,
    {
        let token = self.tokens.next().ok_or_else(|| {
            Error::UnexpectedEnd {
                section: self.section,
            }
        })?;
        token.parse::<T>().map_err(|_| Error::InvalidParameter {})
    }

    fn read_array<const N: usize, T>(&mut self) -> Result<[T; N], Error>
    where
        T: FromStr,
    {
        let mut buf: ArrayVec<T, N> = ArrayVec::new_const();

        for _ in 0..N {
            buf.push(self.read()?);
        }

        Ok(buf.into_inner().unwrap_or_else(|_| unreachable!()))
    }
}

// todo: symmetry isn't properly implemented
#[derive(Clone, Debug, Default)]
struct GeometryBuffer {
    geometry: BTreeMap<Tag, Geometry>,
    deferred_insertions: Vec<(Tag, Geometry)>,
    deferred_removals: Vec<Tag>,
    symmetry_flag: SymmetryFlag,
    ground_plane_flag: GroundPlaneFlag,
}

impl GeometryBuffer {
    /// GA card
    pub fn wire_arc_specification(
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
    pub fn end_geometry_input(&mut self, ground_plane_flag: GroundPlaneFlag) {
        self.ground_plane_flag = ground_plane_flag;
        match (ground_plane_flag, &mut self.symmetry_flag) {
            (GroundPlaneFlag::Present { .. }, SymmetryFlag::Planar(axis)) => {
                axis.remove(ReflectionAxis::Z);
            }
            _ => {}
        }
    }

    /// GM card
    pub fn coordinate_transformation(
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

        let start_bound =
            tag_start.map_or_else(|| Bound::Unbounded, |tag_start| Bound::Included(tag_start));

        let base_rotation =
            UnitQuaternion::from_axis_angle(&Vector3::z_axis(), rotation[2].to_radians())
                * UnitQuaternion::from_axis_angle(&Vector3::y_axis(), rotation[1].to_radians())
                * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), rotation[0].to_radians());
        let base_translation = Translation3::from(translation);
        let base_transform = Isometry3::from_parts(base_translation, base_rotation);

        self.modify_impl(tag_increment, num_new, base_transform, start_bound, false)
    }

    /// GR card
    pub fn generate_cylindrical_structure(&mut self, tag_increment: u32, num_copies: u32) {
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
    pub fn scale_structure(&mut self, scaling: f32) {
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
    pub fn wire_specification(
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
    pub fn reflection_in_coordinate_planes(
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
                    transform: &reflection_matrix * &geometry.transform,
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
    pub fn surface_patch(&mut self, _surface_patch_specification: SurfacePatchSpecification) {
        // should we derive a transform from the vertices and normalize the vertices
        // such that they're centered around a local origin (e.g. their barycenter)?
        //self.geometry.insert(key, value)
        // note: these don't use tags!
        todo!("surface patch");
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

#[derive(Clone, Copy, Debug)]
pub struct PopulateWithNec<'a> {
    pub nec_file: &'a NecFile,
    pub color: Srgba,
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

                                let transform = Transform::new(
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

                                scene.add_object(transform, shape, self.color);
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
