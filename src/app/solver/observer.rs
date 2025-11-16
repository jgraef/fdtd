use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Observer {
    pub write_to_gif: Option<PathBuf>,
    pub display_as_texture: bool,
}
