use std::{
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

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
            loader::{
                LoaderContext,
                LoadingProgress,
                LoadingState,
            },
            texture_channel::TextureReceiver,
        },
    },
    util::{
        ImageLoadExt,
        wgpu::texture_from_image,
    },
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
    pub ambient: Option<Arc<TextureAndView>>,
    pub diffuse: Option<Arc<TextureAndView>>,
    pub specular: Option<Arc<TextureAndView>>,
    pub emissive: Option<Arc<TextureAndView>>,
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
    pub ambient: Option<TextureSource>,
    pub diffuse: Option<TextureSource>,
    pub specular: Option<TextureSource>,
    pub emissive: Option<TextureSource>,
}

impl LoadMaterialTextures {
    pub fn with_ambient(mut self, texture: impl Into<TextureSource>) -> Self {
        self.ambient = Some(texture.into());
        self
    }

    pub fn with_diffuse(mut self, texture: impl Into<TextureSource>) -> Self {
        self.diffuse = Some(texture.into());
        self
    }

    pub fn with_ambient_and_diffuse(self, texture: impl Into<TextureSource>) -> Self {
        let texture = texture.into();
        self.with_ambient(texture.clone()).with_diffuse(texture)
    }

    pub fn with_specular(mut self, texture: impl Into<TextureSource>) -> Self {
        self.specular = Some(texture.into());
        self
    }

    pub fn with_emissive(mut self, texture: impl Into<TextureSource>) -> Self {
        self.emissive = Some(texture.into());
        self
    }
}

impl Loader for LoadMaterialTextures {
    type State = LoadMaterialTexturesState;

    fn start_loading(
        &self,
        context: &mut LoaderContext,
    ) -> Result<LoadMaterialTexturesState, Error> {
        let _ = context;
        Ok(LoadMaterialTexturesState {
            loader: self.clone(),
            output: Default::default(),
        })
    }
}

#[derive(Debug)]
pub struct LoadMaterialTexturesState {
    pub loader: LoadMaterialTextures,
    pub output: MaterialTextures,
}

impl LoadingState for LoadMaterialTexturesState {
    type Output = (MaterialTextures,);

    fn poll(
        &mut self,
        context: &mut LoaderContext,
    ) -> Result<LoadingProgress<(MaterialTextures,)>, Error> {
        let mut any_still_not_loaded = false;

        macro_rules! material {
            ($name:ident) => {
                if let Some(texture_source) = &mut self.loader.$name {
                    assert!(self.output.$name.is_none());

                    if let Some(texture_and_view) = context.renderer.load_texture(texture_source)? {
                        self.loader.$name = None;
                        self.output.$name = Some(texture_and_view);
                    }
                    else {
                        any_still_not_loaded = true;
                    }
                }
            };
        }

        material!(ambient);
        material!(diffuse);
        material!(specular);
        material!(emissive);

        if any_still_not_loaded {
            Ok(LoadingProgress::Pending)
        }
        else {
            let output = std::mem::take(&mut self.output);
            Ok(LoadingProgress::Ready((output,)))
        }
    }
}

#[derive(Clone, Debug)]
pub enum TextureSource {
    File { path: PathBuf },
    Channel { receiver: TextureReceiver },
}

impl From<PathBuf> for TextureSource {
    fn from(value: PathBuf) -> Self {
        Self::File { path: value }
    }
}

impl From<&Path> for TextureSource {
    fn from(value: &Path) -> Self {
        Self::from(PathBuf::from(value))
    }
}

impl From<&str> for TextureSource {
    fn from(value: &str) -> Self {
        Self::from(PathBuf::from(value))
    }
}

impl From<TextureReceiver> for TextureSource {
    fn from(value: TextureReceiver) -> Self {
        Self::Channel { receiver: value }
    }
}

#[derive(Clone, Debug)]
pub struct TextureAndView {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

impl TextureAndView {
    pub fn from_path<P>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: P,
    ) -> Result<Self, image::ImageError>
    where
        P: AsRef<Path>,
    {
        tracing::debug!(path = %path.as_ref().display(), "loading texture");
        let image = image::RgbaImage::from_path(path.as_ref())?;
        let label = path.as_ref().display().to_string();
        let texture = texture_from_image(device, queue, &image, &label);
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(&label),
            ..Default::default()
        });
        Ok(Self { texture, view })
    }
}
