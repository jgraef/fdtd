use hecs::Entity;

use crate::composer::{
    renderer::Renderer,
    scene::SharedWorld,
};

#[derive(Clone, Debug)]
pub struct SceneView {
    pub world: SharedWorld,
    pub camera: Option<Entity>,
}

impl egui::Widget for SceneView {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let response = ui.allocate_response(ui.available_size(), egui::Sense::empty());

        let painter = ui.painter();
        painter.add(egui_wgpu::Callback::new_paint_callback(
            response.rect,
            RenderCallback {
                world: self.world.clone(),
                camera: self.camera,
            },
        ));

        response
    }
}
struct RenderCallback {
    world: SharedWorld,
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
        let mut scene = self.world.world_mut();

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
                let mut scene = self.world.world_mut();

                renderer.render(camera_entity, &mut scene, &info, render_pass);
            }
            else {
                // todo: just clear with black?
            }
        }
    }
}
