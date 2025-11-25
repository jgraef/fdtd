use std::{
    collections::HashMap,
    fmt::Debug,
    ops::Deref,
    sync::Arc,
};

use nalgebra::{
    Isometry3,
    Point3,
};
use parry3d::{
    bounding_volume::{
        Aabb,
        BoundingVolume,
    },
    partitioning as bvh,
    query::Ray,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::app::composer::scene::{
    Changed,
    transform::Transform,
};

#[derive(derive_more::Debug, Default)]
pub struct SpatialQueries {
    bvh: bvh::Bvh,
    stored_entities: HashMap<u32, hecs::Entity>,
    next_leaf_index: u32,

    #[debug(skip)]
    bvh_workspace: bvh::BvhWorkspace,
}

impl SpatialQueries {
    pub(super) fn remove(&mut self, entity: hecs::Entity, world: &mut hecs::World) {
        if let Ok(leaf_index) = world.remove_one::<LeafIndex>(entity) {
            tracing::debug!(?entity, index = leaf_index.index, "removing from octtree");

            // do we need to do this?
            //let _ = world.remove_one::<BoundingBox>(entity);

            self.bvh.remove(leaf_index.index);
            self.stored_entities.remove(&leaf_index.index);
        }
    }

    pub(super) fn update(
        &mut self,
        world: &mut hecs::World,
        command_buffer: &mut hecs::CommandBuffer,
    ) {
        // update changed entities
        for (entity, (transform, collider, leaf_index, bounding_box)) in world
            .query_mut::<(&Transform, &Collider, &LeafIndex, &mut BoundingBox)>()
            .with::<&Changed<Transform>>()
        {
            tracing::debug!(?entity, "transform changed");

            bounding_box.aabb = collider.compute_aabb(&transform.transform);
            self.bvh
                .insert_or_update_partially(bounding_box.aabb, leaf_index.index, 0.0);
        }

        // remove tracked entities that have no transform or collider anymore
        for (entity, ()) in world
            .query_mut::<()>()
            .with::<&LeafIndex>()
            .without::<hecs::Or<&Transform, &Collider>>()
        {
            command_buffer.remove_one::<LeafIndex>(entity);
            command_buffer.remove_one::<BoundingBox>(entity);
        }

        // insert colliders that don't have a leaf ID yet
        for (entity, (transform, collider)) in world
            .query_mut::<(&Transform, &Collider)>()
            .without::<&LeafIndex>()
        {
            let index = self.next_leaf_index;
            self.next_leaf_index += 1;

            tracing::debug!(?entity, index, "adding to octtree");

            let aabb = collider.compute_aabb(&transform.transform);
            self.bvh.insert_or_update_partially(aabb, index, 0.0);

            self.stored_entities.insert(index, entity);
            command_buffer.insert(entity, (LeafIndex { index }, BoundingBox { aabb }));
        }

        // refit bvh
        self.bvh.refit(&mut self.bvh_workspace);

        command_buffer.run_on(world);
    }

    pub fn cast_ray(
        &self,
        ray: &Ray,
        max_time_of_impact: impl Into<Option<f32>>,
        world: &hecs::World,
        filter: impl Fn(hecs::Entity) -> bool,
    ) -> Option<RayHit> {
        let max_time_of_impact = max_time_of_impact.into().unwrap_or(f32::MAX);

        let view = world.view::<(&Transform, &Collider)>();

        self.bvh
            .cast_ray(ray, max_time_of_impact, |leaf_index, best_hit| {
                let entity = self.stored_entities.get(&leaf_index)?;
                if filter(*entity) {
                    let (transform, collider) = view.get(*entity)?;
                    collider.cast_ray(&transform.transform, ray, best_hit, true)
                }
                else {
                    None
                }
            })
            .map(|(leaf_index, time_of_impact)| {
                let entity = self.stored_entities[&leaf_index];
                RayHit {
                    time_of_impact,
                    entity,
                }
            })
    }

    fn intersect_aabb<'a>(&'a self, aabb: Aabb) -> impl Iterator<Item = hecs::Entity> + 'a {
        // note: this is slightly more convenient than the builtin aabb-intersection
        // query as we can move the aabb into the closure

        // note: the leaves iterator doesn't implement
        // any other useful iteration traits, so it's fine to just return an impl here.
        // it would be nice to be able to name the type, but we can't import parry's
        // Leaves iterator anyway.

        self.bvh
            .leaves(move |node| node.aabb().intersects(&aabb))
            .filter_map(|leaf_index| self.stored_entities.get(&leaf_index).copied())
    }

    pub fn point_query<'a>(
        &'a self,
        point: Point3<f32>,
        entities: &'a hecs::World,
    ) -> impl Iterator<Item = hecs::Entity> + 'a {
        let aabb = Aabb {
            mins: point,
            maxs: point,
        };

        let view = entities.view::<(&Transform, &Collider)>();

        self.intersect_aabb(aabb).filter_map(move |entity| {
            let (transform, collider) = view.get(entity)?;

            collider
                .contains_point(&transform.transform, &point)
                .then_some(entity)
        })
    }

    /* todo: need a trait for things that can maybe do this
    pub fn contact_query<'a>(
        &'a self,
        shape: &dyn Shape,
        transform: &Isometry3<f32>,
        entities: &'a hecs::World,
    ) -> impl Iterator<Item = (hecs::Entity, Contact)> {
        let aabb = shape.compute_aabb(transform);

        let view = entities.view::<(&Transform, &Collider)>();

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

    pub fn root_aabb(&self) -> Aabb {
        self.bvh.root_aabb()
    }
}

#[derive(Clone, Copy, Debug)]
struct LeafIndex {
    index: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BoundingBox {
    pub aabb: Aabb,
}

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub time_of_impact: f32,
    pub entity: hecs::Entity,
}

/// Helper to merge an iterator of AABBs
pub fn merge_aabbs<I>(iter: I) -> Option<Aabb>
where
    I: IntoIterator<Item = Aabb>,
{
    iter.into_iter()
        .reduce(|accumulator, aabb| accumulator.merged(&aabb))
}

#[derive(Clone, Debug)]
pub struct Collider(pub Arc<dyn ColliderTraits>);

impl<S: ColliderTraits> From<S> for Collider {
    fn from(value: S) -> Self {
        Self(Arc::new(value))
    }
}

impl Deref for Collider {
    type Target = dyn ColliderTraits;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

pub trait ColliderTraits:
    ComputeAabb + RayCast + PointQuery + Debug + Send + Sync + 'static
{
}

impl<T> ColliderTraits for T where
    T: ComputeAabb + RayCast + PointQuery + Debug + Send + Sync + 'static
{
}

pub trait ComputeAabb {
    fn compute_aabb(&self, transform: &Isometry3<f32>) -> Aabb;
}

impl<T> ComputeAabb for T
where
    T: parry3d::shape::Shape,
{
    fn compute_aabb(&self, transform: &Isometry3<f32>) -> Aabb {
        parry3d::shape::Shape::compute_aabb(self, transform)
    }
}

pub trait RayCast {
    fn supported(&self) -> bool {
        true
    }

    fn cast_ray(
        &self,
        transform: &Isometry3<f32>,
        ray: &Ray,
        max_time_of_impact: f32,
        solid: bool,
    ) -> Option<f32>;
}

impl<T> RayCast for T
where
    T: parry3d::query::RayCast,
{
    fn cast_ray(
        &self,
        transform: &Isometry3<f32>,
        ray: &Ray,
        max_time_of_impact: f32,
        solid: bool,
    ) -> Option<f32> {
        parry3d::query::RayCast::cast_ray(self, transform, ray, max_time_of_impact, solid)
    }
}

pub trait PointQuery {
    fn supported(&self) -> bool {
        true
    }

    fn contains_point(&self, transform: &Isometry3<f32>, point: &Point3<f32>) -> bool;
}

impl<T> PointQuery for T
where
    T: parry3d::query::PointQuery,
{
    fn contains_point(&self, transform: &Isometry3<f32>, point: &Point3<f32>) -> bool {
        parry3d::query::PointQuery::contains_point(self, transform, point)
    }
}

#[cfg(test)]
mod tests {
    use parry3d::shape::Ball;

    use crate::app::composer::scene::{
        Transform,
        spatial::{
            Collider,
            SpatialQueries,
        },
    };

    fn test_bundle() -> impl hecs::DynamicBundle {
        (Collider::from(Ball::new(1.0)), Transform::identity())
    }

    #[test]
    fn it_adds_entities() {
        let mut world = hecs::World::new();
        let mut command_buffer = hecs::CommandBuffer::new();
        let mut octtree = SpatialQueries::default();

        let entity = world.spawn(test_bundle());
        octtree.update(&mut world, &mut command_buffer);

        octtree.bvh.assert_well_formed();
        let leaves = octtree.bvh.leaves(|_| true).collect::<Vec<_>>();
        assert_eq!(leaves.len(), 1);
        assert_eq!(
            octtree.stored_entities.get(&leaves[0]).copied(),
            Some(entity)
        );
    }

    #[test]
    fn it_removes_entities() {
        let mut world = hecs::World::new();
        let mut command_buffer = hecs::CommandBuffer::new();
        let mut octtree = SpatialQueries::default();

        let entity = world.spawn(test_bundle());
        octtree.update(&mut world, &mut command_buffer);

        octtree.remove(entity, &mut world);
        octtree.bvh.assert_well_formed(); // ?

        world.despawn(entity).unwrap();

        octtree.update(&mut world, &mut command_buffer);
        octtree.bvh.assert_well_formed();
        assert!(octtree.bvh.is_empty());
        assert!(octtree.stored_entities.is_empty());
    }
}
