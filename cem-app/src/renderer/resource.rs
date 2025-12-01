use std::sync::Arc;

use cem_util::wgpu::{
    ImageTextureExt,
    UnsupportedColorSpace,
    create_texture,
    create_texture_from_linsrgba,
};
use nalgebra::Vector2;
use palette::LinSrgba;

use crate::{
    app::WgpuContext,
    renderer::{
        Renderer,
        command::CommandSender,
        material::TextureAndView,
        texture_channel::{
            TextureReceiver,
            UndecidedTextureSender,
            texture_channel,
        },
    },
};

#[derive(Clone, Debug)]
pub struct RenderResourceCreator {
    wgpu_context: WgpuContext,
    command_sender: CommandSender,
}

impl RenderResourceCreator {
    pub fn from_renderer(renderer: &Renderer) -> Self {
        Self {
            wgpu_context: renderer.wgpu_context.clone(),
            command_sender: renderer.command_queue.sender.clone(),
        }
    }

    pub fn create_texture(
        &self,
        size: &Vector2<u32>,
        usage: wgpu::TextureUsages,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> wgpu::Texture {
        create_texture(&self.wgpu_context.device, size, usage, format, label)
    }

    pub fn create_texture_from_image(
        &self,
        image: &image::RgbaImage,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> Result<wgpu::Texture, UnsupportedColorSpace> {
        // todo: batch these
        self.wgpu_context.with_staging(|write_staging| {
            image.create_texture(usage, label, &self.wgpu_context.device, write_staging)
        })
    }

    pub fn create_texture_from_color(
        &self,
        color: LinSrgba<u8>,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> wgpu::Texture {
        // todo: batch these
        self.wgpu_context.with_staging(|write_staging| {
            create_texture_from_linsrgba(
                color,
                usage,
                label,
                &self.wgpu_context.device,
                write_staging,
            )
        })
    }

    pub fn create_texture_channel(
        &self,
        size: &Vector2<u32>,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> (UndecidedTextureSender, TextureReceiver) {
        let texture = self.create_texture(size, usage, wgpu::TextureFormat::Rgba8Unorm, label);

        texture_channel(
            Arc::new(TextureAndView::from_texture(texture, label)),
            *size,
            self.command_sender.clone(),
        )
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.wgpu_context.device
    }
}
