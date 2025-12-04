use bevy_ecs::entity::Entity;
use cem_scene::{
    Scene,
    transform::LocalTransform,
};
use nalgebra::{
    Point2,
    Point3,
    Translation3,
    Vector2,
    Vector3,
};
use parry3d::query::Ray;

use crate::{
    composer::camera::CameraWorldMut,
    renderer::{
        DrawCommand,
        camera::{
            CameraProjection,
            Viewport,
        },
    },
};

#[derive(derive_more::Debug)]
pub struct SceneView<'a> {
    scene: &'a mut Scene,
    camera_entity: Option<Entity>,
    scene_pointer: Option<&'a mut ScenePointer>,
}

impl<'a> SceneView<'a> {
    pub fn new(scene: &'a mut Scene) -> Self {
        Self {
            scene,
            camera_entity: None,
            scene_pointer: None,
        }
    }

    pub fn with_camera(mut self, camera: Entity) -> Self {
        self.camera_entity = Some(camera);
        self
    }

    pub fn with_scene_pointer(mut self, scene_pointer: &'a mut ScenePointer) -> Self {
        self.scene_pointer = Some(scene_pointer);
        self
    }
}

impl<'a> egui::Widget for SceneView<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let response = ui.allocate_response(
            ui.available_size(),
            egui::Sense::HOVER | egui::Sense::CLICK | egui::Sense::DRAG,
        );

        if let Some(camera_entity) = self.camera_entity {
            let mut camera_proxy = CameraWorldMut {
                world: &mut self.scene.world,
                camera_entity,
            };

            // handle inputs (and resizing)
            handle_input(&mut camera_proxy, self.scene_pointer, &response);

            if !ui.is_sizing_pass()
                && ui.is_rect_visible(response.rect)
                && let Some(draw_command) = camera_proxy.draw_list()
            {
                // draw frame
                let painter = ui.painter();
                painter.add(egui_wgpu::Callback::new_paint_callback(
                    response.rect,
                    PaintCallback { draw_command },
                ));
            }
        }

        response
    }
}

/// Handle widget's inputs
fn handle_input(
    camera_proxy: &mut CameraWorldMut,
    scene_pointer: Option<&mut ScenePointer>,
    response: &egui::Response,
) {
    let camera_pan_tilt_speed = Vector2::repeat(1.0);
    let camera_translation_speed = Vector3::new(0.5, 0.5, 0.1);

    // update camera's viewport
    camera_proxy.update_viewport(Viewport {
        viewport: response.rect,
    });

    // some events (i.e. mouse wheel) we have to read manually, but we only want to
    // do this when the mouse cursor is on top of the view.
    if response.contains_pointer() {
        response.ctx.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::Vec2 { x: _, y },
                        modifiers: _,
                        phase: _,
                    } => {
                        let delta = *y;
                        camera_proxy.with::<&mut LocalTransform, _, _>(
                            move |mut camera_transform| {
                                camera_transform.translate_local(&Translation3::new(
                                    0.0,
                                    0.0,
                                    camera_translation_speed.z * delta,
                                ))
                            },
                        );
                    }
                    egui::Event::Zoom(zoom) => {
                        tracing::debug!(?zoom, "todo: scene view zoom event");
                    }
                    egui::Event::Rotate(rotation) => {
                        tracing::debug!(?rotation, "todo: scene view rotation event");
                    }
                    _ => {}
                }
            }
        });
    }

    let drag_delta = || {
        // drag delta in normalized screen coordinates `[-1, 1]^2`
        let drag_delta = response.drag_delta();
        Vector2::new(
            2.0 * drag_delta.x / response.rect.width(),
            -2.0 * drag_delta.y / response.rect.height(),
        )
    };

    // map from egui's coordinates to `[-1, 1]^2` normalized coordinates (also flips
    // y so that +y is up). this then corresponds to the NDC (without z)
    let pointer_position_to_normalized = |pointer_position: egui::Pos2| {
        // does egui not have a builtin way of doing this?
        Point2::new(
            2.0 * (pointer_position.x - response.interact_rect.left()) / response.rect.width()
                - 1.0,
            -2.0 * (pointer_position.y - response.interact_rect.top()) / response.rect.height()
                + 1.0,
        )
    };

    let _interact_pointer_position = || {
        response
            .interact_pointer_pos()
            .map(pointer_position_to_normalized)
    };

    let pointer_position = || {
        if response.hovered() {
            let mut hover_pos = response.ctx.input(|input| input.pointer.latest_pos())?;

            // if this returns none, we can use the position as is.
            if let Some(transform) = response.ctx.layer_transform_from_global(response.layer_id) {
                hover_pos = transform * hover_pos;
            }

            Some(pointer_position_to_normalized(hover_pos))
        }
        else {
            None
        }
    };

    if response.dragged_by(egui::PointerButton::Primary) {
        let drag_delta = drag_delta().into();
        camera_proxy.with::<(&mut LocalTransform, &CameraProjection), _, _>(
            move |(mut camera_transform, camera_projection)| {
                let drag_angle = camera_projection.unproject_screen(&drag_delta);

                camera_transform.pan_tilt(
                    camera_pan_tilt_speed.x * drag_angle.x,
                    camera_pan_tilt_speed.y * drag_angle.y,
                    &Vector3::y_axis(),
                );
            },
        );
    }
    else if response.dragged_by(egui::PointerButton::Secondary) {
        let drag_delta = drag_delta();
        camera_proxy.with::<&mut LocalTransform, _, _>(move |mut camera_transform| {
            // todo: we need to take the aspect ratio into account when translating
            camera_transform.translate_local(&Translation3::new(
                -camera_translation_speed.x * drag_delta.x,
                -camera_translation_speed.y * drag_delta.y,
                0.0,
            ));
        });
    }

    if let Some(scene_pointer) = scene_pointer {
        scene_pointer.entity_under_pointer = None;
        scene_pointer.ray = None;

        if let Some(pointer_position) = pointer_position() {
            let (ray, ray_hit) = camera_proxy.shoot_ray(pointer_position);

            // todo: move this code out into the composer? the view certainly doesn't know
            // what is selectable and what not. then we can test for the Selectable tag in
            // the filter closure.
            if let Some(ray_hit) = ray_hit {
                let point_hovered = ray.point_at(ray_hit.time_of_impact);

                scene_pointer.entity_under_pointer = Some(EntityUnderPointer {
                    entity: ray_hit.entity,
                    distance_from_camera: ray_hit.time_of_impact,
                    point_hovered,
                });
            }

            scene_pointer.ray = Some(ray);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScenePointer {
    pub ray: Option<Ray>,
    pub entity_under_pointer: Option<EntityUnderPointer>,
}

#[derive(Clone, Copy, Debug)]
pub struct EntityUnderPointer {
    pub entity: Entity,
    pub distance_from_camera: f32,
    pub point_hovered: Point3<f32>,
}

#[derive(Debug)]
struct PaintCallback {
    draw_command: DrawCommand,
}

impl egui_wgpu::CallbackTrait for PaintCallback {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        // todo: this needs to be send through a queue back so that it can be attached
        // to the camera
        let _draw_info = self.draw_command.render(render_pass);
    }
}
