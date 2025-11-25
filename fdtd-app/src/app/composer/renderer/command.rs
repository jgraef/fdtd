use std::sync::mpsc;

use crate::app::composer::renderer::texture_channel::CopyImageToTextureCommand;

#[derive(Debug)]
pub(super) enum Command {
    CopyImageToTexture(CopyImageToTextureCommand),
}

impl From<CopyImageToTextureCommand> for Command {
    fn from(value: CopyImageToTextureCommand) -> Self {
        Self::CopyImageToTexture(value)
    }
}

#[derive(Debug)]
pub struct CommandReceiver {
    receiver: mpsc::Receiver<Command>,
}

impl CommandReceiver {
    pub fn drain(&self) -> mpsc::TryIter<'_, Command> {
        // will iter over all items until it's empty, or yield nothing if it's closed.
        // the latter should not really happen because the renderer holds onto a sender
        // to hand out.
        //
        // importantly this will never block
        self.receiver.try_iter()
    }
}

#[derive(Clone, Debug)]
pub struct CommandSender {
    sender: mpsc::SyncSender<Command>,
}

impl CommandSender {
    pub(super) fn send(&self, command: impl Into<Command>) {
        if let Err(error) = self.sender.send(command.into()) {
            tracing::warn!(?error, "Renderer command queue full");
        }
    }
}

#[derive(Debug)]
pub struct CommandQueue {
    pub sender: CommandSender,
    pub receiver: CommandReceiver,
}

impl CommandQueue {
    pub fn new(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        Self {
            sender: CommandSender { sender },
            receiver: CommandReceiver { receiver },
        }
    }
}
