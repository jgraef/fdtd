use std::str::FromStr;

use bitflags::bitflags;

pub type Tag = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Comments,
    Geometry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardType {
    Cm,
    Ce,
    Ga,
    Gc,
    Ge,
    Gm,
    Gr,
    Gs,
    Gw,
    Gx,
    Sp,
    Sc,
}

impl CardType {
    pub fn section(&self) -> Section {
        match self {
            CardType::Cm | CardType::Ce => Section::Comments,

            CardType::Ga
            | CardType::Gc
            | CardType::Ge
            | CardType::Gm
            | CardType::Gr
            | CardType::Gs
            | CardType::Gw
            | CardType::Gx
            | CardType::Sp
            | CardType::Sc => Section::Geometry,
        }
    }
}

impl FromStr for CardType {
    type Err = InvalidCardType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "CM" => Ok(Self::Cm),
            "CE" => Ok(Self::Ce),
            "GA" => Ok(Self::Ga),
            "GC" => Ok(Self::Gc),
            "GE" => Ok(Self::Ge),
            "GM" => Ok(Self::Gm),
            "GR" => Ok(Self::Gr),
            "GS" => Ok(Self::Gs),
            "GW" => Ok(Self::Gw),
            "GX" => Ok(Self::Gx),
            "SP" => Ok(Self::Sp),
            "SC" => Ok(Self::Sp),
            _ => {
                Err(InvalidCardType {
                    value: s.to_owned(),
                })
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct InvalidCardType {
    pub value: String,
}

pub trait CardHandler {
    /// Unknown cards
    fn unknown_card(&mut self, section: Section, card: &str);

    /// CM and CE cards
    fn comment(&mut self, comment: &str);

    /// GA card
    fn wire_arc_specification(
        &mut self,
        tag: Tag,
        num_segments: u32,
        arc_radius: f32,
        arc_angles: [f32; 2],
        wire_radius: f32,
    );

    /// GE card
    fn end_geometry_input(&mut self, ground_plane_flag: GroundPlaneFlag);

    /// GM card
    fn coordinate_transformation(
        &mut self,
        tag_increment: u32,
        num_new: u32,
        rotation: [f32; 3],
        translation: [f32; 3],
        tag_start: Option<Tag>,
    );

    /// GR card
    fn generate_cylindrical_structure(&mut self, tag_increment: u32, num_copies: u32);

    /// GS card
    fn scale_structure(&mut self, scaling: f32);

    /// GW card
    fn wire_specification(
        &mut self,
        tag: Tag,
        num_segments: u32,
        wire_ends: [[f32; 3]; 2],
        wire_segments: WireSegments,
    );

    /// GX card
    fn reflection_in_coordinate_planes(
        &mut self,
        tag_increment: u32,
        reflection_axis: ReflectionAxis,
    );

    /// SP card
    fn surface_patch(&mut self, _surface_patch_specification: SurfacePatchSpecification);
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

#[derive(Clone, Debug, thiserror::Error)]
#[error("Invalid ground plane flag: {value}")]
pub struct InvalidGroundPlaneFlag {
    pub value: String,
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
