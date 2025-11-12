#![allow(dead_code)]
#![allow(clippy::explicit_counter_loop)]

pub mod app;
pub mod fdtd;
pub mod feec;
pub mod file_formats;
pub mod geometry;
pub mod util;

use std::{
    fs::File,
    io::BufReader,
    path::PathBuf,
};

use clap::{
    Parser,
    Subcommand,
};
use color_eyre::eyre::Error;
use dotenvy::dotenv;
use tracing_subscriber::EnvFilter;

use crate::{
    app::start::CreateApp,
    file_formats::nec::NecFile,
};

fn main() -> Result<(), Error> {
    let _ = dotenv();
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .pretty()
        .init();

    let args = Args::parse();
    match args.command {
        Command::Main(args) => {
            args.run()?;
        }
        Command::Fdtd(args) => {
            args.run()?;
        }
        Command::ReadNec { file } => {
            let reader = BufReader::new(File::open(&file)?);
            let nec = NecFile::from_reader(reader)?;
            println!("{nec:#?}");
        }
    }

    Ok(())
}

#[derive(Debug, Parser)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    // the main app, the other's are just temporary for testing purposes
    Main(crate::app::args::Args),
    Fdtd(fdtd::Args),
    ReadNec { file: PathBuf },
}
