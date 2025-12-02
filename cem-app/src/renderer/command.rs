use std::sync::mpsc;

use crate::renderer::{
    draw_commands::DrawCommandInfo,
    texture_channel::CopyImageToTextureCommand,
};

#[derive(Debug)]
pub(super) enum Command {
    CopyImageToTexture(CopyImageToTextureCommand),
    DrawCommandInfo(DrawCommandInfo),
}

impl From<CopyImageToTextureCommand> for Command {
    fn from(value: CopyImageToTextureCommand) -> Self {
        Self::CopyImageToTexture(value)
    }
}

impl From<DrawCommandInfo> for Command {
    fn from(value: DrawCommandInfo) -> Self {
        Self::DrawCommandInfo(value)
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

#[derive(Debug)]
pub struct CommandQueue {
    pub sender: CommandSender,
    pub receiver: CommandReceiver,
}

impl Default for CommandQueue {
    fn default() -> Self {
        // we need to make sure this is only reached if there's a bug (e.g. not reading
        // the queue)
        Self::new(1024)
    }
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
