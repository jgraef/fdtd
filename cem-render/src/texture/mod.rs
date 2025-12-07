use std::{
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use cem_util::wgpu::image::{
    MipLevels,
    UnsupportedColorSpace,
};

use crate::{
    renderer::Fallbacks,
    resource::AsyncRenderResourceManager,
    texture::{
        cache::ImageInfo,
        channel::TextureReceiver,
    },
};

pub mod cache;
pub mod channel;

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

    pub async fn load(
        &self,
        mut render_resource_manager: AsyncRenderResourceManager,
    ) -> Result<LoadedTexture, TextureLoadError> {
        match self {
            TextureSource::File { path, mip_levels } => {
                render_resource_manager
                    .load_texture_from_file(path, *mip_levels)
                    .await
            }
            TextureSource::Channel { receiver } => {
                let texture = Arc::new(receiver.inner.clone());
                let texture_view = texture.create_view(&Default::default());
                Ok(LoadedTexture {
                    texture,
                    texture_view,
                    image_info: None,
                })
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoadedTexture {
    pub texture: Arc<wgpu::Texture>,
    pub texture_view: wgpu::TextureView,
    pub image_info: Option<ImageInfo>,
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
pub enum Sampler {
    NearestClamp,
    LinearClamp,
    LinearRepeat,
    Custom(wgpu::Sampler),
}

impl Sampler {
    pub(crate) fn pick<'a>(&'a self, fallbacks: &'a Fallbacks) -> &'a wgpu::Sampler {
        match self {
            Sampler::NearestClamp => &fallbacks.sampler_nearest_clamp,
            Sampler::LinearClamp => &fallbacks.sampler_linear_clamp,
            Sampler::LinearRepeat => &fallbacks.sampler_linear_repeat,
            Sampler::Custom(sampler) => sampler,
        }
    }
}
