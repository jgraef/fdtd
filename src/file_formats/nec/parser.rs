use std::{
    io::BufRead,
    str::FromStr,
};

use arrayvec::ArrayVec;

use crate::file_formats::nec::{
    GroundPlaneFlag,
    SurfacePatchSpecification,
    WireSegments,
    card::{
        CardHandler,
        CardType,
        Section,
        Tag,
    },
};

#[derive(Debug, thiserror::Error)]
#[error("NEC error")]
pub enum Error {
    Io(#[from] std::io::Error),
    #[error("Unexpected end of file")]
    UnexpectedEnd {
        section: Section,
    },
    #[error("Expected a {expected:?} card, but found a {value:?} card")]
    ExpectedSpecificCard {
        value: CardType,
        expected: CardType,
    },
    #[error("Didn't expect a {card_type:?} card while parsing {section:?} section")]
    UnexpectedCardForSection {
        card_type: CardType,
        section: Section,
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
pub struct NecParser {
    state: ParserState,
}

#[derive(Clone, Copy, Debug)]
enum ParserState {
    ReadSection(Section),
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

impl Default for ParserState {
    fn default() -> Self {
        Self::ReadSection(Section::Comments)
    }
}

impl ParserState {
    fn section(&self) -> Option<Section> {
        match self {
            Self::ReadSection(section) => Some(*section),
            Self::ReadGcCard { .. } | Self::ReadScCard { .. } => Some(Section::Geometry),
            Self::End => None,
        }
    }
}

impl NecParser {
    pub fn read_file<R, H>(&mut self, reader: R, card_handler: &mut H) -> Result<(), Error>
    where
        R: BufRead,
        H: CardHandler,
    {
        let mut lines = reader.lines();

        while let Some(section) = self.state.section() {
            let Some(card) = lines.next().transpose()?
            else {
                return Err(Error::UnexpectedEnd { section });
            };
            self.parse_card(&card, &mut *card_handler)?;
        }

        Ok(())
    }

    pub fn parse_card<H>(&mut self, line: &str, card_handler: &mut H) -> Result<(), Error>
    where
        H: CardHandler,
    {
        // what section are we in? if we're in none, we're done.
        let Some(section) = self.state.section()
        else {
            return Ok(());
        };

        let mut token_reader = TokenReader::new(line, section);

        // read card identifier
        let card_type = token_reader.read::<CardType>()?;

        match &self.state {
            ParserState::ReadSection(Section::Comments) => {
                match card_type {
                    CardType::Cm => {
                        card_handler.comment(token_reader.remainder());
                    }
                    CardType::Ce => {
                        let remainder = token_reader.remainder();
                        if !remainder.is_empty() {
                            card_handler.comment(remainder);
                        }
                        self.state = ParserState::ReadSection(Section::Geometry);
                    }
                    _ => return Err(Error::UnexpectedCardForSection { card_type, section }),
                }
            }
            ParserState::ReadSection(Section::Geometry) => {
                match card_type {
                    CardType::Ga => {
                        card_handler.wire_arc_specification(
                            token_reader.read()?,
                            token_reader.read()?,
                            token_reader.read()?,
                            token_reader.read_array()?,
                            token_reader.read()?,
                        );
                    }
                    CardType::Ge => {
                        card_handler.end_geometry_input(
                            token_reader
                                .read::<GroundPlaneFlag>()
                                .ok()
                                .unwrap_or_default(),
                        );
                        self.state = ParserState::End;
                    }
                    //CardType::GF => todo!("Read NGF file"),
                    CardType::Gm => {
                        card_handler.coordinate_transformation(
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
                    CardType::Gr => {
                        card_handler.generate_cylindrical_structure(
                            token_reader.read()?,
                            token_reader.read()?,
                        );
                    }
                    CardType::Gs => {
                        card_handler.scale_structure(token_reader.read()?);
                    }
                    CardType::Gw => {
                        let tag = token_reader.read()?;
                        let num_segments = token_reader.read()?;
                        let wire_ends = [token_reader.read_array()?, token_reader.read_array()?];
                        let wire_radius = token_reader.read::<f32>()?;

                        if wire_radius == 0.0 {
                            self.state = ParserState::ReadGcCard {
                                tag,
                                num_segments,
                                wire_ends,
                            };
                        }
                        else {
                            card_handler.wire_specification(
                                tag,
                                num_segments,
                                wire_ends,
                                WireSegments::Fixed {
                                    radius: wire_radius,
                                },
                            );
                        }
                    }
                    CardType::Gx => {
                        card_handler.reflection_in_coordinate_planes(
                            token_reader.read()?,
                            token_reader.read()?,
                        );
                    }
                    CardType::Sp => {
                        let patch_shape = token_reader.read::<u32>()?;

                        match patch_shape {
                            0 => {
                                card_handler.surface_patch(SurfacePatchSpecification::Arbitrary {
                                    position: token_reader.read_array()?,
                                    elevation_angle: token_reader.read::<f32>()?,
                                    azimuth_angle: token_reader.read::<f32>()?,
                                    patch_area: token_reader.read::<f32>()?,
                                });
                            }
                            1..=3 => {
                                self.state = ParserState::ReadScCard {
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
                    /*CardType::SY => {
                        // even xnec2 doesn't implement this
                        // https://github.com/KJ7LNW/xnec2c/blob/70e3922c477d11294742ac05a1f17428fc9b658a/src/input.c#L1265
                        todo!("deck: {line}")
                    }*/
                    _ => {
                        card_handler.unknown_card(Section::Geometry, line);
                    }
                }
            }
            ParserState::ReadGcCard {
                tag,
                num_segments,
                wire_ends,
            } => {
                let wire_segments = WireSegments::Tapered {
                    length_ratio: token_reader.read()?,
                    first_radius: token_reader.read()?,
                    last_radius: token_reader.read()?,
                };

                card_handler.wire_specification(*tag, *num_segments, *wire_ends, wire_segments);

                self.state = ParserState::ReadSection(Section::Geometry);
            }
            ParserState::ReadScCard {
                patch_shape,
                vertices,
            } => {
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

                card_handler.surface_patch(surface_patch_specification);

                self.state = ParserState::ReadSection(Section::Geometry);
            }
            ParserState::End => {}
        }

        Ok(())
    }
}

#[derive(Debug)]
struct TokenReader<'a> {
    line: &'a str,
    position: usize,
    section: Section,
}

impl<'a> TokenReader<'a> {
    pub fn new(line: &'a str, section: Section) -> Self {
        Self {
            line,
            position: 0,
            section,
        }
    }

    pub fn remainder(&self) -> &'a str {
        &self.line[self.position..]
    }

    fn read_token(&mut self) -> Option<&'a str> {
        if self.position == self.line.len() {
            return None;
        }

        // skip whitespace
        let Some(word_start) = self.line[self.position..]
            .find(|c: char| !c.is_whitespace())
            .map(|p| p + self.position)
        else {
            // only trailing whitespace in string
            self.position = self.line.len();
            return None;
        };

        let word_end = self.line[word_start..]
            .find(|c: char| c.is_whitespace())
            .map(|p| p + word_start)
            .unwrap_or_else(|| {
                // last word in line without trailing whitespace
                self.line.len()
            });

        assert!(word_end > word_start);

        self.position = word_end;
        Some(&self.line[word_start..word_end])
    }

    fn read<T>(&mut self) -> Result<T, Error>
    where
        T: FromStr,
    {
        self.read_token()
            .ok_or_else(|| {
                Error::UnexpectedEnd {
                    section: self.section,
                }
            })?
            .parse::<T>()
            .map_err(|_error| {
                Error::InvalidParameter {
                    // todo
                }
            })
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
