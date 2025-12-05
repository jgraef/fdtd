use bevy_ecs::{
    entity::Entity,
    query::QueryData,
    system::{
        Commands,
        In,
        Query,
    },
    world::{
        Mut,
        World,
    },
};
use cem_scene::{
    spatial::queries::{
        RayCast,
        RayHit,
        WorldAabb,
    },
    transform::{
        GlobalTransform,
        LocalTransform,
    },
};
use nalgebra::{
    Isometry3,
    Point2,
    Translation3,
    UnitQuaternion,
    Vector2,
    Vector3,
};
use parry3d::query::Ray;

use crate::renderer::{
    DrawCommand,
    camera::{
        CameraProjection,
        Viewport,
    },
    grab_draw_list_for_camera,
};

/// A proxy to control a camera in a world.
#[derive(Debug)]
pub struct CameraWorldMut<'a> {
    pub world: &'a mut World,
    pub camera_entity: Entity,
}

impl<'a> CameraWorldMut<'a> {
    /// Grabs the draw list for a camera from the scene
    pub fn draw_list(&mut self) -> Option<DrawCommand> {
        self.world
            .run_system_cached_with(grab_draw_list_for_camera, self.camera_entity)
            .unwrap()
    }

    pub fn update_viewport(&mut self, viewport: Viewport) {
        self.world
            .run_system_cached_with(
                |In((camera_entity, viewport)): In<(Entity, Viewport)>,
                 mut cameras: Query<Option<Mut<Viewport>>>,
                 mut commands: Commands| {
                    if let Some(mut camera_viewport) = cameras.get_mut(camera_entity).unwrap() {
                        if *camera_viewport != viewport {
                            tracing::debug!(?viewport, "viewport changed");
                            *camera_viewport = viewport;
                        }
                    }
                    else {
                        tracing::debug!(?viewport, "viewport added");
                        commands.entity(camera_entity).insert(viewport);
                    }
                },
                (self.camera_entity, viewport),
            )
            .unwrap();
    }

    pub fn with<Q, F, R>(&mut self, f: F) -> R
    where
        for<'w, 's> F: FnMut(Q::Item<'w, 's>) -> R + 'static,
        R: 'static,
        Q: QueryData + 'static,
    {
        self.world
            .run_system_cached_with(
                |In((camera_entity, mut f)): In<(Entity, F)>, mut cameras: Query<Q>| {
                    let item = cameras.get_mut(camera_entity).unwrap();
                    f(item)
                },
                (self.camera_entity, f),
            )
            .unwrap()
    }

    /// Shoots a ray from the camera *pew pew pew*
    ///
    /// # Returns
    ///
    ///  - a [`Ray`] with origin at the camera position in the direction of
    /// where `pointer_position` is pointing at in the projected image.
    ///  - if the ray hits an entity, a [`RayHit`] with the [`Entity`] and
    ///    distance along the ray.
    pub fn shoot_ray(&mut self, pointer_position: Point2<f32>) -> (Ray, Option<RayHit>) {
        self.world
            .run_system_cached_with(
                |In((camera_entity, pointer_position)): In<(Entity, Point2<f32>)>,
                 cameras: Query<(&GlobalTransform, &CameraProjection)>,
                 ray_cast: RayCast| {
                    let (camera_transform, camera_projection) = cameras.get(camera_entity).unwrap();
                    let ray = camera_projection
                        .shoot_screen_ray(&pointer_position)
                        .transform_by(camera_transform.isometry());
                    let ray_hit = ray_cast.cast_ray(&ray, None, |_| true);
                    (ray, ray_hit)
                },
                (self.camera_entity, pointer_position),
            )
            .unwrap()
    }

