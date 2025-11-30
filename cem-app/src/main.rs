#![allow(dead_code)]
#![allow(clippy::explicit_counter_loop)]

pub mod app;
pub mod build_info;
pub mod geometry;
pub mod util;

use std::path::PathBuf;

use clap::{
    Parser,
    Subcommand,
};
use color_eyre::eyre::{
    Error,
    bail,
};
use dotenvy::dotenv;
use tracing_subscriber::EnvFilter;

use crate::app::config::AppConfig;

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
        Command::DumpDefaultConfig { output, format } => {
            let config = AppConfig::default();
            let config = match format.as_str() {
                "toml" => toml::to_string_pretty(&config)?,
                "json" => serde_json::to_string_pretty(&config)?,
                _ => bail!("Invalid format: {format}"),
            };
            if let Some(output) = &output {
                std::fs::write(output, &config)?;
            }
            else {
                println!("{config}");
            }
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
    Main(app::args::Args),
    DumpDefaultConfig {
        #[clap(short, long)]
        output: Option<PathBuf>,
        #[clap(short, long, default_value = "toml")]
        format: String,
    },
}
