use std::{
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use bitflags::bitflags;
use bytemuck::{
    Pod,
    Zeroable,
};
use egui::Id;
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
        loader::{
            AndChanged,
            LoadAsset,
            LoaderContext,
            LoadingProgress,
            LoadingState,
            PreprocessImageInfo,
        },
        properties::{
            HasChangeValue,
            PropertiesUi,
            TrackChanges,
            label_and_value,
            label_and_value_with_config,
            std::NumericPropertyUiConfig,
        },
        renderer::texture_channel::TextureReceiver,
        scene::ui::ComponentUiHeading,
    },
    util::{
        palette::ColorExt,
        wgpu::create_texture_view_from_texture,
    },
};

pub mod presets {
    #![allow(clippy::all)]

    use palette::{
        Srgb,
        Srgba,
        WithAlpha,
    };
    pub use pbr_presets::*;

    use crate::{
        app::composer::renderer::material::Material,
        util::palette::ColorExt as _,
    };

    impl From<MaterialPreset> for Material {
        fn from(value: MaterialPreset) -> Self {
            Material {
                wireframe: Srgba::BLACK,
                albedo: Srgb::from_linear(value.albedo).with_alpha(1.0),
                metalness: value.metalness,
                roughness: value.roughness,
                ambient_occlusion: 1.0,
                transparent: false,
                alpha_threshold: 0.0,
            }
        }
    }
}

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
    pub wireframe: Srgba,

    #[serde(with = "crate::util::serde::palette")]
    pub albedo: Srgba,

    pub metalness: f32,
    pub roughness: f32,
    pub ambient_occlusion: f32,

    pub transparent: bool,
    pub alpha_threshold: f32,
}

impl Material {
    pub fn from_albedo<C>(color: C) -> Self
    where
        Srgba: From<C>,
    {
        let albedo: Srgba = color.into();
        let transparent = albedo.alpha < 1.0;
        Self {
            wireframe: Srgba::BLACK,
            albedo,
            metalness: 0.0,
            roughness: 0.0,
            ambient_occlusion: 1.0,
            transparent,
            alpha_threshold: 0.0,
        }
    }

    pub fn with_albedo(mut self, albedo: Srgba) -> Self {
        self.albedo = albedo;
        self
    }

    pub fn with_metalness(mut self, metalness: f32) -> Self {
        self.metalness = metalness;
        self
    }

    pub fn with_roughness(mut self, roughness: f32) -> Self {
        self.roughness = roughness;
        self
    }

    pub fn with_transparency(mut self, enable: bool) -> Self {
        self.transparent = enable;
        self
    }
}

impl From<Srgba> for Material {
    fn from(value: Srgba) -> Self {
        Self::from_albedo(value)
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

impl ComponentUiHeading for Material {
    fn heading(&self) -> impl Into<egui::RichText> {
        "Material"
    }
}

impl PropertiesUi for Material {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                #[derive(Clone, Copy, Default, PartialEq, Eq)]
                struct SelectedPreset(Option<usize>);
                let mut selected_preset = ui.data(|data| {
                    data.get_temp::<SelectedPreset>(Id::NULL)
                        .unwrap_or_default()
                });
                let selected_before = selected_preset;

                ui.horizontal(|ui| {
                    ui.label("Presets");

                    egui::ComboBox::from_id_salt("material_preset")
                        .selected_text(
                            selected_preset
                                .0
                                .map(|i| presets::ALL[i].name)
                                .unwrap_or_default(),
                        )
                        .show_ui(ui, |ui| {
                            for (i, preset) in presets::ALL.iter().enumerate() {
                                ui.selectable_value(
                                    &mut selected_preset,
                                    SelectedPreset(Some(i)),
                                    preset.name,
                                );
                            }
                        });
                });

                if selected_before != selected_preset {
                    ui.data_mut(|ui| ui.insert_temp(Id::NULL, selected_preset));
                    if let Some(i) = selected_preset.0 {
                        *self = (*presets::ALL[i]).into();
                    }
                }

                label_and_value(ui, "Wireframe", &mut changes, &mut self.wireframe);
                if self.transparent {
                    label_and_value(ui, "Albedo", &mut changes, &mut self.albedo);
                }
                else {
                    label_and_value(ui, "Albedo", &mut changes, &mut self.albedo.color);
                }

                label_and_value_with_config(
                    ui,
                    "Metallic",
                    &mut changes,
                    &mut self.metalness,
                    &NumericPropertyUiConfig::Slider { range: 0.0..=1.0 },
                );
                label_and_value_with_config(
                    ui,
                    "Roughness",
                    &mut changes,
                    &mut self.roughness,
                    &NumericPropertyUiConfig::Slider { range: 0.0..=1.0 },
                );
                label_and_value(ui, "Transparent", &mut changes, &mut self.transparent);

                if changes.changed {
                    // invalidate preset?
                }

                // also track preset change
                if selected_before != selected_preset {
                    changes.mark_changed();
                }
            })
            .response;

