use std::collections::{
    HashMap,
    HashSet,
};

use bevy_ecs::{
    component::Component,
    entity::Entity,
    message::Message,
    resource::Resource,
};
use nalgebra::Point3;
use parry3d::{
    bounding_volume::BoundingVolume,
    partitioning::{
        BvhLeafCost,
        BvhWorkspace,
    },
    query::{
        Ray,
        RayIntersection,
    },
};

use crate::{
    spatial::{
        Aabb,
        queries::RayHit,
        traits::ComputeAabb,
    },
    transform::GlobalTransform,
};

#[derive(Debug, Default, Resource)]
pub struct Bvh {
    bvh: parry3d::partitioning::Bvh,
    leaf_index_map: LeafIndexMap,
    unbounded: HashSet<Entity>,
}

impl Bvh {
    pub fn transaction<'a>(&'a mut self, workspace: &'a mut BvhWorkspace) -> BvhTransaction<'a> {
        BvhTransaction {
            bvh: self,
            workspace,
            bvh_changed: false,
        }
    }

    pub fn root_aabb(&self) -> Aabb {
        self.bvh.root_aabb()
    }

    pub fn cast_ray(
        &self,
        ray: &Ray,
        max_time_of_impact: f32,
        primitive_check: impl Fn(Entity, f32) -> Option<RayIntersection>,
    ) -> Option<RayHit> {
        let mut best_cost = max_time_of_impact;
        let mut best_hit = None;

        // first find the best ray intersection with unbounded colliders
        for entity in &self.unbounded {
            if let Some(ray_hit) = primitive_check(*entity, best_cost) {
                let cost = ray_hit.cost();
                if cost < best_cost {
                    best_cost = cost;
                    best_hit = Some(RayHit {
                        ray_intersection: ray_hit,
                        entity: *entity,
                    });
                }
            }
        }

        // then try to refine the best ray intersection with bounded colliders
        if let Some((_leaf_index, best_bvh_hit)) = self.bvh.find_best(
            best_cost,
            |node, best_hit| node.cast_ray(ray, best_hit),
            |leaf_index, best_hit| {
                let entity = self.leaf_index_map.resolve(leaf_index);
                primitive_check(entity, best_hit).map(|ray_intersection| {
                    RayHit {
                        entity,
                        ray_intersection,
                    }
                })
            },
        ) {
            best_hit = Some(best_bvh_hit);
        }

        best_hit
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
        let unbounded = self.unbounded.iter().copied();

        let bvh_leaves = self
            .bvh
            .leaves(move |node| node.aabb().contains_local_point(&point))
            .map(move |leaf_index| self.leaf_index_map.resolve(leaf_index));

        unbounded.chain(bvh_leaves)
    }
}

#[derive(derive_more::Debug)]
pub struct BvhTransaction<'a> {
    bvh: &'a mut Bvh,
    #[debug(skip)]
    workspace: &'a mut BvhWorkspace,
    bvh_changed: bool,
}

impl<'a> BvhTransaction<'a> {
    pub fn insert(
        &mut self,
        entity: Entity,
        transform: &GlobalTransform,
        collider: &impl ComputeAabb,
    ) -> BvhLeaf {
        if let Some(aabb) = collider.compute_aabb(transform.isometry()) {
            let leaf_index = self.bvh.leaf_index_map.insert(entity);
            self.bvh
                .bvh
                .insert_or_update_partially(aabb, leaf_index, 0.0);
            self.bvh_changed = true;
            BvhLeaf::Aabb { leaf_index, aabb }
        }
        else {
            self.bvh.unbounded.insert(entity);
            BvhLeaf::Unbounded
        }
    }

    pub fn remove(&mut self, entity: Entity, bvh_leaf: &BvhLeaf) {
        match bvh_leaf {
            BvhLeaf::Aabb {
                leaf_index,
                aabb: _,
            } => {
                self.bvh.bvh.remove(*leaf_index);
                self.bvh.leaf_index_map.remove(*leaf_index);
                self.bvh_changed = true;
            }
            BvhLeaf::Unbounded => {
                self.bvh.unbounded.remove(&entity);
            }
        }
    }

    pub fn update(
        &mut self,
        entity: Entity,
        mut bvh_leaf: &mut BvhLeaf,
        transform: &GlobalTransform,
        collider: &impl ComputeAabb,
    ) {
        let aabb = collider.compute_aabb(transform.isometry());

        match (&mut bvh_leaf, aabb) {
            (BvhLeaf::Aabb { leaf_index, aabb }, Some(new_aabb)) => {
                // aabb changed, update in bvh
                self.bvh
                    .bvh
                    .insert_or_update_partially(new_aabb, *leaf_index, 0.0);
                self.bvh_changed = true;
                *aabb = new_aabb;
            }
            (BvhLeaf::Unbounded, None) => {
                // collider was unbounded before and is now, so nothing to
                // update.
            }
            (
                BvhLeaf::Aabb {
                    leaf_index,
                    aabb: _,
                },
                None,
            ) => {
                // aabb is now infinite
                self.bvh.bvh.remove(*leaf_index);
                self.bvh_changed = true;
                *bvh_leaf = BvhLeaf::Unbounded;
            }
            (BvhLeaf::Unbounded, Some(new_aabb)) => {
                // collider was unbounded, but now has a bounded aabb
                let leaf_index = self.bvh.leaf_index_map.insert(entity);
                self.bvh
                    .bvh
                    .insert_or_update_partially(new_aabb, leaf_index, 0.0);
                self.bvh_changed = true;
                *bvh_leaf = BvhLeaf::Aabb {
                    leaf_index,
                    aabb: new_aabb,
                };
            }
        }
    }
}

impl<'a> Drop for BvhTransaction<'a> {
    fn drop(&mut self) {
        if self.bvh_changed {
            self.bvh.bvh.refit(self.workspace);
        }
    }
}

#[derive(Debug, Message)]
pub enum BvhMessage {
    Insert { entity: Entity },
    Remove { entity: Entity },
}

#[derive(Clone, Copy, Debug, Component)]
pub enum BvhLeaf {
    Aabb { leaf_index: u32, aabb: Aabb },
    Unbounded,
}

impl BvhLeaf {
    pub fn aabb(&self) -> Option<Aabb> {
        match self {
            BvhLeaf::Aabb {
                leaf_index: _,
                aabb,
            } => Some(*aabb),
            BvhLeaf::Unbounded => None,
        }
    }
}

/// Maps leaf indices to entities
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
