use std::{
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use bevy_ecs::component::Component;
use bitflags::bitflags;
use bytemuck::{
    Pod,
    Zeroable,
};
use cem_util::wgpu::create_texture_view_from_texture;
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
    composer::loader::{
        AndChanged,
        ImageInfo,
        LoadAsset,
        LoaderContext,
        LoadingProgress,
        LoadingState,
    },
    impl_register_component,
    renderer::texture_channel::TextureReceiver,
    util::{
        egui::probe::{
            HasChangeValue,
            PropertiesUi,
            TrackChanges,
            label_and_value,
            label_and_value_with_config,
            std::NumericPropertyUiConfig,
        },
        palette::ColorExt,
    },
};

pub mod presets {
    #![allow(clippy::all)]

    use palette::{
        Srgb,
        WithAlpha,
    };
    pub use pbr_presets::*;

    use crate::renderer::material::Material;

    impl From<MaterialPreset> for Material {
        fn from(value: MaterialPreset) -> Self {
            Material {
                albedo: Srgb::from_linear(value.albedo).with_alpha(1.0),
                metalness: value.metalness,
                roughness: value.roughness,
                ..Default::default()
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Component)]
pub struct Material {
    #[serde(with = "crate::util::serde::palette")]
    pub albedo: Srgba,

    pub metalness: f32,
    pub roughness: f32,
    pub ambient_occlusion: f32,

    pub transparent: bool,
    pub alpha_threshold: f32,

    pub shading: bool,
    pub tone_map: bool,
    pub gamma: bool,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            albedo: Srgba::WHITE,
            metalness: 0.0,
            roughness: 1.0,
            ambient_occlusion: 1.0,
            transparent: false,
            alpha_threshold: 0.0,
            shading: true,
            tone_map: true,
            gamma: true,
        }
    }
}

