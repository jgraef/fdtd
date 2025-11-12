use std::collections::HashMap;

use parry3d::{
    bounding_volume::{
        Aabb,
        BoundingVolume,
    },
    partitioning as bvh,
    query::Ray,
};

use crate::app::composer::scene::{
    Changed,
    SharedShape,
    Transform,
};

#[derive(derive_more::Debug, Default)]
pub struct OctTree {
    bvh: bvh::Bvh,
    stored_entities: HashMap<u32, hecs::Entity>,
    next_leaf_index: u32,

    #[debug(skip)]
    bvh_workspace: bvh::BvhWorkspace,

    #[debug(skip)]
    command_buffer: hecs::CommandBuffer,
}

impl OctTree {
    /// Remove entities from the octtree
    ///
    /// Call this before [`Self::update`] to remove deleted entities from the
    /// octtree, then remove the entities from the world, and finally call
    /// [`Self::update`].
    pub(super) fn pre_update_removals(
        &mut self,
        world: &mut hecs::World,
        deletions: &[hecs::Entity],
    ) {
        for entity in deletions {
            if let Ok(leaf_index) = world.query_one_mut::<&LeafIndex>(*entity) {
                tracing::debug!(?entity, ?leaf_index, "removing from octtree");
                self.bvh.remove(leaf_index.index);
            }
        }
    }

    pub(super) fn update(&mut self, world: &mut hecs::World) {
        // update changed entities
        for (_entity, (transform, shape, leaf_index, bounding_box)) in world
            .query_mut::<(&Transform, &SharedShape, &LeafIndex, &mut BoundingBox)>()
            .with::<&Collides>()
            .with::<&Changed<Transform>>()
        {
            bounding_box.aabb = shape.compute_aabb(&transform.transform);
            self.bvh
                .insert_or_update_partially(bounding_box.aabb, leaf_index.index, 0.0);
        }

        // insert shapes that don't have a leaf ID yet
        for (entity, (transform, shape)) in world
            .query_mut::<(&Transform, &SharedShape)>()
            .with::<&Collides>()
            .without::<&LeafIndex>()
        {
            let index = self.next_leaf_index;
            self.next_leaf_index += 1;

            tracing::debug!(?entity, index, "adding to octtree");

            let aabb = shape.compute_aabb(&transform.transform);
            self.bvh.insert_or_update_partially(aabb, index, 0.0);

            self.stored_entities.insert(index, entity);
            self.command_buffer
                .insert(entity, (LeafIndex { index }, BoundingBox { aabb }));
        }

        // refit bvh
        self.bvh.refit(&mut self.bvh_workspace);

        self.command_buffer.run_on(world);
    }

    pub fn cast_ray(
        &self,
        ray: &Ray,
        max_time_of_impact: impl Into<Option<f32>>,
        world: &hecs::World,
    ) -> Option<RayHit> {
        let max_time_of_impact = max_time_of_impact.into().unwrap_or(f32::MAX);

        self.bvh
            .cast_ray(ray, max_time_of_impact, |leaf_index, best_hit| {
                self.stored_entities.get(&leaf_index).and_then(|entity| {
                    world
                        .query_one::<(&SharedShape, &Transform)>(*entity)
                        .ok()
                        .and_then(|mut query| {
                            query.get().and_then(|(shape, transform)| {
                                shape.cast_ray(&transform.transform, ray, best_hit, true)
                            })
                        })
                })
            })
            .map(|(leaf_index, time_of_impact)| {
                let entity = self.stored_entities[&leaf_index];
                RayHit {
                    time_of_impact,
                    entity,
                }
            })
    }

    pub fn root_aabb(&self) -> Aabb {
        self.bvh.root_aabb()
    }
}

#[derive(Clone, Copy, Debug)]
struct LeafIndex {
    index: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct BoundingBox {
    pub aabb: Aabb,
}

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub time_of_impact: f32,
    pub entity: hecs::Entity,
}

/// Tag for things that have collisions
#[derive(Clone, Copy, Debug)]
pub struct Collides;

/// Helper to merge an iterator of AABBs
pub fn merge_aabbs<I>(iter: I) -> Option<Aabb>
where
    I: IntoIterator<Item = Aabb>,
{
    iter.into_iter()
        .reduce(|accumulator, aabb| accumulator.merged(&aabb))
}
