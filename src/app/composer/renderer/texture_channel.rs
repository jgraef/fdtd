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
    let state = Arc::new(RwLock::new(State {
        texture_and_view: texture_and_view.clone(),
        size,
        image_buffer: None,
        image_dirty: false,
        command_sender,
    }));

    let sender = UndecidedTextureSender {
        state: state.clone(),
    };
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
    state: Arc<RwLock<State>>,
}

impl CopyImageToTextureCommand {
    pub fn handle(&self, copy_image_to_texture: impl FnOnce(&image::RgbaImage, &wgpu::Texture)) {
        let mut state = self.state.write();
        if state.image_dirty {
            state.image_dirty = false;

            // this is necessary to not borrow twice through the Deref impl of the
            // RwLockWriteGuard
            let state = &mut *state;

            let image = state
                .image_buffer
                .as_mut()
                .expect("received copy-image-to-texture command but no image set");

            copy_image_to_texture(image, &state.texture_and_view.texture);
        }
    }
}

#[derive(Debug)]
pub struct UndecidedTextureSender {
    state: Arc<RwLock<State>>,
}

impl UndecidedTextureSender {
    pub fn send_images(self) -> ImageSender {
        {
            let mut state = self.state.write();
            let size = state.size;
            state.image_buffer = Some(RgbaImage::new(size.x, size.y));
        }

        ImageSender { state: self.state }
    }

    pub fn send_texture(self) -> TextureSender {
        let state = self.state.read();

        let texture_and_view = state.texture_and_view.clone();
        let size = state.size;
        let format = texture_and_view.texture.format();

        TextureSender {
            texture_and_view,
            size,
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
    state: Arc<RwLock<State>>,
}

impl ImageSender {
    pub fn update_image(&mut self) -> ImageGuard<'_> {
        let state = self.state.write();
        let dirty_before = state.image_dirty;

        ImageGuard {
            state_shared: &self.state,
            state_guard: state,
            dirty_before,
        }
    }
}

#[derive(Debug)]
pub struct ImageGuard<'a> {
    state_shared: &'a Arc<RwLock<State>>,
    state_guard: RwLockWriteGuard<'a, State>,
    dirty_before: bool,
}

impl<'a> Drop for ImageGuard<'a> {
    fn drop(&mut self) {
        if !self.dirty_before && self.state_guard.image_dirty {
            self.state_guard
                .command_sender
                .send(CopyImageToTextureCommand {
                    state: self.state_shared.clone(),
                });
        }
    }
}

impl<'a> Deref for ImageGuard<'a> {
    type Target = image::RgbaImage;

    fn deref(&self) -> &Self::Target {
        self.state_guard
            .image_buffer
            .as_ref()
            .expect("image sender, but image not set")
    }
}

impl<'a> DerefMut for ImageGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state_guard.image_dirty = true;
        self.state_guard
            .image_buffer
            .as_mut()
            .expect("image sender, but image not set")
    }
}

#[derive(Debug)]
struct State {
    texture_and_view: Arc<TextureAndView>,
    size: Vector2<u32>,
    image_buffer: Option<image::RgbaImage>,
    image_dirty: bool,
    command_sender: CommandSender,
}
