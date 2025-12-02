use std::{
    collections::HashMap,
    fmt::Debug,
    ops::Deref,
    sync::Arc,
};

use cem_util::format_size;
use nalgebra::{
    Isometry3,
    Point3,
};
use parry3d::{
    bounding_volume::{
        Aabb,
        BoundingVolume,
    },
    partitioning::{
        Bvh,
        BvhWorkspace,
    },
    query::Ray,
};

use crate::{
    debug::DebugUi,
    impl_register_component,
    scene::{
        Changed,
        Scene,
        transform::GlobalTransform,
    },
    util::egui::probe::{
        PropertiesUi,
        TrackChanges,
        label_and_value,
    },
};

#[derive(derive_more::Debug, Default)]
pub struct SpatialQueries {
    bvh: Bvh,
    leaf_index_map: LeafIndexMap,

    #[debug("BvhWorkspace {{ ... }}")]
    bvh_workspace: BvhWorkspace,
}

impl SpatialQueries {
    pub(super) fn remove(
        &mut self,
        entity: hecs::Entity,
        world: &mut hecs::World,
        command_buffer: &mut hecs::CommandBuffer,
    ) {
        if let Ok(leaf_index) = world.remove_one::<LeafIndex>(entity) {
            tracing::debug!(
                ?entity,
                index = leaf_index.leaf_index,
                "removing from octtree"
            );

            self.bvh.remove(leaf_index.leaf_index);
            self.leaf_index_map.remove(leaf_index.leaf_index);
            command_buffer.remove_one::<Aabb>(entity);
        }
    }

    pub(super) fn update(
        &mut self,
        world: &mut hecs::World,
        command_buffer: &mut hecs::CommandBuffer,
    ) {
        // update changed entities
        for (_entity, (transform, collider, leaf_index, aabb)) in world
            .query_mut::<(&GlobalTransform, &Collider, &LeafIndex, &mut Aabb)>()
            .with::<&Changed<GlobalTransform>>()
        {
            *aabb = collider.compute_aabb(transform.isometry());

            self.bvh
                .insert_or_update_partially(*aabb, leaf_index.leaf_index, 0.0);
        }

        // remove tracked entities that have no transform or collider anymore
        for (entity, ()) in world
            .query_mut::<()>()
            .with::<&LeafIndex>()
            .without::<hecs::Or<&GlobalTransform, &Collider>>()
        {
            command_buffer.remove_one::<LeafIndex>(entity);
            command_buffer.remove_one::<Aabb>(entity);
        }

        // insert colliders that don't have a leaf ID yet
        for (entity, (transform, collider)) in world
            .query_mut::<(&GlobalTransform, &Collider)>()
            .without::<&LeafIndex>()
        {
            let leaf_index = self.leaf_index_map.insert(entity);

            tracing::debug!(?entity, leaf_index, "adding to octtree");

            let aabb = collider.compute_aabb(transform.isometry());
            self.bvh.insert_or_update_partially(aabb, leaf_index, 0.0);

            command_buffer.insert(entity, (LeafIndex { leaf_index }, aabb));
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

        let view = world.view::<(&GlobalTransform, &Collider)>();

        self.bvh
            .cast_ray(ray, max_time_of_impact, |leaf_index, best_hit| {
                let entity = self.leaf_index_map.resolve(leaf_index);
                if filter(entity) {
                    let (transform, collider) = view.get(entity)?;
                    collider.cast_ray(transform.isometry(), ray, best_hit, true)
                }
                else {
                    None
                }
            })
            .map(|(leaf_index, time_of_impact)| {
                let entity = self.leaf_index_map.resolve(leaf_index);
                RayHit {
                    time_of_impact,
                    entity,
                }
            })
    }

    pub fn intersect_aabb<'a>(&'a self, aabb: Aabb) -> impl Iterator<Item = hecs::Entity> + 'a {
        // note: this is slightly more convenient than the builtin aabb-intersection
        // query as we can move the aabb into the closure

        // note: the leaves iterator doesn't implement
        // any other useful iteration traits, so it's fine to just return an impl here.
        // it would be nice to be able to name the type, but we can't import parry's
        // Leaves iterator anyway.

