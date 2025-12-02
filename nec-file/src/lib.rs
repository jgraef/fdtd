#![warn(unused_qualifications)]

//! [NEC][1] file format
//!
//! The [xnec2c implementation][2] for reference.
//!
//! [1]: https://www.radio-bip.qc.ca/NEC2/nec2prt3.pdf
//! [2]: https://github.com/KJ7LNW/xnec2c/blob/70e3922c477d11294742ac05a1f17428fc9b658a/src/input.c

pub mod card;
pub mod interpreter;
pub mod parser;

pub use crate::interpreter::NecFile;
