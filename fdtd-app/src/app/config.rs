use std::num::NonZero;

use palette::Srgb;
use serde::{
    Deserialize,
    Serialize,
};

use crate::app::composer::renderer::{
    Outline,
    light::{
        AmbientLight,
        PointLight,
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_recently_opened_files_limit")]
    pub recently_opened_files_limit: usize,

    #[serde(default)]
    pub composer: ComposerConfig,

    pub graphics: GraphicsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            recently_opened_files_limit: default_recently_opened_files_limit(),
            composer: Default::default(),
            graphics: Default::default(),
        }
    }
}

fn default_recently_opened_files_limit() -> usize {
    10
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ComposerConfig {
    #[serde(default)]
    pub undo_limit: Option<usize>,

    #[serde(default)]
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

    #[serde(default = "default_ambient_light")]
    pub ambient_light: AmbientLight,

    #[serde(default = "default_point_light")]
    pub point_light: PointLight,

    #[serde(default = "default_to_true")]
    pub tone_map: bool,

    #[serde(default = "default_gamma")]
    pub gamma: f32,
}

impl Default for View3dConfig {
    fn default() -> Self {
        Self {
            background_color: default_background_color(),
            fovy: default_fovy(),
            ambient_light: default_ambient_light(),
            point_light: default_point_light(),
            tone_map: true,
            gamma: 2.4,
        }
    }
}

fn default_ambient_light() -> AmbientLight {
    AmbientLight::white_light(0.4)
}

fn default_point_light() -> PointLight {
    PointLight::white_light(0.8)
}

fn default_background_color() -> Srgb {
    palette::named::BLUEVIOLET.into_format()
}

fn default_fovy() -> f32 {
    45.0
}

fn default_gamma() -> f32 {
    // note: we need to gamma correct, because the surface texture from egui-wgpu is
    // linear rgba
    2.4
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphicsConfig {
    #[serde(default = "default_wgpu_backends")]
    pub backends: wgpu::Backends,

    #[serde(default)]
    pub power_preference: wgpu::PowerPreference,
    // this is really limited and hard to tell what works
    //#[serde(default = "default_multisample_count")]
    //pub multisample_count: NonZero<u32>,

    //#[serde(default)]
    //pub instance_flags: wgpu::InstanceFlags,

    //pub memory_budget_thresholds: wgpu::MemoryBudgetThresholds,
}

impl Default for GraphicsConfig {
    fn default() -> Self {
        Self {
            backends: default_wgpu_backends(),
            power_preference: Default::default(),
            //multisample_count: default_multisample_count(),
        }
    }
}

fn default_wgpu_backends() -> wgpu::Backends {
    wgpu::Backends::PRIMARY
}

fn default_multisample_count() -> NonZero<u32> {
    NonZero::new(4).unwrap()
}

fn default_to_true() -> bool {
    true
}
