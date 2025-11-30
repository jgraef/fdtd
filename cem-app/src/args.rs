use std::path::PathBuf;

#[derive(Clone, Debug, clap::Parser)]
pub struct Args {
    pub file: Option<PathBuf>,

    #[clap(long)]
    pub new_file: bool,

    #[clap(long)]
    pub ignore_config: bool,
}