        self.bvh
            .leaves(move |node| node.aabb().intersects(&aabb))
            .map(|leaf_index| self.leaf_index_map.resolve(leaf_index))
    }

    pub fn point_query<'a>(
        &'a self,
        point: Point3<f32>,
        entities: &'a hecs::World,
    ) -> impl Iterator<Item = hecs::Entity> + 'a {
        let view = entities.view::<(&GlobalTransform, &Collider)>();

        self.bvh
            .leaves(move |node| node.aabb().contains_local_point(&point))
            .filter_map(move |leaf_index| {
                let entity = self.leaf_index_map.resolve(leaf_index);
                let (transform, collider) = view.get(entity)?;
                collider
                    .contains_point(transform.isometry(), &point)
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

    pub fn root_aabb(&self) -> Aabb {
        self.bvh.root_aabb()
    }
}

#[derive(Clone, Copy, Debug)]
struct LeafIndex {
    leaf_index: u32,
}

#[derive(Clone, Debug, Default)]
struct LeafIndexMap {
    entities: HashMap<u32, hecs::Entity>,
    next_leaf_index: u32,
}

impl LeafIndexMap {
    fn insert(&mut self, entity: hecs::Entity) -> u32 {
        let leaf_index = self.next_leaf_index;
        self.next_leaf_index += 1;
        self.entities.insert(leaf_index, entity);
        leaf_index
    }

    fn remove(&mut self, leaf_index: u32) -> Option<hecs::Entity> {
        self.entities.remove(&leaf_index)
    }

    fn resolve(&self, leaf_index: u32) -> hecs::Entity {
        *self
            .entities
            .get(&leaf_index)
            .expect("Leaf index not in stored_entities")
    }
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

#[derive(Clone)]
pub struct Collider {
    inner: Arc<dyn AnyCollider>,
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
    fn compute_aabb(&self, transform: &Isometry3<f32>) -> Aabb {
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
    ) -> Option<f32> {
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

pub trait AnyCollider: ComputeAabb + RayCast + PointQuery + Debug + Send + Sync + 'static {}

impl<T> AnyCollider for T where T: ComputeAabb + RayCast + PointQuery + Debug + Send + Sync + 'static
{}

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

impl<T> From<T> for Collider
where
    T: parry3d::shape::Shape + Debug,
{
    fn from(value: T) -> Self {
        Collider::new(Arc::new(value))
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

impl DebugUi for SpatialQueries {
    fn show_debug(&self, ui: &mut egui::Ui) {
        ui.label(format!(
            "Bvh size: {}",
            format_size(self.bvh.total_memory_size())
        ));
    }
}

impl PropertiesUi for Aabb {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                label_and_value(ui, "Min", &mut changes, &mut self.mins);
                label_and_value(ui, "Max", &mut changes, &mut self.maxs);
            })
            .response;

        changes.propagated(response)
    }
}

impl_register_component!(Aabb where ComponentUi);

pub trait SceneSpatialExt {
    fn aabb(&self) -> Aabb;

    fn cast_ray(
        &self,
        ray: &Ray,
        max_time_of_impact: impl Into<Option<f32>>,
        filter: impl Fn(hecs::Entity) -> bool,
    ) -> Option<RayHit>;

    fn point_query(&self, point: &Point3<f32>) -> impl Iterator<Item = hecs::Entity>;

    fn intersect_aabb<'a>(&'a self, aabb: Aabb) -> impl Iterator<Item = hecs::Entity> + 'a;

    /// Computes the scene's AABB relative to an observer.
    ///
    /// # Arguments
    /// - `relative_to`: The individual AABBs of objects in the scene will be
    ///   relative to this, i.e. they wll be transformed by its inverse.
    /// - `approximate_relative_aabbs`: Compute the individual AABBs by
    ///   transforming the pre-computed AABB
    fn compute_aabb_relative_to_observer(
        &self,
        relative_to: &Isometry3<f32>,
        approximate_relative_aabbs: bool,
    ) -> Option<Aabb>;
}

impl SceneSpatialExt for Scene {
    fn aabb(&self) -> Aabb {
        self.resources.expect::<SpatialQueries>().root_aabb()
    }

    fn cast_ray(
        &self,
        ray: &Ray,
        max_time_of_impact: impl Into<Option<f32>>,
        filter: impl Fn(hecs::Entity) -> bool,
    ) -> Option<RayHit> {
        self.resources.expect::<SpatialQueries>().cast_ray(
            ray,
            max_time_of_impact,
            &self.entities,
            filter,
        )
    }

    fn point_query(&self, point: &Point3<f32>) -> impl Iterator<Item = hecs::Entity> {
        self.resources
            .expect::<SpatialQueries>()
            .point_query(*point, &self.entities)
    }

    fn intersect_aabb<'a>(&'a self, aabb: Aabb) -> impl Iterator<Item = hecs::Entity> + 'a {
        self.resources
            .expect::<SpatialQueries>()
            .intersect_aabb(aabb)
    }

    /// Computes the scene's AABB relative to an observer.
    ///
    /// # Arguments
    /// - `relative_to`: The individual AABBs of objects in the scene will be
    ///   relative to this, i.e. they wll be transformed by its inverse.
    /// - `approximate_relative_aabbs`: Compute the individual AABBs by
    ///   transforming the pre-computed AABB
    fn compute_aabb_relative_to_observer(
        &self,
        relative_to: &Isometry3<f32>,
        approximate_relative_aabbs: bool,
    ) -> Option<Aabb> {
        let relative_to_inv = relative_to.inverse();

        if approximate_relative_aabbs {
            let mut query = self.entities.query::<&Aabb>();
            let aabbs = query
                .iter()
                .map(|(_entity, aabb)| aabb.transform_by(&relative_to_inv));
            merge_aabbs(aabbs)
        }
        else {
            let mut query = self.entities.query::<(&GlobalTransform, &Collider)>();
            let aabbs = query.iter().map(|(_entity, (transform, collider))| {
                let transform = relative_to_inv * transform.isometry();
                collider.compute_aabb(&transform)
            });
            merge_aabbs(aabbs)
        }
    }
}

#[cfg(test)]
mod tests {
    use parry3d::shape::Ball;

    use crate::scene::{
        GlobalTransform,
        spatial::{
            Collider,
            SpatialQueries,
        },
    };

    fn test_bundle() -> impl hecs::DynamicBundle {
        (
            Collider::from(Ball::new(1.0)),
            GlobalTransform::new_test(Default::default()),
        )
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
        assert_eq!(octtree.leaf_index_map.resolve(leaves[0]), entity);
    }

    #[test]
    fn it_removes_entities() {
        let mut world = hecs::World::new();
        let mut command_buffer = hecs::CommandBuffer::new();
        let mut octtree = SpatialQueries::default();

        let entity = world.spawn(test_bundle());
        octtree.update(&mut world, &mut command_buffer);

        octtree.remove(entity, &mut world, &mut command_buffer);
        octtree.bvh.assert_well_formed(); // ?

        world.despawn(entity).unwrap();

        octtree.update(&mut world, &mut command_buffer);
        octtree.bvh.assert_well_formed();
        assert!(octtree.bvh.is_empty());
        assert!(octtree.leaf_index_map.entities.is_empty());
    }
}
