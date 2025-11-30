use std::{
    fs::File,
    io::BufReader,
    path::PathBuf,
};

use clap::Parser;
use color_eyre::eyre::Error;
use nec_file::NecFile;

fn main() -> Result<(), Error> {
    color_eyre::install()?;

    let args = Args::parse();
    let reader = BufReader::new(File::open(&args.file)?);
    let nec = NecFile::from_reader(reader)?;
    println!("{nec:#?}");

    Ok(())
}

/// Read a NEC file and print it
#[derive(Debug, Parser)]
struct Args {
    /// Path to NEC file
    file: PathBuf,
}
