use nalgebra::{
    Point2,
    Point3,
    Translation3,
    Vector2,
    Vector3,
};

use crate::composer::{
    renderer::{
        DrawFrame,
        Renderer,
        camera::{
            CameraProjection,
            Viewport,
        },
    },
    scene::{
        Changed,
        Scene,
        Transform,
    },
};

#[derive(derive_more::Debug)]
pub struct SceneView<'a> {
    scene: &'a mut Scene,
    renderer: &'a mut Renderer,
    camera_entity: Option<hecs::Entity>,
    clicked_entity: Option<&'a mut Option<ClickedEntity>>,
    #[debug(ignore)]
    command_buffer: hecs::CommandBuffer,
}

impl<'a> SceneView<'a> {
    pub fn new(scene: &'a mut Scene, renderer: &'a mut Renderer) -> Self {
        Self {
            scene,
            renderer,
            camera_entity: None,
            clicked_entity: None,
            command_buffer: hecs::CommandBuffer::new(),
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

        // update camera's viewport
        if let Ok(viewport) = self
            .scene
            .entities
            .query_one_mut::<&mut Viewport>(camera_entity)
        {
            if viewport.viewport != response.rect {
                tracing::debug!(viewport = ?response.rect, "viewport changed");
                viewport.viewport = response.rect;
                self.command_buffer
                    .insert_one(camera_entity, Changed::<Viewport>::default());
            }
        }
        else {
            self.command_buffer.insert_one(
                camera_entity,
                Viewport {
                    viewport: response.rect,
                },
            );
        }

        // we could insert Changed<_> for camera movement and then only update the
        // camera buffer when it actually changes

        response.ctx.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::Vec2 { x: _, y: delta },
                        modifiers: _,
                    } => {
                        if let Ok(camera_transform) = self
                            .scene
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
            if let Ok((camera_transform, camera_projection)) =
                self.scene
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
            if let Ok(camera_transform) = self
                .scene
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

        // todo: always raycast and return the hovered entity. the consumer can then
        // checked if it was clicked or not.
        if response.clicked_by(egui::PointerButton::Primary) {
            if let Some(clicked_entity) = &mut self.clicked_entity {
                **clicked_entity = None;

                if let Some(pointer_position) = interact_pointer_pos() {
                    if let Ok(mut query) = self
                        .scene
                        .entities
                        .query_one::<(&Transform, &CameraProjection)>(camera_entity)
                    {
                        if let Some((camera_transform, camera_projection)) = query.get() {
                            let ray = camera_projection
                                .shoot_screen_ray(&pointer_position)
                                .transform_by(&camera_transform.transform);

                            if let Some(ray_hit) = self.scene.cast_ray(&ray, None) {
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

        // handle inputs
        if response.contains_pointer() {
            self.handle_input(&response);
        }

        // apply any buffered commands to scene
        self.command_buffer.run_on(&mut self.scene.entities);

        // draw frame
        if let Some(draw_frame) = self.renderer.prepare_frame(&self.scene, self.camera_entity) {
            let painter = ui.painter();
            painter.add(egui_wgpu::Callback::new_paint_callback(
                response.rect,
                RenderCallback { draw_frame },
            ));
        }

        response
    }
}

#[derive(Debug)]
struct RenderCallback {
    draw_frame: DrawFrame,
}

impl egui_wgpu::CallbackTrait for RenderCallback {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        self.draw_frame.render(render_pass);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ClickedEntity {
    pub entity: hecs::Entity,
    pub distance_from_camera: f32,
    pub point_clicked: Point3<f32>,
}
