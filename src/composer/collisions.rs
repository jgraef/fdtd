use std::collections::HashMap;

use parry3d::{
    partitioning as bvh,
    query::Ray,
};

use crate::composer::scene::{
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
    pub fn update(&mut self, world: &mut hecs::World) {
        // update changed entities
        for (_entity, (transform, shape, leaf_index)) in world
            .query_mut::<(&Transform, &SharedShape, &LeafIndex)>()
            .with::<&Changed<Transform>>()
        {
            self.insert_or_update_node(transform, shape, leaf_index.0);
        }

        // insert shapes that don't have a leaf ID yet
        for (entity, (transform, shape)) in world
            .query_mut::<(&Transform, &SharedShape)>()
            .without::<&LeafIndex>()
        {
            let leaf_index = self.next_leaf_index;
            self.next_leaf_index += 1;

            tracing::debug!(?entity, leaf_index, "adding to octtree");

            self.insert_or_update_node(transform, shape, leaf_index);
            self.stored_entities.insert(leaf_index, entity);
            self.command_buffer
                .insert_one(entity, LeafIndex(leaf_index));
        }

        // refit bvh
        self.bvh.refit(&mut self.bvh_workspace);

        self.command_buffer.run_on(world);
    }

    fn insert_or_update_node(
        &mut self,
        transform: &Transform,
        shape: &SharedShape,
        leaf_index: u32,
    ) {
        let aabb = shape.compute_aabb(&transform.transform);
        self.bvh.insert_or_update_partially(aabb, leaf_index, 0.0);
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LeafIndex(u32);

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub time_of_impact: f32,
    pub entity: hecs::Entity,
}
