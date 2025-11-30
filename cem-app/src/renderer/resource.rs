use std::sync::Arc;

use cem_util::wgpu::{
    create_texture,
    create_texture_from_color,
    create_texture_from_image,
};
use nalgebra::Vector2;
use palette::Srgba;

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
        label: &str,
    ) -> wgpu::Texture {
        create_texture(&self.wgpu_context.device, size, usage, label)
    }

    pub fn create_texture_from_image(
        &self,
        image: &image::RgbaImage,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> wgpu::Texture {
        create_texture_from_image(
            &self.wgpu_context.device,
            &self.wgpu_context.queue,
            image,
            usage,
            label,
        )
    }

    pub fn create_texture_from_color(
        &self,
        color: &Srgba,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> wgpu::Texture {
        create_texture_from_color(
            &self.wgpu_context.device,
            &self.wgpu_context.queue,
            &color.into_format(),
            usage,
            label,
        )
    }

    pub fn create_texture_channel(
        &self,
        size: &Vector2<u32>,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> (UndecidedTextureSender, TextureReceiver) {
        let texture = self.create_texture(size, usage, label);

        texture_channel(
            Arc::new(TextureAndView::from_texture(texture, label)),
            *size,
            self.command_sender.clone(),
        )
    }

    /*
    pub fn load_texture(
        &mut self,
        texture_source: &mut TextureSource,
    ) -> Result<LoadingProgress<Arc<TextureAndView>>, Error> {
        match texture_source {
            TextureSource::File { path } => {
                let texture_and_view = self.texture_cache.get_or_insert(path, || {
                    Ok::<_, Error>(Arc::new(TextureAndView::from_path(
                        &self.wgpu_context.device,
                        &self.wgpu_context.queue,
                        &path,
                    )?))
                })?;
                Ok(LoadingProgress::Ready(texture_and_view))
            }
            TextureSource::Channel { receiver } => {
                let texture_and_view = receiver.register(
                    &self.command_sender,
                    wgpu::TextureFormat::Rgba8Unorm,
                    |size, label| create_texture(&self.wgpu_context.device, size, label),
                );
                Ok(texture_and_view.into())
            }
        }
    } */

    pub fn device(&self) -> &wgpu::Device {
        &self.wgpu_context.device
    }
}
