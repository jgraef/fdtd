use nalgebra::{
    Point2,
    Point3,
    Translation3,
    Vector2,
    Vector3,
};

use crate::composer::{
    renderer::{
        Renderer,
        camera::CameraProjection,
    },
    scene::{
        SharedScene,
        Transform,
    },
};

#[derive(Debug)]
pub struct SceneView<'a> {
    scene: SharedScene,
    camera_entity: Option<hecs::Entity>,
    clicked_entity: Option<&'a mut Option<ClickedEntity>>,
}

impl<'a> SceneView<'a> {
    pub fn new(scene: SharedScene) -> Self {
        Self {
            scene,
            camera_entity: None,
            clicked_entity: None,
        }
    }

    pub fn with_camera(mut self, camera: hecs::Entity) -> Self {
        self.camera_entity = Some(camera);
        self
    }

    pub fn with_entity_selection(mut self, clicked_entity: &'a mut Option<ClickedEntity>) -> Self {
        self.clicked_entity = Some(clicked_entity);
        self
    }

    /// Handle widget's inputs
    fn handle_input(&mut self, response: &egui::Response) {
        let camera_pan_tilt_speed = Vector2::repeat(1.0);
        let camera_translation_speed = Vector3::new(0.5, 0.5, 0.1);

        let Some(camera_entity) = self.camera_entity
        else {
            return;
        };

        response.ctx.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::Vec2 { x: _, y: delta },
                        modifiers: _,
                    } => {
                        let mut scene = self.scene.write();

                        if let Ok(camera_transform) = scene
                            .entities
                            .query_one_mut::<&mut Transform>(camera_entity)
                        {
                            camera_transform.translate_local(&Translation3::new(
                                0.0,
                                0.0,
                                camera_translation_speed.z * *delta,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        });

        let drag_delta = || {
            // drag delta in normalized screen coordinates `[-1, 1]^2`
            let drag_delta = response.drag_delta();
            Vector2::new(
                2.0 * drag_delta.x / response.rect.width(),
                -2.0 * drag_delta.y / response.rect.height(),
            )
        };
        let interact_pointer_pos = || {
            // does egui not have a better way of doing this?
            response.interact_pointer_pos().map(|pointer_position| {
                Point2::new(
                    2.0 * (pointer_position.x - response.rect.left()) / response.rect.width() - 1.0,
                    -2.0 * (pointer_position.y - response.rect.top()) / response.rect.height()
                        + 1.0,
                )
            })
        };

        if response.dragged_by(egui::PointerButton::Primary) {
            let mut scene = self.scene.write();

            if let Ok((camera_transform, camera_projection)) =
                scene
                    .entities
                    .query_one_mut::<(&mut Transform, &CameraProjection)>(camera_entity)
            {
                let drag_angle = camera_projection.unproject_screen(&drag_delta().into());

                camera_transform.pan_tilt(
                    camera_pan_tilt_speed.x * drag_angle.x,
                    camera_pan_tilt_speed.y * drag_angle.y,
                    &Vector3::y_axis(),
                );
            }
        }
        else if response.dragged_by(egui::PointerButton::Secondary) {
            let mut scene = self.scene.write();

            if let Ok(camera_transform) = scene
                .entities
                .query_one_mut::<&mut Transform>(camera_entity)
            {
                // todo: we need to take the aspect ratio into account when translating
                let drag_delta = drag_delta();
                camera_transform.translate_local(&Translation3::new(
                    -camera_translation_speed.x * drag_delta.x,
                    -camera_translation_speed.y * drag_delta.y,
                    0.0,
                ));
            }
        }

        if response.clicked_by(egui::PointerButton::Primary) {
            if let Some(clicked_entity) = &mut self.clicked_entity {
                **clicked_entity = None;

                if let Some(pointer_position) = interact_pointer_pos() {
                    let scene = self.scene.read();

                    if let Ok(mut query) = scene
                        .entities
                        .query_one::<(&Transform, &CameraProjection)>(camera_entity)
                    {
                        if let Some((camera_transform, camera_projection)) = query.get() {
                            let ray = camera_projection
                                .shoot_screen_ray(&pointer_position)
                                .transform_by(&camera_transform.transform);

                            if let Some(ray_hit) = scene.cast_ray(&ray, None) {
                                tracing::debug!(?ray_hit, ?ray, "ray hit");
                                let point_clicked = ray.point_at(ray_hit.time_of_impact);

                                **clicked_entity = Some(ClickedEntity {
                                    entity: ray_hit.entity,
                                    distance_from_camera: ray_hit.time_of_impact,
                                    point_clicked,
                                });
                            }
                            else {
                                tracing::debug!(?ray, "ray didn't hit");
                            }
                        }
                    }
                }
            }
        }
    }
}

impl<'a> egui::Widget for SceneView<'a> {
    fn ui(mut self, ui: &mut egui::Ui) -> egui::Response {
        let response = ui.allocate_response(ui.available_size(), egui::Sense::click_and_drag());

        if response.contains_pointer() {
            self.handle_input(&response);
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

#[derive(Debug)]
struct RenderCallback {
    scene: SharedScene,
    camera: Option<hecs::Entity>,
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
        let mut scene = self.scene.write();

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
                let mut scene = self.scene.write();

                renderer.render(camera_entity, &mut scene, &info, render_pass);
            }
            else {
                // todo: just clear with black?
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ClickedEntity {
    pub entity: hecs::Entity,
    pub distance_from_camera: f32,
    pub point_clicked: Point3<f32>,
}
