use std::{
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use bevy_ecs::system::{
    ResMut,
    SystemParam,
};
use cem_util::wgpu::{
    MipLevels,
    UnsupportedColorSpace,
    create_texture_view_from_texture,
};

use crate::{
    renderer::Fallbacks,
    resource::RenderResourceManager,
    texture::{
        cache::{
            ImageInfo,
            TextureCache,
        },
        channel::TextureReceiver,
    },
};

pub mod cache;
pub mod channel;

#[derive(Debug, SystemParam)]
pub struct TextureLoaderContext<'w> {
    pub texture_cache: ResMut<'w, TextureCache>,
    pub render_resource_manager: RenderResourceManager<'w>,
}

impl<'w> TextureLoaderContext<'w> {
    pub fn load_texture_from_file<P>(
        &mut self,
        path: P,
        mip_levels: MipLevels,
    ) -> Result<(Arc<TextureAndView>, ImageInfo), TextureLoadError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        self.texture_cache.get_or_insert(path, || {
            tracing::debug!(path = %path.display(), ?mip_levels, "loading texture from file");

            let label = path.display().to_string();

            let image = image::ImageReader::open(path)?.decode()?;
            let original_color_type = image.color();
            let image = image.into_rgba8();

            let texture = self.render_resource_manager.create_texture_from_image(
                &label,
                &image,
                wgpu::TextureUsages::TEXTURE_BINDING,
                mip_levels,
            )?;

            Ok((
                TextureAndView::from_texture(texture, &label),
                ImageInfo {
                    original_color_type,
                },
            ))
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TextureLoadError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Image(#[from] image::ImageError),

    #[error(transparent)]
    UnsupportedColorspace(#[from] UnsupportedColorSpace),
}

#[derive(Clone, Debug)]
pub enum TextureSource {
    File {
        path: PathBuf,
        mip_levels: MipLevels,
    },
    Channel {
        receiver: TextureReceiver,
    },
}

impl TextureSource {
    pub fn from_path_with_mip_levels(path: impl Into<PathBuf>, mip_levels: MipLevels) -> Self {
        Self::File {
            path: path.into(),
            mip_levels,
        }
    }

    pub fn load(
        &self,
        context: &mut TextureLoaderContext,
    ) -> Result<LoadedTexture, TextureLoadError> {
        match self {
            TextureSource::File { path, mip_levels } => {
                let (texture_and_view, info) = context.load_texture_from_file(path, *mip_levels)?;

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
}

#[derive(Clone, Debug)]
pub struct LoadedTexture {
    pub texture_and_view: Arc<TextureAndView>,
    pub info: Option<ImageInfo>,
}

impl From<PathBuf> for TextureSource {
    fn from(value: PathBuf) -> Self {
        Self::File {
            path: value,
            mip_levels: MipLevels::One,
        }
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

#[derive(Clone, Debug, Default)]
pub enum Sampler {
    #[default]
    Clamp,
    Repeat,
    Custom(wgpu::Sampler),
}

impl Sampler {
    pub(crate) fn pick<'a>(&'a self, fallbacks: &'a Fallbacks) -> &'a wgpu::Sampler {
        match self {
            Sampler::Clamp => &fallbacks.sampler_clamp,
            Sampler::Repeat => &fallbacks.sampler_repeat,
            Sampler::Custom(sampler) => sampler,
        }
    }
}
