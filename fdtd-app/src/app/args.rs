use std::path::PathBuf;

use crate::Error;

#[derive(Clone, Debug, clap::Parser)]
pub struct Args {
    pub file: Option<PathBuf>,

    #[clap(long)]
    pub new_file: bool,
}

impl Args {
    pub fn run(self) -> Result<(), Error> {
        crate::app::start::run_app(self)
    }
}
