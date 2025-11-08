#[derive(Debug, Default)]
pub struct DebugPanel {
    pub show: bool,
}

impl DebugPanel {
    pub fn show(&mut self, ctx: &egui::Context) {
        ctx.input(|input| {
            self.show ^= input.key_pressed(egui::Key::F5);
        });

        if self.show {
            egui::SidePanel::left("debug_panel").show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("debug_panel")
                    .max_height(f32::INFINITY)
                    .show(ui, |ui| {
                        if ui.button("Close").clicked() {
                            self.show = false;
                        }

                        ui.collapsing("Settings", |ui| {
                            ctx.settings_ui(ui);
                        });

                        ui.collapsing("Inspection", |ui| {
                            ctx.inspection_ui(ui);
                        });

                        ui.collapsing("Memory", |ui| {
                            ctx.memory_ui(ui);
                        });
                    });
            });
        }
    }
}
