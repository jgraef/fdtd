//! Source code example of how to create your own widget.
//! This is meant to be read as a tutorial, hence the plethora of comments.

use std::{
    path::Path,
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use parking_lot::Mutex;

use crate::{
    app::start::WgpuContext,
    util::FormatPath,
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
}

impl EguiUtilUiExt for egui::Ui {
    fn toggle_button(&mut self, on: &mut bool) -> egui::Response {
        toggle_ui(self, on)
    }
}

pub trait EguiUtilContextExt {
    fn repaint_trigger(&self) -> RepaintTrigger;
    fn wgpu_context(&self) -> WgpuContext;
}

impl EguiUtilContextExt for egui::Context {
    fn repaint_trigger(&self) -> RepaintTrigger {
        RepaintTrigger {
            repaint_interval: None,
            egui: self.clone(),
            viewport: self.viewport_id(),
        }
    }

    fn wgpu_context(&self) -> WgpuContext {
        self.data(|data| data.get_temp(egui::Id::NULL))
            .expect("no wgpu context available")
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
