use std::path::PathBuf;

use color_eyre::eyre::Error;
use dotenv::dotenv;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Error> {
    let _ = dotenv();
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .pretty()
        .init();

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);

    fdtd_build::create_constants_module(
        "src/app/composer/renderer/material/materials.json",
        out_dir.join("materials.rs"),
    )?;

    Ok(())
}
