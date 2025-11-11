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

    fn depth_texture_format(&self) -> Option<wgpu::TextureFormat> {
        Some(wgpu::TextureFormat::Depth24PlusStencil8)
    }

    fn create_app(self, context: CreateAppContext) -> Self::App {
        App::new(context, self)
    }

    fn required_features(&self) -> wgpu::Features {
        wgpu::Features::POLYGON_MODE_LINE
    }
}
