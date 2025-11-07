use hecs::Entity;
use nalgebra::{
    Translation3,
    Vector2,
    Vector3,
};

use crate::composer::{
    renderer::Renderer,
    scene::{
        SharedWorld,
        Transform,
    },
};

#[derive(Clone, Debug)]
pub struct SceneView {
    pub scene: SharedWorld,
    pub camera_entity: Option<Entity>,
}

impl egui::Widget for SceneView {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // todo: these should probably be scalars and then we need to adjust
        // horizontal/vertical translation/rotation with the fov.
        let camera_pan_tilt_speed = Vector2::repeat(1.0);
        let camera_translation_speed = Vector3::new(0.5, 0.5, 0.1);

        let response = ui.allocate_response(ui.available_size(), egui::Sense::click_and_drag());

        if let Some(camera_entity) = self.camera_entity {
            let mut scene = self.scene.world_mut();

            if let Ok(camera_transform) = scene
                .entities
                .query_one_mut::<&mut Transform>(camera_entity)
            {
                if response.contains_pointer() {
                    ui.input(|input| {
                        for event in &input.events {
                            match event {
                                egui::Event::MouseWheel {
                                    unit: egui::MouseWheelUnit::Line,
                                    delta: egui::Vec2 { x: _, y: delta },
                                    modifiers: _,
                                } => {
                                    camera_transform.translate_local(&Translation3::new(
                                        0.0,
                                        0.0,
                                        camera_translation_speed.z * *delta,
                                    ));
                                }
                                _ => {}
                            }
                        }
                    });
                }

                if response.dragged() {
                    // drag delta in normalized screen coordinates `[-1, 1]^2`
                    let drag_delta = {
                        let drag_delta = response.drag_delta();
                        Vector2::new(
                            2.0 * drag_delta.x / response.rect.width(),
                            -2.0 * drag_delta.y / response.rect.height(),
                        )
                    };

                    if response.dragged_by(egui::PointerButton::Primary) {
                        camera_transform.pan_tilt(
                            camera_pan_tilt_speed.x * drag_delta.x,
                            camera_pan_tilt_speed.y * drag_delta.y,
                            &Vector3::y_axis(),
                        );
                    }
                    else if response.dragged_by(egui::PointerButton::Secondary) {
                        camera_transform.translate_local(&Translation3::new(
                            -camera_translation_speed.x * drag_delta.x,
                            -camera_translation_speed.y * drag_delta.y,
                            0.0,
                        ));
                    }
                }
            }
        }

        let painter = ui.painter();
        painter.add(egui_wgpu::Callback::new_paint_callback(
            response.rect,
            RenderCallback {
                scene: self.scene.clone(),
                camera: self.camera_entity,
            },
        ));

        response
    }
}
struct RenderCallback {
    scene: SharedWorld,
    camera: Option<Entity>,
}

impl egui_wgpu::CallbackTrait for RenderCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let mut scene = self.scene.world_mut();

        let renderer = Renderer::get_mut_or_init(callback_resources, device);
        renderer.prepare(&mut scene, device, queue);

        vec![]
    }

    fn finish_prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = Renderer::get_mut(callback_resources) {
            renderer.finish_prepare();
        }

        vec![]
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(renderer) = Renderer::get(callback_resources) {
            if let Some(camera_entity) = self.camera {
                let mut scene = self.scene.world_mut();

                renderer.render(camera_entity, &mut scene, &info, render_pass);
            }
            else {
                // todo: just clear with black?
            }
        }
    }
}
