use std::{
    collections::VecDeque,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

pub use egui_file_dialog as file_dialog;
use egui_file_dialog::{
    DialogState,
    FileDialog,
};
use parking_lot::Mutex;
use serde::{
    Deserialize,
    Serialize,
};

use crate::path::{
    FormatPath,
    format_path,
};

/// iOS-style toggle switch:
///
/// ``` text
///      _____________
///     /       /.....\
///    |       |.......|
///     \_______\_____/
/// ```
///
/// ## Example:
/// ``` ignore
/// toggle_ui(ui, &mut my_bool);
/// ```
pub fn toggle_ui(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    // Widget code can be broken up in four steps:
    //  1. Decide a size for the widget
    //  2. Allocate space for it
    //  3. Handle interactions with the widget (if any)
    //  4. Paint the widget

    // 1. Deciding widget size:
    // You can query the `ui` how much space is available,
    // but in this example we have a fixed size widget based on the height of a
    // standard button:
    let desired_size = ui.spacing().interact_size.y * egui::vec2(2.0, 1.0);

    // 2. Allocating space:
    // This is where we get a region of the screen assigned.
    // We also tell the Ui to sense clicks in the allocated region.
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    // 3. Interact: Time to check for clicks!
    if response.clicked() {
        *on = !*on;
        response.mark_changed(); // report back that the value changed
    }

    // Attach some meta-data to the response which can be used by screen readers:
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });

    // 4. Paint!
    // Make sure we need to paint:
    if ui.is_rect_visible(rect) {
        // Let's ask for a simple animation from egui.
        // egui keeps track of changes in the boolean associated with the id and
        // returns an animated value in the 0-1 range for how much "on" we are.
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
        // We will follow the current style by asking
        // "how should something that is being interacted with be painted?".
        // This will, for instance, give us different colors when the widget is hovered
        // or clicked.
        let visuals = ui.style().interact_selectable(&response, *on);
        // All coordinates are in absolute screen coordinates so we use `rect` to place
        // the elements.
        let rect = rect.expand(visuals.expansion);
        let radius = 0.5 * rect.height();
        ui.painter().rect(
            rect,
            radius,
            visuals.bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        // Paint the circle, animating it from left to right with `how_on`:
        let circle_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        let center = egui::pos2(circle_x, rect.center().y);
        ui.painter()
            .circle(center, 0.75 * radius, visuals.bg_fill, visuals.fg_stroke);
    }

    // All done! Return the interaction response so the user can check what happened
    // (hovered, clicked, ...) and maybe show a tooltip:
    response
}

/// Here is the same code again, but a bit more compact:
#[expect(dead_code)]
fn toggle_ui_compact(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let desired_size = ui.spacing().interact_size.y * egui::vec2(2.0, 1.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });

    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
        let visuals = ui.style().interact_selectable(&response, *on);
        let rect = rect.expand(visuals.expansion);
        let radius = 0.5 * rect.height();
        ui.painter().rect(
            rect,
            radius,
            visuals.bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let circle_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        let center = egui::pos2(circle_x, rect.center().y);
        ui.painter()
            .circle(center, 0.75 * radius, visuals.bg_fill, visuals.fg_stroke);
    }

    response
}

pub trait EguiUtilUiExt {
    fn toggle_button(&mut self, on: &mut bool) -> egui::Response;
    fn noop(&mut self) -> egui::Response;
    fn file_picker_button(
        &mut self,
        path: &mut Option<PathBuf>,
        config: &FilePickerConfig,
    ) -> egui::Response;
}

impl EguiUtilUiExt for egui::Ui {
    fn toggle_button(&mut self, on: &mut bool) -> egui::Response {
        toggle_ui(self, on)
    }

    fn noop(&mut self) -> egui::Response {
        self.allocate_response(egui::Vec2::default(), egui::Sense::empty())
    }

    fn file_picker_button(
        &mut self,
        path: &mut Option<PathBuf>,
        config: &FilePickerConfig,
    ) -> egui::Response {
        let id = self.id().with("file_picker");
        struct State {
            file_dialog: FileDialog,
            formatted_path: Option<egui::RichText>,
            formatted_file_name: Option<egui::RichText>,
        }

        let mut was_picked = false;
        let mut is_open = false;
        let mut button_text = None;
        let mut hover_text = None;

        {
            let state = self.data(|data| data.get_temp::<Arc<Mutex<State>>>(id));

            if let Some(state) = state {
                let mut state = state.lock();

                if path.is_none() {
                    state.formatted_path = None;
                    state.formatted_file_name = None;
                }

                state.file_dialog.update(self.ctx());

                if let Some(picked) = state.file_dialog.take_picked() {
                    tracing::debug!(path = %picked.display(), "picked file");

                    state.formatted_path = Some(format_path(&picked).to_string().into());
                    state.formatted_file_name =
                        Some(picked.file_name().unwrap().display().to_string().into());

                    *path = Some(picked);
                    was_picked = true;
                }
                // Arc these if you want to avoid cloning every frame
                button_text = state.formatted_file_name.clone();
                hover_text = state.formatted_path.clone();

                is_open = matches!(state.file_dialog.state(), DialogState::Open);
            }
        };

        let mut response = self
            .horizontal(|ui| {
                let button_text = button_text.unwrap_or_else(|| "Pick File".into());
                let mut button_response = ui.add_enabled(!is_open, egui::Button::new(button_text));

                if let Some(hover_text) = hover_text {
                    button_response = button_response.on_hover_text(hover_text);
                }

                if button_response.clicked() {
                    *path = None;

                    let state = ui.data_mut(|data| {
                        data.get_temp_mut_or_insert_with(id, || {
                            let file_dialog = FileDialog::new();
                            Arc::new(Mutex::new(State {
                                file_dialog,
                                formatted_path: None,
                                formatted_file_name: None,
                            }))
                        })
                        .clone()
                    });

                    let mut state = state.lock();

                    match config {
                        FilePickerConfig::Open => state.file_dialog.pick_file(),
                        FilePickerConfig::Save => state.file_dialog.save_file(),
                    }

                    state.formatted_path = None;
                }

                if ui
                    .add_enabled(path.is_some(), egui::Button::new("x"))
                    .clicked()
                {
                    *path = None;
                }
            })
            .response;

        if was_picked {
            response.mark_changed();
        }

        response
    }
}

