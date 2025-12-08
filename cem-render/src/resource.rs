use std::{
    num::NonZero,
    path::Path,
    sync::Arc,
};

use bevy_ecs::{
    resource::Resource,
    system::{
        Res,
        ResMut,
        SystemParam,
    },
};
use cem_util::wgpu::{
    buffer::{
        SubmitOnDrop,
        WriteStaging,
        WriteStagingBelt,
        WriteStagingCommit,
        WriteStagingTransaction,
    },
    create_texture,
    create_texture_from_linsrgba,
    image::{
        ImageTextureExt,
        MipLevels,
        UnsupportedColorSpace,
    },
};
use nalgebra::Vector2;
use palette::LinSrgba;
use parking_lot::Mutex;

use crate::{
    command::CommandSender,
    renderer::{
        Renderer,
        SharedRenderer,
    },
    texture::{
        LoadedTexture,
        TextureLoadError,
        cache::{
            ImageInfo,
            TextureCache,
        },
        channel::{
            TextureReceiver,
            UndecidedTextureSender,
            texture_channel,
        },
        mipmap_cache::MipMapCache,
    },
};

#[derive(Debug, SystemParam)]
pub struct RenderResourceManager<'w> {
    renderer: Res<'w, SharedRenderer>,
    transaction: ResMut<'w, RenderResourceTransactionState>,
    command_sender: Res<'w, CommandSender>,
    texture_cache: Res<'w, TextureCache>,
    mipmap_cache: Option<Res<'w, SharedMipMapCache>>,
}

impl<'w> RenderResourceManager<'w> {
    pub fn device(&self) -> &wgpu::Device {
        &self.renderer.device
    }

    pub fn create_texture(
        &mut self,
        label: &str,
        size: &Vector2<u32>,
        usage: wgpu::TextureUsages,
        format: wgpu::TextureFormat,
        mip_level_count: NonZero<u32>,
    ) -> wgpu::Texture {
        create_texture(
            label,
            size,
            usage,
            format,
            mip_level_count,
            &self.renderer.device,
        )
    }

    pub fn create_texture_from_color(
        &mut self,
        color: LinSrgba<u8>,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> wgpu::Texture {
        self.transaction.with(&self.renderer, |transaction| {
            create_texture_from_linsrgba(
                color,
                usage,
                label,
                &self.renderer.device,
                &mut transaction.write_staging,
            )
        })
    }

    pub fn create_texture_channel(
        &mut self,
        size: &Vector2<u32>,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> (UndecidedTextureSender, TextureReceiver) {
        let texture = self.create_texture(
            label,
            size,
            usage,
            wgpu::TextureFormat::Rgba8Unorm,
            const { NonZero::new(1).unwrap() },
        );

        texture_channel(texture, *size, self.command_sender.clone())
    }

    // todo: if we want to use this somewhere we would likely want it to write the
    // image into all mip levels
    pub fn write_to_texture(&mut self, image: &image::RgbaImage, texture: &wgpu::Texture) {
        self.transaction.with(&self.renderer, |transaction| {
            image.write_to_texture(texture, &mut transaction.write_staging);
        });
    }

    pub fn as_async(&self) -> AsyncRenderResourceManager {
        AsyncRenderResourceManager {
            renderer: self.renderer.clone(),
            transaction: Default::default(),
            _command_sender: self.command_sender.clone(),
            texture_cache: self.texture_cache.clone(),
            mipmap_cache: self.mipmap_cache.as_deref().cloned(),
        }
    }
}

#[derive(Debug, Default, Resource)]
pub struct RenderResourceTransactionState(pub Option<RenderResourceTransaction>);

impl RenderResourceTransactionState {
    pub fn with<R>(
        &mut self,
        renderer: &Renderer,
        f: impl FnOnce(&mut RenderResourceTransaction) -> R,
    ) -> R {
        let transaction = self
            .0
            .get_or_insert_with(|| RenderResourceTransaction::new(renderer));

        f(transaction)
    }

    pub async fn with_async<R>(
        &mut self,
        renderer: &Renderer,
        f: impl AsyncFnOnce(&mut RenderResourceTransaction) -> R,
    ) -> R {
        let transaction = self
            .0
            .get_or_insert_with(|| RenderResourceTransaction::new(renderer));

        f(transaction).await
    }
}

#[derive(Debug)]
pub struct RenderResourceTransaction {
    pub write_staging: SubmitOnDrop<
        WriteStagingTransaction<WriteStagingBelt, wgpu::Device, wgpu::CommandEncoder>,
        wgpu::Queue,
    >,
}

impl RenderResourceTransaction {
    fn new(renderer: &Renderer) -> Self {
        Self {
            write_staging: SubmitOnDrop::new(
                WriteStagingTransaction::new(
                    renderer.staging_pool.belt(),
                    renderer.device.clone(),
                    renderer
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("render/resource_manager/transaction"),
                        }),
                ),
                renderer.queue.clone(),
            ),
        }
    }

