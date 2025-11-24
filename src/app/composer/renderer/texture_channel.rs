use std::{
    ops::{
        Deref,
        DerefMut,
    },
    sync::Arc,
};

use image::RgbaImage;
use nalgebra::Vector2;
use parking_lot::{
    RwLock,
    RwLockWriteGuard,
};

use crate::app::composer::renderer::{
    command::CommandSender,
    light::TextureAndView,
};

pub(super) fn texture_channel(
    texture_and_view: Arc<TextureAndView>,
    size: Vector2<u32>,
    command_sender: CommandSender,
) -> (UndecidedTextureSender, TextureReceiver) {
    let shared = Arc::new(Shared {
        texture_and_view: texture_and_view.clone(),
        size,
        image_buffer: RwLock::new(None),
        command_sender,
    });

    let sender = UndecidedTextureSender { shared };
    let receiver = TextureReceiver {
        inner: texture_and_view,
    };
    (sender, receiver)
}

#[derive(Clone, Debug)]
pub struct TextureReceiver {
    pub(super) inner: Arc<TextureAndView>,
}

#[derive(Debug)]
pub(super) struct CopyImageToTextureCommand {
    shared: Arc<Shared>,
}

impl CopyImageToTextureCommand {
    pub fn handle(&self, copy_image_to_texture: impl FnOnce(&image::RgbaImage, &wgpu::Texture)) {
        let mut image_buffer = self.shared.image_buffer.write();
        let image_buffer = image_buffer
            .as_mut()
            .expect("copy-image-to-texture command without image buffer");

        if image_buffer.dirty {
            image_buffer.dirty = false;

            copy_image_to_texture(&image_buffer.buffer, &self.shared.texture_and_view.texture);
        }
    }
}

#[derive(Debug)]
pub struct UndecidedTextureSender {
    shared: Arc<Shared>,
}

impl UndecidedTextureSender {
    pub fn send_images(self) -> ImageSender {
        {
            let mut image_buffer = self.shared.image_buffer.write();
            assert!(image_buffer.is_none(), "image buffer already present");
            *image_buffer = Some(ImageBuffer {
                buffer: RgbaImage::new(self.shared.size.x, self.shared.size.y),
                dirty: false,
            });
        }

        ImageSender {
            shared: self.shared,
        }
    }

    pub fn send_texture(self) -> TextureSender {
        let texture_and_view = self.shared.texture_and_view.clone();
        let format = texture_and_view.texture.format();
        TextureSender {
            texture_and_view,
            size: self.shared.size,
            format,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextureSender {
    pub texture_and_view: Arc<TextureAndView>,
    pub size: Vector2<u32>,
    pub format: wgpu::TextureFormat,
}

#[derive(Debug)]
pub struct ImageSender {
    shared: Arc<Shared>,
}

impl ImageSender {
    pub fn update_image(&mut self) -> ImageGuard<'_> {
        let mut image_buffer = self.shared.image_buffer.write();
        let dirty_before = image_buffer
            .as_mut()
            .expect("no image buffer in image sender")
            .dirty;

        ImageGuard {
            shared: &self.shared,
            image_buffer,
            dirty_before,
        }
    }

    pub fn size(&self) -> Vector2<u32> {
        self.shared.size
    }
}

#[derive(Debug)]
pub struct ImageGuard<'a> {
    shared: &'a Arc<Shared>,
    image_buffer: RwLockWriteGuard<'a, Option<ImageBuffer>>,
    dirty_before: bool,
}

impl<'a> Drop for ImageGuard<'a> {
    fn drop(&mut self) {
        let image_buffer = self
            .image_buffer
            .as_mut()
            .expect("no image buffer in image sender");
        if !self.dirty_before && image_buffer.dirty {
            self.shared.command_sender.send(CopyImageToTextureCommand {
                shared: self.shared.clone(),
            });
        }
    }
}

impl<'a> Deref for ImageGuard<'a> {
    type Target = image::RgbaImage;

    fn deref(&self) -> &Self::Target {
        let image_buffer = self
            .image_buffer
            .as_ref()
            .expect("no image buffer in image sender");
        &image_buffer.buffer
    }
}

impl<'a> DerefMut for ImageGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let image_buffer = self
            .image_buffer
            .as_mut()
            .expect("no image buffer in image sender");
        image_buffer.dirty = true;
        &mut image_buffer.buffer
    }
}

#[derive(Debug)]
struct Shared {
    texture_and_view: Arc<TextureAndView>,
    size: Vector2<u32>,
    command_sender: CommandSender,
    image_buffer: RwLock<Option<ImageBuffer>>,
}

#[derive(Debug)]
struct ImageBuffer {
    buffer: image::RgbaImage,
    dirty: bool,
}