        changes.propagated(response)
    }
}

#[derive(Clone, Debug)]
pub struct AlbedoTexture {
    pub texture: Arc<TextureAndView>,
    pub transparent: bool,
}

/// Combined ambient occlusion, roughness, metalness map
#[derive(Clone, Debug)]
pub struct MaterialTexture {
    pub texture: Arc<TextureAndView>,
    pub flags: MaterialTextureFlags,
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Pod, Zeroable)]
    #[repr(C)]
    pub struct MaterialTextureFlags: u32 {
        const METALLIC          = 0x0000_0002;
        const ROUGHNESS         = 0x0000_0004;
        const AMBIENT_OCCLUSION = 0x0000_0008;
    }
}

bitflags! {
    pub struct MaterialFlags: u32 {
        const ALBEDO_TEXTURE      = 0x0000_0001;
        const TRANSPARENT         = 0x0000_0010;
    }
}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
pub(super) struct MaterialData {
    wireframe: LinSrgba,
    edges: LinSrgba,
    albedo: LinSrgba,
    metalness: f32,
    roughness: f32,
    ambient_occlusion: f32,
    // combined from MaterialFlags and MaterialTextureFlags
    flags: u32,
    alpha_threshold: f32,
    _padding: [u32; 3],
}

impl MaterialData {
    pub fn new(
        material: Option<&Material>,
        albedo_texture: Option<&AlbedoTexture>,
        material_texture: Option<&MaterialTexture>,
    ) -> Self {
        let mut data = Self {
            wireframe: LinSrgba::BLACK,
            ..Default::default()
        };

        if let Some(albedo_texture) = albedo_texture {
            // if a texture is present, a non-exitent material will yield white,
            // such that it doesn't affect the color output
            data.albedo = LinSrgba::WHITE;
            data.flags |= MaterialFlags::ALBEDO_TEXTURE.bits();
            if albedo_texture.transparent {
                data.flags |= MaterialFlags::TRANSPARENT.bits();
            }
        }
        else {
            // if a texture isn't present, it will default to a white texture, but
            // because a material is also not present, we don't want any color, so
            // we set it to black.
            data.albedo = LinSrgba::BLACK;
        }

        if let Some(material) = material {
            data.wireframe = material.wireframe.into_linear();
            data.albedo = material.albedo.into_linear();
            data.alpha_threshold = material.alpha_threshold;
            if material.transparent {
                data.flags |= MaterialFlags::TRANSPARENT.bits();
            }
        }

        if let Some(material_texture) = material_texture {
            data.flags |= material_texture.flags.bits();
        }

        macro_rules! material {
            ($name:ident, $flag:ident, $default:expr) => {
                data.$name = material.as_ref().map_or_else(
                    || {
                        let texture_present =
                            material_texture.as_ref().map_or(false, |material_texture| {
                                material_texture.flags.contains(MaterialTextureFlags::$flag)
                            });
                        if texture_present {
                            // if this value is present in the material texture we want it unchanged
                            1.0
                        }
                        else {
                            // if the texture also doesn't have this, we set a default
                            $default
                        }
                    },
                    |material| material.$name,
                );
            };
        }

        material!(metalness, METALLIC, 0.0);
        material!(roughness, ROUGHNESS, 0.0);
        material!(ambient_occlusion, AMBIENT_OCCLUSION, 1.0);

        data
    }
}

#[derive(Clone, Debug)]
pub struct LoadAlbedoTexture {
    pub source: TextureSource,
    pub transparency: Option<bool>,
}

