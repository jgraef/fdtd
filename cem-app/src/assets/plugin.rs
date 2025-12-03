use bevy_ecs::schedule::{
    IntoScheduleConfigs,
    SystemSet,
};
use cem_scene::{
    SceneBuilder,
    plugin::Plugin,
    schedule,
};

use crate::assets::{
    LoadAsset,
    systems::{
        poll_loaders,
        start_loading,
    },
};

#[derive(Clone, Copy, Debug)]
pub struct AssetPlugin;

impl Plugin for AssetPlugin {
    fn setup(&self, builder: &mut SceneBuilder) {
        let _ = builder;
    }
}

pub trait AssetExt {
    fn register_asset_loader<A, M>(&mut self)
    where
        A: LoadAsset;
}

#[rustfmt::skip]
impl AssetExt for SceneBuilder {
    fn register_asset_loader<A, M>(&mut self)
    where
        A: LoadAsset,

    {
        self.add_systems(
            schedule::PostUpdate,
            start_loading::<A>.in_set(AssetLoaderSystems::StartLoading),
        )
        .add_systems(
            schedule::PostUpdate,
            poll_loaders::<A::State>
                .in_set(AssetLoaderSystems::PollLoaders)
                .after(AssetLoaderSystems::StartLoading),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, SystemSet)]
pub enum AssetLoaderSystems {
    StartLoading,
    PollLoaders,
}
