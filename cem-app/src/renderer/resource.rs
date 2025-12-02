use cem_util::wgpu::{
    ImageTextureExt,
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
    app::WgpuContext,
    renderer::{
        renderer::Renderer,
        texture_channel::{
            TextureReceiver,
            UndecidedTextureSender,
        },
    },
};

#[derive(Clone, Debug)]
pub struct RenderResourceManager {
    wgpu_context: WgpuContext,
    // todo: bevy-migrate
    //command_sender: CommandSender,
}

impl RenderResourceManager {
    pub(super) fn from_renderer(renderer: &Renderer) -> Self {
        Self {
            wgpu_context: renderer.wgpu_context.clone(),
            //command_sender: renderer.command_queue.sender.clone(),
        }
    }

    pub fn begin_transaction(&self) -> RenderResourceManagerTransaction<'_> {
        RenderResourceManagerTransaction {
            device: &self.wgpu_context.device,
            //command_sender: &self.command_sender,
            write_staging: SubmitOnDrop::new(
                WriteStagingTransaction::new(
                    self.wgpu_context.staging_pool.belt(),
                    &self.wgpu_context.device,
                    self.wgpu_context.device.create_command_encoder(
                        &wgpu::CommandEncoderDescriptor {
                            label: Some("render/resource_manager/transaction"),
                        },
                    ),
                ),
                &self.wgpu_context.queue,
            ),
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.wgpu_context.device
    }
}

#[derive(Debug)]
pub struct RenderResourceManagerTransaction<'a> {
    // note: this is also in `write_staging`, but we can't borrow it whole also mut-borrrowing the
    // whole `write_staging` field.
    device: &'a wgpu::Device,
    // todo: bevy-migrate
    //command_sender: &'a CommandSender,
    write_staging: SubmitOnDrop<
        WriteStagingTransaction<WriteStagingBelt, &'a wgpu::Device, wgpu::CommandEncoder>,
        &'a wgpu::Queue,
    >,
}

impl<'a> RenderResourceManagerTransaction<'a> {
    pub fn create_texture(
        &mut self,
        size: &Vector2<u32>,
        usage: wgpu::TextureUsages,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> wgpu::Texture {
        create_texture(size, usage, format, label, self.device)
    }

    pub fn create_texture_from_image(
        &mut self,
        image: &image::RgbaImage,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> Result<wgpu::Texture, UnsupportedColorSpace> {
        // todo: batch staging
        image.create_texture(usage, label, self.device, &mut self.write_staging)
    }

    pub fn create_texture_from_color(
        &mut self,
        color: LinSrgba<u8>,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> wgpu::Texture {
        // todo: batch staging

        create_texture_from_linsrgba(color, usage, label, self.device, &mut self.write_staging)
    }

    pub fn create_texture_channel(
        &mut self,
        size: &Vector2<u32>,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> (UndecidedTextureSender, TextureReceiver) {
        // todo: bevy-migrate

        /*let texture = self.create_texture(size, usage, wgpu::TextureFormat::Rgba8Unorm, label);

        texture_channel(
            Arc::new(TextureAndView::from_texture(texture, label)),
            *size,
            self.command_sender.clone(),
        )*/
        todo!();
    }

    pub fn device(&self) -> &wgpu::Device {
        self.device
    }

    pub fn commit(self) {
        // this also submits it to the queue
        self.write_staging.commit();
    }

    pub fn discard(self) {
        self.write_staging.discard();
    }
}
