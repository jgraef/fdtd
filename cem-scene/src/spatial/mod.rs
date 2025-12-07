mod bvh;
mod collider;
pub mod queries;
mod systems;
pub mod traits;

use bevy_ecs::schedule::{
    IntoScheduleConfigs,
    SystemSet,
};
pub use parry3d::{
    bounding_volume::{
        Aabb,
        BoundingVolume,
    },
    query::{
        Ray,
        RayIntersection,
    },
};

pub use crate::spatial::collider::Collider;
use crate::{
    plugin::Plugin,
    schedule,
    spatial::bvh::{
        Bvh,
        BvhMessage,
    },
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
            .insert_resource(Bvh::default())
            .register_message::<BvhMessage>()
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
