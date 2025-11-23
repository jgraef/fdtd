use std::sync::Arc;

use image::RgbaImage;
use nalgebra::Vector2;
use parking_lot::{
    RwLock,
    RwLockReadGuard,
};

use crate::app::composer::renderer::{
    command::CommandSender,
    light::TextureAndView,
};

pub fn texture_channel() -> (UndecidedTextureSender, TextureReceiver) {
    let shared = Arc::new(RwLock::new(State::default()));

    let sender = UndecidedTextureSender {
        state: shared.clone(),
    };
    let receiver = TextureReceiver { state: shared };
    (sender, receiver)
}

#[derive(Clone, Debug)]
pub struct TextureReceiver {
    state: Arc<RwLock<State>>,
}

impl TextureReceiver {
    pub fn register(
        &mut self,
        command_sender: &CommandSender,
        create_texture: impl FnOnce(&Vector2<u32>, &str) -> wgpu::Texture,
    ) -> Option<Arc<TextureAndView>> {
        let mut state = self.state.write();

        if let Some(texture_specification) = &state.texture_specification
            && state.texture_and_view.is_none()
        {
            state.texture_and_view = Some(create_texture_and_view(
                texture_specification,
                create_texture,
            ));
        }

        if state.command_sender.is_none() {
            state.command_sender = Some(command_sender.clone());
        }

        state.texture_and_view.clone()
    }
}

fn create_texture_and_view(
    texture_specification: &TextureSpecification,
    create_texture: impl FnOnce(&Vector2<u32>, &str) -> wgpu::Texture,
) -> Arc<TextureAndView> {
    let texture = create_texture(&texture_specification.size, &texture_specification.label);
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(&texture_specification.label),
        ..Default::default()
    });

    Arc::new(TextureAndView { texture, view })
}

#[derive(Debug)]
pub(super) struct CreateTextureForChannelCommand {
    state: Arc<RwLock<State>>,
}

impl CreateTextureForChannelCommand {
    pub fn handle(&self, create_texture: impl FnOnce(&Vector2<u32>, &str) -> wgpu::Texture) {
        let mut state = self.state.write();
        let texture_specification = state
            .texture_specification
            .as_ref()
            .expect("command to create texture, but texture_specification not set");
        assert!(state.texture_and_view.is_none());

        state.texture_and_view = Some(create_texture_and_view(
            texture_specification,
            create_texture,
        ));
    }
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

            let texture_and_view = state
                .texture_and_view
                .as_ref()
                .expect("received copy-image-to-texture command but no texture set");

            copy_image_to_texture(image, &texture_and_view.texture);
        }
    }
}

#[derive(Debug)]
pub struct UndecidedTextureSender {
    state: Arc<RwLock<State>>,
}

impl UndecidedTextureSender {
    pub fn send_images(self, size: &Vector2<u32>, label: impl ToString) -> ImageSender {
        {
            let mut state = self.state.write();

            state.texture_specification = Some(TextureSpecification {
                size: *size,
                label: label.to_string(),
            });
            state.image_buffer = Some(RgbaImage::new(size.x, size.y));

            if let Some(command_sender) = &state.command_sender {
                command_sender.send(CreateTextureForChannelCommand {
                    state: self.state.clone(),
                });
            }
        }

        ImageSender { state: self.state }
    }

    pub fn send_texture(self, size: &Vector2<u32>, label: impl ToString) -> TextureSender {
        let mut state = self.state.write();
        state.texture_specification = Some(TextureSpecification {
            size: *size,
            label: label.to_string(),
        });
        drop(state);
        TextureSender { state: self.state }
    }
}

#[derive(Debug)]
pub struct TextureSender {
    state: Arc<RwLock<State>>,
}

#[derive(Debug)]
pub struct ImageSender {
    state: Arc<RwLock<State>>,
}

impl ImageSender {
    pub fn update_image(&mut self) -> ImageGuard<'_> {
        let state = self.state.read();
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
    state_guard: RwLockReadGuard<'a, State>,
    dirty_before: bool,
}

impl<'a> Drop for ImageGuard<'a> {
    fn drop(&mut self) {
        if !self.dirty_before
            && self.state_guard.image_dirty
            && let Some(command_sender) = &self.state_guard.command_sender
        {
            command_sender.send(CreateTextureForChannelCommand {
                state: self.state_shared.clone(),
            });
        }
    }
}

#[derive(Debug, Default)]
struct State {
    texture_and_view: Option<Arc<TextureAndView>>,
    image_buffer: Option<image::RgbaImage>,
    image_dirty: bool,
    texture_specification: Option<TextureSpecification>,
    command_sender: Option<CommandSender>,
}

#[derive(Debug)]
struct TextureSpecification {
    size: Vector2<u32>,
    label: String,
}
