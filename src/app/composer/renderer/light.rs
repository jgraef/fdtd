use std::path::PathBuf;

use bytemuck::{
    Pod,
    Zeroable,
};
use palette::{
    LinSrgba,
    Srgb,
    Srgba,
    WithAlpha,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    Error,
    app::composer::{
        properties::{
            PropertiesUi,
            TrackChanges,
            label_and_value,
        },
        renderer::{
            Loader,
            Outline,
            Renderer,
        },
    },
    util::wgpu::texture_view_from_path,
};

/// Material properties that define how an object looks in the scene.
///
/// This defines how the light from point sources and ambient light it modulated
/// by the objects surface.
///
/// It also defines the colors for wireframe and outline rendering.
///
/// Note that these are only visual properties!
///
/// # TODO
///
/// - Needs to know if this is transparent, so we can sort by depth.
/// - The colors should be `Option`s, so that if nothing is set, it can either
///   default to black or white, depending of a texture is used for that
///   material (see [`MaterialData::new`]). But this requires some work with the
///   serde-integration (we can use the `serde_with` crate).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Material {
    #[serde(with = "crate::util::serde::palette")]
    pub ambient: Srgba,

    #[serde(with = "crate::util::serde::palette")]
    pub diffuse: Srgba,

    #[serde(with = "crate::util::serde::palette")]
    pub specular: Srgba,

    #[serde(with = "crate::util::serde::palette")]
    pub emissive: Srgba,

    pub shininess: f32,

    #[serde(with = "crate::util::serde::palette")]
    pub wireframe: Srgba,
}

impl From<Srgba> for Material {
    fn from(value: Srgba) -> Self {
        const WHITE: Srgba = Srgba::new(1.0, 1.0, 1.0, 1.0);
        const BLACK: Srgba = Srgba::new(0.0, 0.0, 0.0, 1.0);

        Self {
            ambient: value * 0.5,
            diffuse: value,
            specular: WHITE,
            emissive: BLACK,
            shininess: 8.0,
            wireframe: BLACK,
        }
    }
}

impl From<Srgba<u8>> for Material {
    fn from(value: Srgba<u8>) -> Self {
        Self::from(value.into_format::<f32, f32>())
    }
}

impl From<Srgb> for Material {
    fn from(value: Srgb) -> Self {
        Self::from(value.with_alpha(1.0))
    }
}

impl From<Srgb<u8>> for Material {
    fn from(value: Srgb<u8>) -> Self {
        Self::from(value.with_alpha(255))
    }
}

impl PropertiesUi for Material {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(ui, "Ambient", &mut changes, &mut self.ambient);
                label_and_value(ui, "Diffuse", &mut changes, &mut self.diffuse);
                label_and_value(ui, "Specular", &mut changes, &mut self.specular);
                label_and_value(ui, "Emissive", &mut changes, &mut self.emissive);
                label_and_value(ui, "Shininess", &mut changes, &mut self.shininess);
                label_and_value(ui, "Wireframe", &mut changes, &mut self.wireframe);
            })
            .response;

        changes.propagated(response)
    }
}

#[derive(Clone, Debug, Default)]
pub struct MaterialTextures {
    pub ambient: Option<wgpu::TextureView>,
    pub diffuse: Option<wgpu::TextureView>,
    pub specular: Option<wgpu::TextureView>,
    pub emissive: Option<wgpu::TextureView>,
}

/// A point light source.
///
/// This defines the color of the light that can be reflected diffusely or
/// specularly.
///
/// Note that we intend to only support one light source at a time, since there
/// isn't much need for more.
///
/// # TODO
///
/// This is currently not used. At the moment we only want one point light
/// specific for a camera, colocated with it. The camera position (and thus the
/// light's) is already sent to the shader. The diffuse and specular light
/// components can be modulated by the camera as well. So there is no need for
/// this right now.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PointLight {
    #[serde(with = "crate::util::serde::palette")]
    pub diffuse: Srgb,

    #[serde(with = "crate::util::serde::palette")]
    pub specular: Srgb,
}

impl PointLight {
    pub const WHITE: Self = Self::from_single_color(Srgb::new(1.0, 1.0, 1.0));

    pub const fn from_single_color(color: Srgb) -> Self {
        Self {
            diffuse: color,
            specular: color,
        }
    }
}

impl Default for PointLight {
    fn default() -> Self {
        Self::WHITE
    }
}

impl From<Srgb> for PointLight {
    fn from(value: Srgb) -> Self {
        Self::from_single_color(value)
    }
}

impl From<Srgb<u8>> for PointLight {
    fn from(value: Srgb<u8>) -> Self {
        Self::from(value.into_format::<f32>())
    }
}

impl PropertiesUi for PointLight {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(ui, "Diffuse", &mut changes, &mut self.diffuse);
                label_and_value(ui, "Specular", &mut changes, &mut self.specular);
            })
            .response;

        changes.propagated(response)
    }
}