impl Material {
    pub fn from_albedo<C>(color: C) -> Self
    where
        Srgba: From<C>,
    {
        let albedo = color.into();
        Self {
            albedo,
            transparent: albedo.alpha < 1.0,
            ..Default::default()
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

    pub fn with_shading(mut self, enable: bool) -> Self {
        self.shading = enable;
        self
    }

    pub fn with_tone_map(mut self, enable: bool) -> Self {
        self.tone_map = enable;
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

impl PropertiesUi for Material {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                let id: egui::Id = ui.id().with("material_preset");

                #[derive(Clone, Copy, Default, PartialEq, Eq)]
                struct SelectedPreset(Option<usize>);
                let selection_id = id.with("selection");
                let mut selected_preset = ui.data(|data| {
                    data.get_temp::<SelectedPreset>(selection_id)
                        .unwrap_or_default()
                });
                let selected_before = selected_preset;

                ui.horizontal(|ui| {
                    ui.label("Presets");

                    egui::ComboBox::from_id_salt(id.with("combo_box"))
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
                    ui.data_mut(|ui| ui.insert_temp(selection_id, selected_preset));
                    if let Some(i) = selected_preset.0 {
                        *self = (*presets::ALL[i]).into();
                    }
                }

                label_and_value(ui, "Albedo", &mut changes, &mut self.albedo);
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
                label_and_value_with_config(
                    ui,
                    "Ambient Occlusion",
                    &mut changes,
                    &mut self.ambient_occlusion,
                    &NumericPropertyUiConfig::Slider { range: 0.0..=1.0 },
                );
                label_and_value(ui, "Transparent", &mut changes, &mut self.transparent);
                label_and_value_with_config(
                    ui,
                    "Alpha Threshold",
                    &mut changes,
                    &mut self.alpha_threshold,
                    &NumericPropertyUiConfig::Slider { range: 0.0..=1.0 },
                );
                label_and_value(ui, "Shading", &mut changes, &mut self.shading);
                label_and_value(ui, "Tone Map", &mut changes, &mut self.tone_map);
                label_and_value(ui, "Gamma", &mut changes, &mut self.gamma);

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

impl_register_component!(Material where Changed, ComponentUi, default);

#[derive(Clone, Copy, Debug, Component)]
pub struct Wireframe {
    pub color: Srgba,
}

impl Wireframe {
    pub fn new<C>(color: C) -> Self
    where
        Srgba: From<C>,
    {
        Self {
            color: color.into(),
        }
    }
}

impl Default for Wireframe {
    fn default() -> Self {
        Self::new(Srgba::BLACK)
    }
}

impl PropertiesUi for Wireframe {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        self.color.properties_ui(ui, &Default::default())
    }
}

impl_register_component!(Wireframe where Changed, ComponentUi, default);

#[derive(Clone, Debug, Component)]
pub struct AlbedoTexture {
    pub texture: Arc<TextureAndView>,
    pub transparent: bool,
}

/// Combined ambient occlusion, roughness, metalness map
#[derive(Clone, Debug, Component)]
pub struct MaterialTexture {
    pub texture: Arc<TextureAndView>,
    pub flags: MaterialTextureFlags,
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    pub struct MaterialTextureFlags: u32 {
        const METALNESS           = 0x0000_0002;
        const ROUGHNESS           = 0x0000_0004;
        const AMBIENT_OCCLUSION   = 0x0000_0008;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    struct MaterialFlags: u32 {
        const ALBEDO_TEXTURE      = 0x0000_0001;
        const TRANSPARENT         = 0x0000_0010;
        const SHADING             = 0x0000_0020;
        const TONE_MAP            = 0x0000_0040;
        const GAMMA               = 0x0000_0080;
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
        wireframe: Option<&Wireframe>,
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

        if let Some(material_texture) = material_texture {
            data.flags |= material_texture.flags.bits();
        }

        if let Some(material) = material {
            data.albedo = material.albedo.into_linear();
            data.alpha_threshold = material.alpha_threshold;

            if material.transparent {
                data.flags |= MaterialFlags::TRANSPARENT.bits();
            }
            if material.shading {
                data.flags |= MaterialFlags::SHADING.bits();
            }
            if material.tone_map {
                data.flags |= MaterialFlags::TONE_MAP.bits();
            }
            if material.gamma {
                data.flags |= MaterialFlags::GAMMA.bits();
            }
        }

        if let Some(wireframe) = wireframe {
            data.wireframe = wireframe.color.into_linear();
        }

        // default values
        macro_rules! material {
            {$(($name:ident, $flag:ident, $default:expr);)*} => {{
                if let Some(material) = material {
                    $(data.$name = material.$name;)*
                }
                else {
                    let texture_flags = material_texture.as_ref().map(|material_texture| material_texture.flags).unwrap_or_default();
                    $(data.$name = if texture_flags.contains(MaterialTextureFlags::$flag) { 1.0 } else { $default };)*
                }
            }};
        }

        material! {
            (metalness, METALNESS, 0.0);
            (roughness, ROUGHNESS, 0.0);
            (ambient_occlusion, AMBIENT_OCCLUSION, 1.0);
        }

        data
    }
}

#[derive(Clone, Debug, Component)]
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
        let loaded_texture = self.source.load_with(context)?;

        let transparent = self
            .transparency
            .or_else(|| {
                loaded_texture
                    .info
                    .map(|info| info.original_color_type.has_alpha())
            })
            .unwrap_or_default();

        Ok(LoadingProgress::Ready(
            AlbedoTexture {
                texture: loaded_texture.texture_and_view,
                transparent,
            }
            .into(),
        ))
    }
}

#[derive(Clone, Debug, Component)]
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
        let loaded_texture = self.source.load(context)?;

        Ok(LoadingProgress::Ready(
            MaterialTexture {
                texture: loaded_texture.texture_and_view,
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
    pub fn load_with(&self, context: &mut LoaderContext) -> Result<LoadedTexture, Error> {
        match self {
            TextureSource::File { path } => {
                // todo: this should not be implied here
                let usage = wgpu::TextureUsages::TEXTURE_BINDING;

                let (texture_and_view, info) = context.load_texture_from_file(path, usage)?;

                Ok(LoadedTexture {
                    texture_and_view,
                    info: Some(info),
                })
            }
            TextureSource::Channel { receiver } => {
                Ok(LoadedTexture {
                    texture_and_view: receiver.inner.clone(),
                    info: None,
                })
            }
        }
    }

    pub fn load(&self, context: &mut LoaderContext) -> Result<LoadedTexture, Error> {
        self.load_with(context)
    }
}

#[derive(Clone, Debug)]
pub struct LoadedTexture {
    texture_and_view: Arc<TextureAndView>,
    info: Option<ImageInfo>,
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
