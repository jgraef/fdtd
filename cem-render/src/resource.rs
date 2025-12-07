use std::{
    num::NonZero,
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
    ImageTextureExt,
    MipLevels,
    UnsupportedColorSpace,
    buffer::{
        SubmitOnDrop,
        WriteStagingBelt,
        WriteStagingCommit,
        WriteStagingTransaction,
    },
    create_texture,
    create_texture_from_linsrgba,
};
use nalgebra::Vector2;
use palette::LinSrgba;

use crate::{
    command::CommandSender,
    renderer::{
        Renderer,
        SharedRenderer,
    },
    texture::{
        TextureAndView,
        channel::{
            TextureReceiver,
            UndecidedTextureSender,
            texture_channel,
        },
    },
};

#[derive(Debug, SystemParam)]
pub struct RenderResourceManager<'w> {
    renderer: Res<'w, SharedRenderer>,
    transaction: ResMut<'w, RenderResourceTransactionState>,
    command_sender: Res<'w, CommandSender>,
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

    pub fn create_texture_from_image(
        &mut self,
        label: &str,
        image: &image::RgbaImage,
        usage: wgpu::TextureUsages,
        mip_levels: MipLevels,
    ) -> Result<wgpu::Texture, UnsupportedColorSpace> {
        self.transaction.with(&self.renderer, |transaction| {
            image.create_texture(
                label,
                usage,
                mip_levels,
                &self.renderer.device,
                &mut transaction.write_staging,
            )
        })
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

        texture_channel(
            Arc::new(TextureAndView::from_texture(texture, label)),
            *size,
            self.command_sender.clone(),
        )
    }

    // todo: if we want to use this somewhere we would likely want it to write the
    // image into all mip levels
    pub fn write_to_texture(&mut self, image: &image::RgbaImage, texture: &wgpu::Texture) {
        self.transaction.with(&self.renderer, |transaction| {
            image.write_to_texture(texture, &mut transaction.write_staging);
        });
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
