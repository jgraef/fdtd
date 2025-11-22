use std::sync::Arc;

use color_eyre::eyre::Error;
use egui::Id;
use parking_lot::Mutex;

pub fn show_error_dialog(ctx: &egui::Context) {
    let container = ctx
        .data(|data| data.get_temp::<Container>(Id::NULL))
        .expect("error dialog not registered in egui context");

    let mut inner = container.inner.lock();
    inner.show(ctx);
}

pub trait ResultExt<T>: Sized {
    fn ok_or_handle(self, handler: impl ErrorHandler) -> Option<T>;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    Error: From<E>,
{
    fn ok_or_handle(self, handler: impl ErrorHandler) -> Option<T> {
        match self {
            Ok(value) => Some(value),
            Err(error) => {
                handler.handle_error(error.into());
                None
            }
        }
    }
}

pub trait ErrorHandler {
    fn handle_error(self, error: Error);
}

impl ErrorHandler for &mut ErrorDialog {
    fn handle_error(self, error: Error) {
        self.error = Some(error);
    }
}

impl ErrorHandler for &egui::Context {
    fn handle_error(self, error: Error) {
        let container = self
            .data(|data| data.get_temp::<Container>(Id::NULL))
            .expect("error dialog not initialized");

        let mut inner = container.inner.lock();
        inner.error = Some(error);
    }
}

impl ErrorHandler for &egui::Ui {
    fn handle_error(self, error: Error) {
        self.ctx().handle_error(error);
    }
}

#[derive(Debug, Default)]
pub struct ErrorDialog {
    error: Option<Error>,
}

impl ErrorDialog {
    pub fn ok_or_show<T, E>(&mut self, result: Result<T, E>) -> Option<T>
    where
        Error: From<E>,
    {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.error = Some(error.into());
                None
            }
        }
    }

    pub fn set_error<E>(&mut self, error: E)
    where
        Error: From<E>,
    {
        self.error = Some(error.into());
    }

    pub fn clear(&mut self) {
        self.error = None;
    }

    pub fn register_in_context(self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.insert_temp(Id::NULL, Container::default());
        })
    }

    fn show(&mut self, ctx: &egui::Context) {
        if let Some(error) = &self.error {
            let mut open1 = true;
            let mut open2 = true;

            egui::Window::new("Error")
                .movable(true)
                .open(&mut open1)
                .collapsible(false)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("error_message")
                        .show(ui, |ui| {
                            egui::Frame::new().inner_margin(5).show(ui, |ui| {
                                ui.label(format!("{error:#}"));
                            });
                        });

                    ui.separator();

                    ui.with_layout(egui::Layout::right_to_left(Default::default()), |ui| {
                        if ui.button("Close").clicked() {
                            open2 = false;
                        }
                    });
                });

            if !open1 || !open2 {
                self.error = None;
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Container {
    inner: Arc<Mutex<ErrorDialog>>,
}
