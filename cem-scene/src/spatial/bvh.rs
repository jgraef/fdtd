use std::collections::HashMap;

use bevy_ecs::{
    component::Component,
    entity::Entity,
    message::Message,
    resource::Resource,
};
use nalgebra::Point3;
use parry3d::{
    bounding_volume::{
        Aabb,
        BoundingVolume,
    },
    partitioning::BvhWorkspace,
    query::Ray,
};

use crate::{
    spatial::{
        queries::RayHit,
        traits::ComputeAabb,
    },
    transform::GlobalTransform,
};

#[derive(Debug, Default, Resource)]
pub struct Bvh {
    bvh: parry3d::partitioning::Bvh,
    leaf_index_map: LeafIndexMap,
}

impl Bvh {
    pub fn transaction<'a>(&'a mut self, workspace: &'a mut BvhWorkspace) -> BvhTransaction<'a> {
        BvhTransaction {
            bvh: self,
            workspace,
            changed: false,
        }
    }

    pub fn root_aabb(&self) -> Aabb {
        self.bvh.root_aabb()
    }

    pub fn cast_ray(
        &self,
        ray: &Ray,
        max_time_of_impact: f32,
        primitive_check: impl Fn(Entity, f32) -> Option<f32>,
    ) -> Option<RayHit> {
        self.bvh
            .cast_ray(ray, max_time_of_impact, |leaf_index, best_hit| {
                let entity = self.leaf_index_map.resolve(leaf_index);
                primitive_check(entity, best_hit)
            })
            .map(|(leaf_index, time_of_impact)| {
                let entity = self.leaf_index_map.resolve(leaf_index);
                RayHit {
                    entity,
                    time_of_impact,
                }
            })
    }

    pub fn intersect_aabb<'a>(&'a self, aabb: Aabb) -> impl Iterator<Item = (Entity, Aabb)> + 'a {
        // note: this is slightly more convenient than the builtin aabb-intersection
        // query as we can move the aabb into the closure

        // note: the leaves iterator doesn't implement
        // any other useful iteration traits, so it's fine to just return an impl here.
        // it would be nice to be able to name the type, but we can't import parry's
        // Leaves iterator anyway.

        self.bvh
            .leaves(move |node| node.aabb().intersects(&aabb))
            .map(|leaf_index| {
                (
                    self.leaf_index_map.resolve(leaf_index),
                    self.bvh.leaf_node(leaf_index).unwrap().aabb(),
                )
            })
    }

    /// This queries all entities that might contain a point.
    ///
    /// The returned entities' colliders needs to be checked if they contain
    /// this point to be exact.
    pub fn point_query<'a>(&'a self, point: Point3<f32>) -> impl Iterator<Item = Entity> + 'a {
        self.bvh
            .leaves(move |node| node.aabb().contains_local_point(&point))
            .map(move |leaf_index| self.leaf_index_map.resolve(leaf_index))
    }
}

#[derive(derive_more::Debug)]
pub struct BvhTransaction<'a> {
    bvh: &'a mut Bvh,
    #[debug(skip)]
    workspace: &'a mut BvhWorkspace,
    changed: bool,
}

impl<'a> BvhTransaction<'a> {
    pub fn insert(
        &mut self,
        entity: Entity,
        transform: &GlobalTransform,
        collider: &impl ComputeAabb,
    ) -> LeafIndex {
        let leaf_index = self.bvh.leaf_index_map.insert(entity);

        let aabb = collider.compute_aabb(transform.isometry());
        self.bvh
            .bvh
            .insert_or_update_partially(aabb, leaf_index, 0.0);

        self.changed = true;

        LeafIndex { leaf_index }
    }

    pub fn remove(&mut self, leaf_index: &LeafIndex) {
        self.bvh.bvh.remove(leaf_index.leaf_index);
        self.changed = true;
    }

    pub fn update(
        &mut self,
        leaf_index: &LeafIndex,
        transform: &GlobalTransform,
        collider: &impl ComputeAabb,
    ) {
        let aabb = collider.compute_aabb(transform.isometry());

        self.bvh
            .bvh
            .insert_or_update_partially(aabb, leaf_index.leaf_index, 0.0);

        self.changed = true;
    }
}

impl<'a> Drop for BvhTransaction<'a> {
    fn drop(&mut self) {
        if self.changed {
            self.bvh.bvh.refit(&mut self.workspace);
        }
    }
}

#[derive(Debug, Message)]
pub enum BvhMessage {
    Insert { entity: Entity },
    Remove { entity: Entity },
}

#[derive(Clone, Copy, Debug, Component)]
pub struct LeafIndex {
    pub leaf_index: u32,
}

#[derive(Clone, Debug, Default)]
struct LeafIndexMap {
    entities: HashMap<u32, Entity>,
    next_leaf_index: u32,
}

impl LeafIndexMap {
    pub fn insert(&mut self, entity: Entity) -> u32 {
        let leaf_index = self.next_leaf_index;
        self.next_leaf_index += 1;
        self.entities.insert(leaf_index, entity);
        leaf_index
    }

    pub fn remove(&mut self, leaf_index: u32) -> Option<Entity> {
        self.entities.remove(&leaf_index)
    }

    pub fn resolve(&self, leaf_index: u32) -> Entity {
        *self
            .entities
            .get(&leaf_index)
            .expect("Leaf index not in stored_entities")
    }
}