    pub fn commit(self) {
        // this also submits it to the queue
        self.write_staging.commit();
    }

    pub fn discard(self) {
        self.write_staging.discard();
    }
}

#[derive(Debug)]
pub struct AsyncRenderResourceManager {
    renderer: SharedRenderer,
    transaction: RenderResourceTransactionState,
    _command_sender: CommandSender,
    texture_cache: TextureCache,
    mipmap_cache: Option<SharedMipMapCache>,
}

impl AsyncRenderResourceManager {
    // todo: have this return a stream so that we can yield partially loaded
    // textures (e.g. lowest mip-level) earlier.
    pub async fn load_texture_from_file<P>(
        &mut self,
        path: P,
        mip_levels: MipLevels,
    ) -> Result<LoadedTexture, TextureLoadError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let (texture, image_info) = self
            .texture_cache
            .get_or_insert(path, async || {
                tracing::debug!(path = %path.display(), ?mip_levels, "loading texture from file");

                let label = path.display().to_string();

                let image = image::ImageReader::open(path)?.decode()?;
                let original_color_type = image.color();
                let image = image.into_rgba8();

                let texture = self
                    .transaction
                    .with_async(&self.renderer, async |transaction| {
                        // pretend this is async lol
                        let texture = if let Some(mipmap_cache) = &self.mipmap_cache {
                            let mut mipmap_cache = mipmap_cache.0.lock();

                            create_texture_from_mipmap_cache(
                                &label,
                                &image,
                                &self.renderer.device,
                                &mut transaction.write_staging,
                                &mut mipmap_cache,
                            )?
                        }
                        else {
                            image.create_texture(
                                &label,
                                wgpu::TextureUsages::TEXTURE_BINDING,
                                mip_levels,
                                &self.renderer.device,
                                &mut transaction.write_staging,
                            )?
                        };

                        Ok::<_, TextureLoadError>(texture)
                    })
                    .await?;

                Ok::<_, TextureLoadError>((
                    texture,
                    ImageInfo {
                        original_color_type,
                    },
                ))
            })
            .await?;

        // if a fixed mip level count is specified, we use that. otherwise we use all
        // available mip levels
        let mut mip_level_count = mip_levels.fixed_mip_level_count();

        // check if the cached texture actually has enough mip levels
        // todo: if not we need to make more.
        if let Some(requested_mip_level_count) = mip_level_count
            && requested_mip_level_count.get() > texture.mip_level_count()
        {
            tracing::warn!(?requested_mip_level_count, cached_mip_level_count = ?texture.mip_level_count(), "todo: Cached texture's mip level count too low");
            mip_level_count = None;
        }

        // todo: label
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor {
            mip_level_count: mip_level_count.map(|mip_level_count| mip_level_count.get()),
            ..Default::default()
        });

        Ok(LoadedTexture {
            texture,
            texture_view,
            image_info: Some(image_info),
        })
    }

    pub async fn create_texture_from_image(
        &mut self,
        label: &str,
        image: &image::RgbaImage,
        usage: wgpu::TextureUsages,
        mip_levels: MipLevels,
    ) -> Result<wgpu::Texture, UnsupportedColorSpace> {
        self.transaction
            .with_async(&self.renderer, async |transaction| {
                // pretend this is async lol
                image.create_texture(
                    label,
                    usage,
                    mip_levels,
                    &self.renderer.device,
                    &mut transaction.write_staging,
                )
            })
            .await
    }
}

fn create_texture_from_mipmap_cache<S>(
    label: &str,
    base_image: &image::RgbaImage,
    device: &wgpu::Device,
    mut write_staging: S,
    mipmap_cache: &mut MipMapCache,
) -> Result<wgpu::Texture, TextureLoadError>
where
    S: WriteStaging,
{
    let texture_format = base_image.texture_format()?;

    Ok(mipmap_cache.create_texture(
        base_image,
        |mip_level_count, base_size| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: base_size.x,
                    height: base_size.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: mip_level_count.get(),
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: texture_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        },
        |texture, mip_level, mip_size, image| {
            image.write_to_texture_mip_level(texture, mip_level, mip_size, &mut write_staging)
        },
    )?)
}

// todo: we need proper multi-threading for this like we do for the texture
// cache
#[derive(Clone, Debug, Resource)]
pub(crate) struct SharedMipMapCache(Arc<Mutex<MipMapCache>>);

impl SharedMipMapCache {
    pub fn new(mipmap_cache: MipMapCache) -> Self {
        Self(Arc::new(Mutex::new(mipmap_cache)))
    }
}
