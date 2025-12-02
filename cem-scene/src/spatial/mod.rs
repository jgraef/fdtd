mod bvh;
mod collider;
pub mod queries;
mod systems;
pub mod traits;

use bevy_ecs::schedule::{
    IntoScheduleConfigs,
    SystemSet,
};
pub use parry3d::bounding_volume::Aabb;
use parry3d::bounding_volume::BoundingVolume;

pub use crate::spatial::collider::Collider;
use crate::{
    plugin::Plugin,
    schedule,
    transform::TransformSystems,
};

/// Helper to merge an iterator of AABBs
pub fn merge_aabbs<I>(iter: I) -> Option<Aabb>
where
    I: IntoIterator<Item = Aabb>,
{
    iter.into_iter()
        .reduce(|accumulator, aabb| accumulator.merged(&aabb))
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub enum SpatialSystems {
    BvhUpdate,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SpatialQueryPlugin;

impl Plugin for SpatialQueryPlugin {
    fn setup(&self, builder: &mut crate::SceneBuilder) {
        builder
            .add_systems(
                schedule::PostStartup,
                systems::update_bvh
                    .in_set(SpatialSystems::BvhUpdate)
                    .after(TransformSystems::Propagate),
            )
            .add_systems(
                schedule::PostUpdate,
                systems::update_bvh
                    .in_set(SpatialSystems::BvhUpdate)
                    .after(TransformSystems::Propagate),
            );
    }
}
