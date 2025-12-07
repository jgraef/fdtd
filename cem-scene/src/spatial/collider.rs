use std::{
    fmt::Debug,
    ops::Deref,
    sync::Arc,
};

use bevy_ecs::{
    component::Component,
    lifecycle::HookContext,
    world::DeferredWorld,
};
use nalgebra::{
    Isometry3,
    Point3,
};
use parry3d::{
    bounding_volume::Aabb,
    query::{
        Ray,
        RayIntersection,
    },
};

use crate::spatial::{
    bvh::BvhMessage,
    traits::{
        AnyCollider,
        ComputeAabb,
        PointQuery,
        RayCast,
    },
};

#[derive(Clone, Component)]
#[component(on_add = collider_added, on_remove = collider_removed)]
pub struct Collider {
    inner: Arc<dyn AnyCollider>,
}

fn collider_added(mut world: DeferredWorld, context: HookContext) {
    world.write_message(BvhMessage::Insert {
        entity: context.entity,
    });
}

fn collider_removed(mut world: DeferredWorld, context: HookContext) {
    world.write_message(BvhMessage::Remove {
        entity: context.entity,
    });
}

impl Debug for Collider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Collider").field(&*self.inner).finish()
    }
}

impl Deref for Collider {
    type Target = dyn AnyCollider;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl Collider {
    pub fn new(value: Arc<dyn AnyCollider>) -> Self {
        Self { inner: value }
    }
}

impl ComputeAabb for Collider {
    fn compute_aabb(&self, transform: &Isometry3<f32>) -> Option<Aabb> {
        self.inner.compute_aabb(transform)
    }
}

impl RayCast for Collider {
    fn cast_ray(
        &self,
        transform: &Isometry3<f32>,
        ray: &Ray,
        max_time_of_impact: f32,
        solid: bool,
    ) -> Option<RayIntersection> {
        self.inner
            .cast_ray(transform, ray, max_time_of_impact, solid)
    }

    fn supported(&self) -> bool {
        RayCast::supported(&*self.inner)
    }
}

impl PointQuery for Collider {
    fn contains_point(&self, transform: &Isometry3<f32>, point: &Point3<f32>) -> bool {
        self.inner.contains_point(transform, point)
    }

    fn supported(&self) -> bool {
        PointQuery::supported(&*self.inner)
    }
}

impl<T> From<T> for Collider
where
    T: parry3d::shape::Shape + Debug,
{
    fn from(value: T) -> Self {
        Collider::new(Arc::new(value))
    }
}
