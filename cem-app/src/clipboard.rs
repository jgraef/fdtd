use std::{
    collections::VecDeque,
    sync::Arc,
};

use parking_lot::RwLock;

/// Clipboard
///
/// Manages copying to and pasting from the clipboard. This contains a local
/// buffer for objects that can't be copied outside the app (e.g. entities).
///
/// # TODO
///
/// - Wire up with the OS clipboard
#[derive(Clone, Debug, Default)]
pub struct Clipboard {
    local_buffer: LocalBuffer,
}

impl Clipboard {
    pub fn push(&mut self, item: impl Into<ClipboardItem>) {
        self.local_buffer.push(item.into());
    }

    pub fn top(&self) -> Option<&ClipboardItem> {
        self.local_buffer.top()
    }
}

#[derive(Clone, Debug, Default)]
struct LocalBuffer {
    buffer: VecDeque<ClipboardItem>,
    limit: Option<usize>,
}

impl LocalBuffer {
    pub fn new(limit: Option<usize>) -> Self {
        Self {
            buffer: VecDeque::new(),
            limit,
        }
    }

    pub fn push(&mut self, item: ClipboardItem) {
        self.buffer.push_front(item);

        if let Some(limit) = self.limit {
            while self.buffer.len() > limit {
                self.buffer.pop_back().unwrap();
            }
        }
    }

    pub fn top(&self) -> Option<&ClipboardItem> {
        self.buffer.front()
    }
}

#[derive(Clone, derive_more::Debug)]
pub enum ClipboardItem {
    Text {
        text: String,
    },
    Entities {
        // todo: bevy-migrate: clipboard
        //#[debug(skip)]
        //entity_builder: Vec<hecs::EntityBuilderClone>,
    },
}

impl From<String> for ClipboardItem {
    fn from(value: String) -> Self {
        Self::Text { text: value }
    }
}

impl From<&str> for ClipboardItem {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

pub trait EguiClipboardExt {
    fn clipboard<R>(&self, f: impl FnOnce(&Clipboard) -> R) -> R;
    fn clipboard_mut<R>(&self, f: impl FnOnce(&mut Clipboard) -> R) -> R;
}

impl EguiClipboardExt for egui::Context {
    fn clipboard<R>(&self, f: impl FnOnce(&Clipboard) -> R) -> R {
        EguiClipboardData::read(self, f)
    }

    fn clipboard_mut<R>(&self, f: impl FnOnce(&mut Clipboard) -> R) -> R {
        EguiClipboardData::write(self, f)
    }
}

impl EguiClipboardExt for egui::Ui {
    fn clipboard<R>(&self, f: impl FnOnce(&Clipboard) -> R) -> R {
        EguiClipboardData::read(self.ctx(), f)
    }

    fn clipboard_mut<R>(&self, f: impl FnOnce(&mut Clipboard) -> R) -> R {
        EguiClipboardData::write(self.ctx(), f)
    }
}

#[derive(Clone)]
struct EguiClipboardData(Arc<RwLock<Clipboard>>);

impl EguiClipboardData {
    fn get(ctx: &egui::Context) -> Self {
        ctx.data(|data| {
            data.get_temp::<EguiClipboardData>(egui::Id::NULL)
                .expect("Clipboard not initialized. Is the clipboard extension plugin registered?")
        })
    }

    fn read<R>(ctx: &egui::Context, f: impl FnOnce(&Clipboard) -> R) -> R {
        let clipboard = Self::get(ctx);
        let clipboard = clipboard.0.read();
        f(&clipboard)
    }

    fn write<R>(ctx: &egui::Context, f: impl FnOnce(&mut Clipboard) -> R) -> R {
        let clipboard = Self::get(ctx);
        let mut clipboard = clipboard.0.write();
        f(&mut clipboard)
    }
}

#[derive(Debug)]
pub struct EguiClipboardPlugin;

impl egui::Plugin for EguiClipboardPlugin {
    fn debug_name(&self) -> &'static str {
        "clipboard-ext"
    }

    fn setup(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::NULL,
                EguiClipboardData(Arc::new(RwLock::new(Clipboard::default()))),
            );
        });
    }

    fn input_hook(&mut self, input: &mut egui::RawInput) {
        for event in &input.events {
            match event {
                egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) => {
                    tracing::debug!(?event, "clipboard-ext: input event");
                }
                _ => {}
            }
        }
    }

    fn output_hook(&mut self, output: &mut egui::FullOutput) {
        for command in &output.platform_output.commands {
            #[allow(clippy::single_match)]
            match command {
                egui::OutputCommand::CopyText(_) => {
                    tracing::debug!(?command, "clipboard-ext: platform command");
                }
                _ => {}
            }
        }
    }
}
