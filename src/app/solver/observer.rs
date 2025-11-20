use std::path::PathBuf;

use crate::app::solver::FieldComponent;

#[derive(Clone, Debug)]
pub struct Observer {
    pub write_to_gif: Option<PathBuf>,
    pub display_as_texture: bool,
    pub field_component: FieldComponent,
}