    /// Moves the camera such that it fits the whole scene.
    ///
    /// Specifically this only translates the camera. It will be translated (by
    /// moving backwards) such that it will fit the AABB of the scene. The
    /// AABB is calculated relative to the camera orientation. The camera will
    /// also be translated laterally to its view axis to center to the AABB.
    pub fn fit_to_scene(&mut self, margin: &Vector2<f32>) {
        self.world
            .run_system_cached_with(
                |In((camera_entity, margin)): In<(Entity, Vector2<f32>)>,
                 mut cameras: Query<(&GlobalTransform, &mut LocalTransform, &CameraProjection)>,
                 mut world_aabb: WorldAabb| {
                    // get camera transform and projection
                    // note: we could use another transform if we want to reposition the camera e.g.
                    // along a coordinate axis.
                    let Ok((
                        camera_global_transform,
                        mut camera_local_transform,
                        camera_projection,
                    )) = cameras.get_mut(camera_entity)
                    else {
                        return;
                    };

                    // compute scene AABB relative to camera
                    let Some(scene_aabb) =
                        world_aabb.relative_to_observer(camera_global_transform.isometry(), false)
                    else {
                        return;
                    };

                    // center camera on aabb
                    let mut translation = scene_aabb.center().coords;
                    translation.z -=
                        camera_projection.distance_to_fit_aabb_into_fov(&scene_aabb, &margin);

                    // apply translation to camera
                    camera_local_transform.translate_local(&Translation3::from(translation));
                },
                (self.camera_entity, *margin),
            )
            .unwrap();
    }

    /// Fit the camera to the scene looking along a specified axis.
    ///
    /// This is meant to be used along the canonical axis of the scene. It will
    /// not calculate the scene's AABB as viewed along the axis, but instead
    /// just rotate the scene's AABB.
    pub fn fit_to_scene_looking_along_axis(
        &mut self,
        axis: &Vector3<f32>,
        up: &Vector3<f32>,
        margin: &Vector2<f32>,
    ) {
        self.world
            .run_system_cached_with(
                |In((camera_entity, axis, up, margin)): In<(
                    Entity,
                    Vector3<f32>,
                    Vector3<f32>,
                    Vector2<f32>,
                )>,
                 mut cameras: Query<(&mut LocalTransform, &CameraProjection)>,
                 world_aabb: WorldAabb| {
                    let scene_aabb = world_aabb.root_aabb();

                    let Ok((mut camera_local_transform, camera_projection)) =
                        cameras.get_mut(camera_entity)
                    else {
                        return;
                    };

                    let rotation = UnitQuaternion::face_towards(&axis, &up);

                    let reference_transform =
                        Isometry3::from_parts(Translation3::identity(), rotation);

                    let scene_aabb = scene_aabb.transform_by(&reference_transform);

                    let distance =
                        camera_projection.distance_to_fit_aabb_into_fov(&scene_aabb, &margin);

                    let mut new_local = LocalTransform::from(Isometry3::from_parts(
                        Translation3::from(scene_aabb.center().coords),
                        rotation,
                    ));
                    new_local.translate_local(&Translation3::from(-Vector3::z() * distance));

                    // FIXME: this doesn't work anymore if the camera has a parent
                    *camera_local_transform = new_local;
                },
                (self.camera_entity, *axis, *up, *margin),
            )
            .unwrap();
    }

    pub fn point_to_scene_center(&mut self) {
        self.world
            .run_system_cached_with(
                |In(camera_entity): In<Entity>,
                 world_aabb: WorldAabb,
                 mut cameras: Query<&mut LocalTransform>| {
                    let scene_center = world_aabb.root_aabb().center();

                    let Ok(mut camera_transform) = cameras.get_mut(camera_entity)
                    else {
                        return;
                    };

                    let eye = camera_transform.position();

                    // normally up is always +Y
                    let mut up = Vector3::y();

                    // but we need to take into account when we're directly above the scene center
                    const COLLINEAR_THRESHOLD: f32 = 0.01f32.to_radians();
                    if (eye - scene_center).cross(&up).norm_squared() < COLLINEAR_THRESHOLD {
                        // we would be looking straight up or down, so keep the up vector from the
                        // camera
                        up = camera_transform.isometry.rotation.transform_vector(&up);
                        tracing::debug!(?eye, ?scene_center, ?up, "looking straight up or down");
                    }

                    *camera_transform = LocalTransform::look_at(&eye, &scene_center, &up);
                },
                self.camera_entity,
            )
            .unwrap();
    }
}