#[derive(Clone, Debug)]
pub enum FilePickerConfig {
    Open,
    Save,
}

// clippy, this config is not final yet, and soon it won't be derivable anymore.
#[allow(clippy::derivable_impls)]
impl Default for FilePickerConfig {
    fn default() -> Self {
        Self::Open
    }
}

pub trait EguiUtilContextExt {
    fn repaint_trigger(&self) -> RepaintTrigger;
}

impl EguiUtilContextExt for egui::Context {
    fn repaint_trigger(&self) -> RepaintTrigger {
        RepaintTrigger {
            repaint_interval: None,
            egui: self.clone(),
            viewport: self.viewport_id(),
        }
    }
}

impl<P> From<FormatPath<P>> for egui::WidgetText
where
    P: AsRef<Path>,
{
    fn from(value: FormatPath<P>) -> Self {
        value.to_string().into()
    }
}

#[derive(Clone, Debug)]
pub struct RepaintTrigger {
    repaint_interval: Option<Duration>,
    egui: egui::Context,
    viewport: egui::ViewportId,
}

impl RepaintTrigger {
    pub fn with_max_fps(mut self, max_fps: u16) -> Self {
        self.repaint_interval = Some(Duration::from_millis(1000 / u64::from(max_fps)));
        self
    }

    pub fn repaint(&self) {
        let mut do_repaint = true;

        if let Some(repaint_interval) = self.repaint_interval {
            let last_repaint = self
                .egui
                .data(|data| data.get_temp::<LastRepaint>(egui::Id::NULL));

            if let Some(last_repaint) = last_repaint {
                let mut last_repaint = last_repaint.0.lock();
                let now = Instant::now();
                if now > *last_repaint + repaint_interval {
                    *last_repaint = now;
                }
                else {
                    do_repaint = false;
                }
            }
        }

        if do_repaint {
            self.egui.request_repaint_of(self.viewport);
        }
    }
}

#[derive(Clone)]
struct LastRepaint(Arc<Mutex<Instant>>);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct RecentlyOpenedFilesData {
    files: VecDeque<PathBuf>,
}

/// Container to store recently opened files in egui's memory
#[derive(Clone, Debug)]
pub struct RecentlyOpenedFiles {
    ctx: egui::Context,
    limit: Option<usize>,
    id: egui::Id,
}

impl RecentlyOpenedFiles {
    pub fn new(ctx: egui::Context, id: egui::Id, limit: impl Into<Option<usize>>) -> Self {
        Self {
            ctx,
            limit: limit.into(),
            id,
        }
    }

    pub fn get(&self) -> Vec<PathBuf> {
        self.ctx.data_mut(|data| {
            data.get_persisted_mut_or_default::<RecentlyOpenedFilesData>(self.id)
                .files
                .iter()
                .cloned()
                .collect()
        })
    }

    pub fn insert(&self, path: impl Into<PathBuf>) {
        self.ctx.data_mut(|data| {
            let data = data.get_persisted_mut_or_default::<RecentlyOpenedFilesData>(self.id);

            data.files.push_front(path.into());

            if let Some(limit) = self.limit {
                let too_many = data.files.len().saturating_sub(limit);
                for _ in 0..too_many {
                    data.files.pop_back();
                }
            }
        });
    }

    pub fn move_to_top(&self, path: impl AsRef<Path>) {
        self.ctx.data_mut(|data| {
            let data = data.get_persisted_mut_or_default::<RecentlyOpenedFilesData>(self.id);

            let path = path.as_ref();

            if let Some(first) = data.files.front()
                && first == path
            {
                // the passed in path is already at the top
                return;
            }

            let mut owned_path = None;
            data.files.retain_mut(|file| {
                if file == path {
                    // if we already have a PathBuf for this in the list we take it out
                    owned_path = Some(std::mem::take(file));
                    false
                }
                else {
                    true
                }
            });

            // either reuse the PathBuf we found or create one from the passed in path
            data.files
                .push_front(owned_path.unwrap_or_else(|| path.to_owned()));
        });
    }
}
