use std::path::PathBuf;

use crate::app::{
    App,
    start::{
        CreateApp,
        CreateAppContext,
    },
};

#[derive(Debug, clap::Parser)]
pub struct Args {
    pub file: Option<PathBuf>,

    #[clap(long)]
    pub new_file: bool,
}

impl CreateApp for Args {
    type App = App;

    fn create_app(self, context: CreateAppContext) -> Self::App {
        App::new(context, self)
    }
}
