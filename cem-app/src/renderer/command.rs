use std::sync::mpsc;

use bevy_ecs::{
    entity::Entity,
    resource::Resource,
};
use cem_util::exclusive::Exclusive;

use crate::renderer::{
    draw_commands::DrawCommandInfo,
    texture::channel::CopyImageToTextureCommand,
};

#[derive(Debug)]
pub(super) enum Command {
    CopyImageToTexture(CopyImageToTextureCommand),
    DrawCommandInfo {
        camera_entity: Entity,
        draw_command_info: DrawCommandInfo,
    },
}

impl From<CopyImageToTextureCommand> for Command {
    fn from(value: CopyImageToTextureCommand) -> Self {
        Self::CopyImageToTexture(value)
    }
}

#[derive(Debug, Resource)]
pub struct CommandReceiver {
    receiver: Exclusive<mpsc::Receiver<Command>>,
}

impl CommandReceiver {
    pub fn drain(&mut self) -> mpsc::TryIter<'_, Command> {
        // will iter over all items until it's empty, or yield nothing if it's closed.
        // the latter should not really happen because the renderer holds onto a sender
        // to hand out.
        //
        // importantly this will never block
        self.receiver.get_mut().try_iter()
    }
}

#[derive(Clone, Debug, Resource)]
pub struct CommandSender {
    sender: mpsc::SyncSender<Command>,
}

impl CommandSender {
    pub(super) fn send(&self, command: impl Into<Command>) {
        match self.sender.try_send(command.into()) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => {
                // when tearing down the applications there might still be a
                // command sender out there that can send while the renderer is
                // gone. we should just ignore any commands then.
            }
            Err(mpsc::TrySendError::Full(_)) => {
                // the renderer can't keep up. we could either make the queue unlimited but this
                // can easily lead to resource exhaustion, if we e.g. forget to read the queue
                // at all. we should consider this a hard error, because it
                // likely is a programming mistake
                panic!("renderer command queue full. are we reading from it?");
            }
        }
    }
}

pub fn queue(capacity: usize) -> (CommandSender, CommandReceiver) {
    let (sender, receiver) = mpsc::sync_channel(capacity);
    (
        CommandSender { sender },
        CommandReceiver {
            receiver: Exclusive::new(receiver),
        },
    )
}
