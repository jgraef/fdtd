use bevy_ecs::{
    entity::Entity,
    system::{
        Query,
        Res,
        SystemParam,
    },
};
use nalgebra::{
    Isometry3,
    Point3,
};
use parry3d::{
    bounding_volume::Aabb,
    query::Ray,
};

use crate::{
    spatial::{
        bvh::Bvh,
        collider::Collider,
        merge_aabbs,
    },
    transform::GlobalTransform,
};

#[derive(Debug, SystemParam)]
pub struct RayCast<'w, 's> {
    bvh: Res<'w, Bvh>,
    query: Query<'w, 's, (&'static GlobalTransform, &'static Collider)>,
}

impl<'w, 's> RayCast<'w, 's> {
    pub fn cast_ray(
        &self,
        ray: &Ray,
        max_time_of_impact: impl Into<Option<f32>>,
        filter: impl Fn(Entity) -> bool,
    ) -> Option<RayHit> {
        let max_time_of_impact = max_time_of_impact.into().unwrap_or(f32::MAX);

        self.bvh
            .cast_ray(ray, max_time_of_impact, |entity, best_hit| {
                if filter(entity) {
                    let (transform, collider) = self.query.get(entity).ok()?;
                    collider.cast_ray(transform.isometry(), ray, best_hit, true)
                }
                else {
                    None
                }
            })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub time_of_impact: f32,
    pub entity: Entity,
}

#[derive(Debug, SystemParam)]
pub struct IntersectAabb<'w> {
    bvh: Res<'w, Bvh>,
}

impl<'w> IntersectAabb<'w> {
    pub fn intersect_aabb(&self, aabb: Aabb) -> impl Iterator<Item = (Entity, Aabb)> {
        self.bvh.intersect_aabb(aabb)
    }
}

// todo: put a `Q: QueryData` on this to query more than just the
// entity. this is hard because the iterator could return a mut-borrow from the
// same entity twice (and thus the compiler doesn't allow it).
#[derive(Debug, SystemParam)]
pub struct PointQuery<'w, 's> {
    bvh: Res<'w, Bvh>,
    query: Query<'w, 's, (&'static GlobalTransform, &'static Collider)>,
}

impl<'w, 's> PointQuery<'w, 's> {
    pub fn point_query<'a>(&'a self, point: Point3<f32>) -> impl Iterator<Item = Entity> + 'a {
        self.bvh.point_query(point).filter_map(move |entity| {
            let (transform, collider) = self.query.get(entity).ok()?;
            collider
                .contains_point(transform.isometry(), &point)
                .then_some(entity)
        })
    }
}

/* todo: need a trait for things that can maybe do this
pub fn contact_query<'a>(
    &'a self,
    shape: &dyn Shape,
    transform: &Isometry3<f32>,
    entities: &'a hecs::World,
) -> impl Iterator<Item = (Entity, Contact)> {
    let aabb = shape.compute_aabb(transform);

    let view = entities.view::<(&GlobalTransform, &Collider)>();

    self.intersect_aabb(aabb).filter_map(move |entity| {
        let (other_transform, other_shape) = view.get(entity)?;

        parry3d::query::contact(
            transform,
            shape,
            &other_transform.transform,
            &*other_shape.0,
            0.0,
        )
        .ok()
        .flatten()
        .map(|contact| (entity, contact))
    })
} */

#[derive(Debug, SystemParam)]
pub struct RootAabb<'w> {
    bvh: Res<'w, Bvh>,
}

impl<'w> RootAabb<'w> {
    pub fn get(&self) -> Aabb {
        self.bvh.root_aabb()
    }
}

#[derive(Debug, SystemParam)]
pub struct WorldAabb<'w, 's> {
    bvh: Res<'w, Bvh>,
    query: Query<'w, 's, (&'static GlobalTransform, &'static Collider)>,
}

impl<'w, 's> WorldAabb<'w, 's> {
    pub fn root_aabb(&self) -> Aabb {
        self.bvh.root_aabb()
    }

    /// Computes the scene's AABB relative to an observer.
    ///
    /// # Arguments
    /// - `relative_to`: The individual AABBs of objects in the scene will be
    ///   relative to this, i.e. they wll be transformed by its inverse.
    /// - `approximate_relative_aabbs`: Compute the individual AABBs by
    ///   transforming the pre-computed AABB
    pub fn relative_to_observer(
        &mut self,
        relative_to: &Isometry3<f32>,
        approximate_relative_aabbs: bool,
    ) -> Option<Aabb> {
        let relative_to_inv = relative_to.inverse();

        if approximate_relative_aabbs {
            /*let mut query = self.entities.query::<&Aabb>();
            let aabbs = query
                .iter()
                .map(|(_entity, aabb)| aabb.transform_by(&relative_to_inv));
            merge_aabbs(aabbs)*/
            todo!(
                "fixme: Aabbs are currently not stored because we can't derive Component on them."
            );
        }
        else {
            let aabbs = self.query.iter().map(|(transform, collider)| {
                let transform = relative_to_inv * transform.isometry();
                collider.compute_aabb(&transform)
            });
            merge_aabbs(aabbs)
        }
    }
}
