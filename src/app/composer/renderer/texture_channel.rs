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
        format: wgpu::TextureFormat,
        create_texture: impl FnOnce(&Vector2<u32>, &str) -> wgpu::Texture,
    ) -> Option<Arc<TextureAndView>> {
        let mut state = self.state.write();

        let receiver_spec = ReceiverSpec { format };

        if state.receiver_spec.is_none() {
            state.receiver_spec = Some(receiver_spec);
        }

        if state.command_sender.is_none() {
            state.command_sender = Some(command_sender.clone());
        }

        if let Some(sender_spec) = &state.sender_spec
            && state.texture_and_view.is_none()
        {
            let texture_and_view = create_texture_and_view(sender_spec, create_texture);
            assert_eq!(texture_and_view.texture.format(), receiver_spec.format);
            state.texture_and_view = Some(texture_and_view);
        }

        state.texture_and_view.clone()
    }
}

fn create_texture_and_view(
    sender_spec: &SenderSpec,
    create_texture: impl FnOnce(&Vector2<u32>, &str) -> wgpu::Texture,
) -> Arc<TextureAndView> {
    let texture = create_texture(&sender_spec.size, &sender_spec.label);

    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(&sender_spec.label),
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

        // note: this command is only sent if the sender can't find a texture, but by
        // the time we got around to handling it, we might already have created it.
        if state.texture_and_view.is_none() {
            let texture_specification = state
                .sender_spec
                .as_ref()
                .expect("command to create texture, but texture_specification not set");

            state.texture_and_view = Some(create_texture_and_view(
                texture_specification,
                create_texture,
            ));
        }
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

            state.image_buffer = Some(RgbaImage::new(size.x, size.y));

            // note: even though we don't need the texture to send images, we need to notify
            // the renderer that we have set the size, and it'll create it and manage it for
            // us.
            request_texture(&mut state, &self.state, *size, label.to_string());
        }

        ImageSender { state: self.state }
    }

    pub fn send_texture(self, size: &Vector2<u32>, label: impl ToString) -> TextureSender {
        let mut state = self.state.write();
        request_texture(&mut state, &self.state, *size, label.to_string());
        drop(state);
        TextureSender { state: self.state }
    }
}

#[derive(Debug)]
pub struct TextureSender {
    state: Arc<RwLock<State>>,
}

impl TextureSender {
    pub fn get(&self) -> Option<Arc<TextureAndView>> {
        let state = self.state.read();
        state.texture_and_view.clone()
    }

    pub fn format(&self) -> Option<wgpu::TextureFormat> {
        let state = self.state.read();
        Some(state.receiver_spec.as_ref()?.format)
    }
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

#[derive(Debug, Default)]
struct State {
    texture_and_view: Option<Arc<TextureAndView>>,
    image_buffer: Option<image::RgbaImage>,
    image_dirty: bool,
    sender_spec: Option<SenderSpec>,
    receiver_spec: Option<ReceiverSpec>,
    command_sender: Option<CommandSender>,
}

fn request_texture(
    state_guard: &mut State,
    state_shared: &Arc<RwLock<State>>,
    size: Vector2<u32>,
    label: String,
) {
    assert!(
        state_guard.texture_and_view.is_none(),
        "We should not have a texture and view yet, because we haven't requested a size yet"
    );
    assert!(
        state_guard.sender_spec.is_none(),
        "request_texture called with a sender_spec already present"
    );

    state_guard.sender_spec = Some(SenderSpec { size, label });

    if let Some(command_sender) = &state_guard.command_sender {
        command_sender.send(CreateTextureForChannelCommand {
            state: state_shared.clone(),
        });
    }
}

#[derive(Clone, Debug)]
struct SenderSpec {
    size: Vector2<u32>,
    label: String,
}

#[derive(Clone, Copy, Debug)]
struct ReceiverSpec {
    format: wgpu::TextureFormat,
}
