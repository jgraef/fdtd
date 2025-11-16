use nalgebra::{
    Point2,
    Point3,
    Translation3,
    Vector2,
    Vector3,
};
use parry3d::query::Ray;

use crate::app::composer::{
    renderer::{
        Renderer,
        camera::{
            CameraProjection,
            Viewport,
        },
    },
    scene::{
        Changed,
        Scene,
        transform::Transform,
    },
};

#[derive(derive_more::Debug)]
pub struct SceneView<'a> {
    scene: &'a mut Scene,
    renderer: &'a mut Renderer,
    camera_entity: Option<hecs::Entity>,
    scene_pointer: Option<&'a mut ScenePointer>,
}

impl<'a> SceneView<'a> {
    pub fn new(scene: &'a mut Scene, renderer: &'a mut Renderer) -> Self {
        Self {
            scene,
            renderer,
            camera_entity: None,
            scene_pointer: None,
        }
    }

    pub fn with_camera(mut self, camera: hecs::Entity) -> Self {
        self.camera_entity = Some(camera);
        self
    }

    pub fn with_scene_pointer(mut self, scene_pointer: &'a mut ScenePointer) -> Self {
        self.scene_pointer = Some(scene_pointer);
        self
    }

    /// Handle widget's inputs
    fn handle_input(&mut self, response: &egui::Response) {
        // note: we could insert Changed<_> for camera movement and then only update the
        // camera buffer when it actually changes

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
                self.scene
                    .command_buffer
                    .insert_one(camera_entity, Changed::<Viewport>::default());
            }
        }
        else {
            self.scene.command_buffer.insert(
                camera_entity,
                (
                    Viewport {
                        viewport: response.rect,
                    },
                    Changed::<Viewport>::default(),
                ),
            );
        }

        // some events (i.e. mouse wheel) we have to read manually, but we only want to
        // do this when the mouse cursor is on top of the view.
        if response.contains_pointer() {
            response.ctx.input(|input| {
                for event in &input.events {
                    #[allow(clippy::single_match)]
                    match event {
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Line,
                            delta: egui::Vec2 { x: _, y: delta },
                            modifiers: _,
                            phase: _,
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
                if let Some(transform) = response.ctx.layer_transform_from_global(response.layer_id)
                {
                    hover_pos = transform * hover_pos;
                }

                Some(pointer_position_to_normalized(hover_pos))
            }
            else {
                None
            }
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
        else if response.dragged_by(egui::PointerButton::Secondary)
            && let Ok(camera_transform) = self
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

        if let Some(scene_pointer) = &mut self.scene_pointer {
            scene_pointer.entity_under_pointer = None;
            scene_pointer.ray = None;

            if let Some(pointer_position) = pointer_position()
                && let Some(ray) =
                    shoot_ray_from_camera(self.scene, camera_entity, pointer_position)
            {
                // todo: move this code out into the composer? the view certainly doesn't know
                // what is selectable and what not. then we can test for the Selectable tag in
                // the filter closure.
                if let Some(ray_hit) = self.scene.cast_ray(&ray, None, |_entity| true) {
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

        // apply any buffered commands to scene
        self.scene.apply_deferred();
    }

    pub fn shoot_ray_from_camera(&self, pointer_position: Point2<f32>) -> Option<Ray> {
        self.camera_entity.and_then(|camera_entity| {
            shoot_ray_from_camera(self.scene, camera_entity, pointer_position)
        })
    }
}

fn shoot_ray_from_camera(
    scene: &Scene,
    camera_entity: hecs::Entity,
    pointer_position: Point2<f32>,
) -> Option<Ray> {
    scene
        .entities
        .query_one::<(&Transform, &CameraProjection)>(camera_entity)
        .ok()
        .and_then(|mut query| {
            query.get().map(|(camera_transform, camera_projection)| {
                camera_projection
                    .shoot_screen_ray(&pointer_position)
                    .transform_by(&camera_transform.transform)
            })
        })
}

impl<'a> egui::Widget for SceneView<'a> {
    fn ui(mut self, ui: &mut egui::Ui) -> egui::Response {
        let response = ui.allocate_response(
            ui.available_size(),
            egui::Sense::HOVER | egui::Sense::CLICK | egui::Sense::DRAG,
        );

        // handle inputs (and resizing)
        self.handle_input(&response);

        if !ui.is_sizing_pass() && ui.is_rect_visible(response.rect) {
            // draw frame

            if let Some(draw_command) = self.renderer.prepare_frame(
                self.camera_entity
                    .and_then(|camera_entity| self.scene.entities.entity(camera_entity).ok()),
            ) {
                let painter = ui.painter();
                painter.add(egui_wgpu::Callback::new_paint_callback(
                    response.rect,
                    draw_command,
                ));
            }
        }

        response
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScenePointer {
    pub ray: Option<Ray>,
    pub entity_under_pointer: Option<EntityUnderPointer>,
}

#[derive(Clone, Copy, Debug)]
pub struct EntityUnderPointer {
    pub entity: hecs::Entity,
    pub distance_from_camera: f32,
    pub point_hovered: Point3<f32>,
}