impl LoadAlbedoTexture {
    pub fn new(source: impl Into<TextureSource>) -> Self {
        Self {
            source: source.into(),
            transparency: None,
        }
    }

    pub fn with_transparency(mut self, enable: bool) -> Self {
        self.transparency = Some(enable);
        self
    }
}

impl From<TextureSource> for LoadAlbedoTexture {
    fn from(value: TextureSource) -> Self {
        Self::new(value)
    }
}

impl LoadAsset for LoadAlbedoTexture {
    type State = Self;

    fn start_loading(&self, context: &mut LoaderContext) -> Result<Self, Error> {
        let _ = context;
        Ok(self.clone())
    }
}

impl LoadingState for LoadAlbedoTexture {
    type Output = AndChanged<AlbedoTexture>;

    fn poll(
        &mut self,
        context: &mut LoaderContext,
    ) -> Result<LoadingProgress<AndChanged<AlbedoTexture>>, Error> {
        let texture = self.source.load_with(context, |_image, info| {
            if self.transparency.is_none() {
                if info.original_color_type.has_alpha() {
                    // is scanning the whole image worth it? the artist should just save it without
                    // alpha channel self.transparency =
                    // Some(scan_image_for_alpha_pixels(image));
                    self.transparency = Some(true);
                }
                else {
                    self.transparency = Some(false)
                };
            }
            Ok(())
        })?;

        let transparency = self
            .transparency
            .expect("bug: transparency should have been determined by now");

        Ok(LoadingProgress::Ready(
            AlbedoTexture {
                texture,
                transparent: transparency,
            }
            .into(),
        ))
    }
}

fn scan_image_for_alpha_pixels(image: &image::RgbaImage) -> bool {
    let check = |pixel: &image::Rgba<u8>| -> bool { pixel.0[3] < 0xff };

    #[cfg(feature = "rayon")]
    {
        use rayon::iter::ParallelIterator;
        image.par_pixels().any(check)
    }

    #[cfg(not(feature = "rayon"))]
    {
        image.pixels().any(check)
    }
}

#[derive(Clone, Debug)]
pub struct LoadMaterialTexture {
    pub source: TextureSource,
    pub flags: MaterialTextureFlags,
}

impl LoadMaterialTexture {
    pub fn new(source: impl Into<TextureSource>, flags: MaterialTextureFlags) -> Self {
        Self {
            source: source.into(),
            flags,
        }
    }
}

impl LoadAsset for LoadMaterialTexture {
    type State = Self;

    fn start_loading(&self, context: &mut LoaderContext) -> Result<Self, Error> {
        let _ = context;
        Ok(self.clone())
    }
}

impl LoadingState for LoadMaterialTexture {
    type Output = AndChanged<MaterialTexture>;

    fn poll(
        &mut self,
        context: &mut LoaderContext,
    ) -> Result<LoadingProgress<AndChanged<MaterialTexture>>, Error> {
        Ok(LoadingProgress::Ready(
            MaterialTexture {
                texture: self.source.load(context)?,
                flags: self.flags,
            }
            .into(),
        ))
    }
}

#[derive(Clone, Debug)]
pub enum TextureSource {
    File { path: PathBuf },
    Channel { receiver: TextureReceiver },
}

impl TextureSource {
    pub fn load_with<F>(
        &self,
        context: &mut LoaderContext,
        preprocess_image: F,
    ) -> Result<Arc<TextureAndView>, Error>
    where
        F: FnMut(&mut image::RgbaImage, &PreprocessImageInfo) -> Result<(), Error>,
    {
        match self {
            TextureSource::File { path } => {
                // todo: this should not be implied here
                let usage = wgpu::TextureUsages::TEXTURE_BINDING;

                context.load_texture_from_file(path, usage, preprocess_image)
            }
            TextureSource::Channel { receiver } => Ok(receiver.inner.clone()),
        }
    }

    pub fn load(&self, context: &mut LoaderContext) -> Result<Arc<TextureAndView>, Error> {
        self.load_with(context, |_, _| Ok(()))
    }
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
    pub fn from_texture(texture: wgpu::Texture, label: &str) -> Self {
        let view = create_texture_view_from_texture(&texture, label);
        Self { texture, view }
    }
}
