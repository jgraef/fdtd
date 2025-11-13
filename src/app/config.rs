use palette::Srgb;
use serde::{
    Deserialize,
    Serialize,
};

use crate::app::composer::{
    renderer::{
        Outline,
        light::CameraLightFilter,
    },
    solver::SolverConfig,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_recently_opened_files_limit")]
    pub recently_opened_files_limit: usize,

    #[serde(default)]
    pub composer: ComposerConfig,

    /// Default solver configs
    #[serde(default)]
    pub default_solver_configs: Vec<SolverConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            recently_opened_files_limit: default_recently_opened_files_limit(),
            composer: Default::default(),
            default_solver_configs: vec![],
        }
    }
}

fn default_recently_opened_files_limit() -> usize {
    10
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ComposerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_limit: Option<usize>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redo_limit: Option<usize>,

    #[serde(default)]
    pub views: ViewsConfig,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ViewsConfig {
    #[serde(rename = "3d", default)]
    pub view_3d: View3dConfig,

    #[serde(default)]
    pub selection_outline: Outline,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View3dConfig {
    // todo: option so we can have no clear color
    #[serde(
        default = "default_background_color",
        with = "crate::util::serde::palette"
    )]
    pub background_color: Srgb,

    /// in degrees
    #[serde(default = "default_fovy")]
    pub fovy: f32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_filter: Option<CameraLightFilter>,
}

impl Default for View3dConfig {
    fn default() -> Self {
        Self {
            background_color: default_background_color(),
            fovy: default_fovy(),
            light_filter: None,
        }
    }
}

fn default_background_color() -> Srgb {
    palette::named::BLUEVIOLET.into_format()
}

fn default_fovy() -> f32 {
    45.0
}