/// Defines filters for lighting components per camera.
///
/// Notably this is the only place to specify ambient lighting. Ambient light is
/// assumed to be bright white everywhere, but it can be modulated by the
/// camera.
///
/// The other components are multiplied with the color generated by the light
/// from a light source or light emitted by the material itself.
#[derive(Clone, Copy, Debug, Pod, Zeroable, Serialize, Deserialize)]
#[repr(C)]
pub struct CameraLightFilter {
    #[serde(with = "crate::util::serde::palette")]
    pub ambient: LinSrgba,

    #[serde(with = "crate::util::serde::palette")]
    pub diffuse: LinSrgba,

    #[serde(with = "crate::util::serde::palette")]
    pub specular: LinSrgba,

    #[serde(with = "crate::util::serde::palette")]
    pub emissive: LinSrgba,
}

impl Default for CameraLightFilter {
    fn default() -> Self {
        // todo: in default operation these should all be white. the materials specify
        // the ratios
        let rgb1 = |x| LinSrgba::new(x, x, x, 1.0);
        Self {
            ambient: rgb1(0.8),
            diffuse: rgb1(0.8),
            specular: rgb1(0.5),
            emissive: rgb1(1.0),
        }
    }
}

impl PropertiesUi for CameraLightFilter {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(ui, "Ambient", &mut changes, &mut self.ambient);
                label_and_value(ui, "Diffuse", &mut changes, &mut self.diffuse);
                label_and_value(ui, "Specular", &mut changes, &mut self.specular);
                label_and_value(ui, "Emissive", &mut changes, &mut self.emissive);
            })
            .response;

        changes.propagated(response)
    }
}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
pub struct MaterialData {
    wireframe: LinSrgba,
    outline: LinSrgba,
    ambient: LinSrgba,
    diffuse: LinSrgba,
    specular: LinSrgba,
    emissive: LinSrgba,
    shininess: f32,
    outline_thickness: f32,
    _padding: [u32; 2],
}

impl MaterialData {
    pub fn new(
        material: Option<&Material>,
        material_textures: Option<&MaterialTextures>,
        outline: Option<&Outline>,
    ) -> Self {
        let (outline, outline_thickness) = outline
            .map(|outline| (outline.color.into_linear(), outline.thickness))
            .unwrap_or_default();

        const BLACK: LinSrgba = LinSrgba::new(0.0, 0.0, 0.0, 1.0);
        const WHITE: LinSrgba = LinSrgba::new(1.0, 1.0, 1.0, 1.0);

        let mut data = Self {
            outline,
            outline_thickness,
            ..Default::default()
        };

        data.wireframe = material
            .as_ref()
            .map_or(BLACK, |material| material.wireframe.into_linear());
        data.shininess = material
            .as_ref()
            .map_or(32.0, |material| material.shininess);

        macro_rules! color {
            ($name:ident) => {
                data.$name = material.as_ref().map_or_else(
                    || {
                        let texture_present = material_textures
                            .map_or(false, |material_textures| material_textures.$name.is_some());
                        if texture_present { WHITE } else { BLACK }
                    },
                    |material| material.$name.into_linear(),
                );
            };
        }

        color!(ambient);
        color!(diffuse);
        color!(specular);
        color!(emissive);

        data
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct PointLightData {
    pub diffuse: LinSrgba,
    pub specular: LinSrgba,
}

impl PointLightData {
    pub fn new(point_light: &PointLight) -> Self {
        Self {
            diffuse: point_light.diffuse.into_linear().with_alpha(1.0),
            specular: point_light.specular.into_linear().with_alpha(1.0),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LoadMaterialTextures {
    pub ambient: Option<PathBuf>,
    pub diffuse: Option<PathBuf>,
    pub specular: Option<PathBuf>,
    pub emissive: Option<PathBuf>,
}

impl LoadMaterialTextures {
    pub fn with_ambient(mut self, path: impl Into<PathBuf>) -> Self {
        self.ambient = Some(path.into());
        self
    }

    pub fn with_diffuse(mut self, path: impl Into<PathBuf>) -> Self {
        self.diffuse = Some(path.into());
        self
    }

    pub fn with_ambient_and_diffuse(self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        self.with_ambient(path.clone()).with_diffuse(path)
    }

    pub fn with_specular(mut self, path: impl Into<PathBuf>) -> Self {
        self.specular = Some(path.into());
        self
    }

    pub fn with_emissive(mut self, path: impl Into<PathBuf>) -> Self {
        self.emissive = Some(path.into());
        self
    }
}

impl Loader for LoadMaterialTextures {
    type Output = (MaterialTextures,);

    fn load(&self, renderer: &Renderer) -> Result<Self::Output, Error> {
        let mut output = MaterialTextures::default();

        macro_rules! texture {
            ($name:ident) => {
                if let Some(path) = &self.$name {
                    output.$name = Some(texture_view_from_path(
                        &renderer.wgpu_context.device,
                        &renderer.wgpu_context.queue,
                        path,
                    )?);
                }
            };
        }

        texture!(ambient);
        texture!(diffuse);
        texture!(specular);
        texture!(emissive);

        Ok((output,))
    }
}
