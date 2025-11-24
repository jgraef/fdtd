use nalgebra::Vector2;
use palette::Srgba;

use crate::{
    app::{
        composer::renderer::{
            Renderer,
            command::CommandSender,
            texture_channel::{
                TextureReceiver,
                TextureSender,
            },
        },
        start::WgpuContext,
    },
    util::wgpu::{
        create_texture,
        create_texture_from_color,
        create_texture_from_image,
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

    pub fn create_texture(&self, size: &Vector2<u32>, label: &str) -> wgpu::Texture {
        create_texture(&self.wgpu_context.device, size, label)
    }

    pub fn create_texture_from_image(
        &self,
        image: &image::RgbaImage,
        label: &str,
    ) -> wgpu::Texture {
        create_texture_from_image(
            &self.wgpu_context.device,
            &self.wgpu_context.queue,
            image,
            label,
        )
    }

    pub fn create_texture_from_color(&self, color: &Srgba, label: &str) -> wgpu::Texture {
        create_texture_from_color(
            &self.wgpu_context.device,
            &self.wgpu_context.queue,
            &color.into_format(),
            label,
        )
    }

    pub fn create_texture_channel(
        &self,
        _size: &Vector2<u32>,
        _label: &str,
    ) -> (TextureSender, TextureReceiver) {
        todo!();
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
