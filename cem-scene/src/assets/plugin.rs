use bevy_ecs::schedule::{
    IntoScheduleConfigs,
    SystemSet,
};

use crate::{
    SceneBuilder,
    assets::{
        LoadAsset,
        systems::start_loading,
    },
    plugin::Plugin,
    schedule,
};

#[derive(Clone, Copy, Debug)]
pub struct AssetPlugin;

impl Plugin for AssetPlugin {
    fn setup(&self, builder: &mut SceneBuilder) {
        // note: this doesn't do anything but we'll keep it since we will probably add
        // stuff (e.g. resources) at some point
        let _ = builder;
    }
}

pub trait AssetExt {
    fn register_asset_loader<A>(&mut self) -> &mut Self
    where
        A: LoadAsset;
}

impl AssetExt for SceneBuilder {
    fn register_asset_loader<A>(&mut self) -> &mut Self
    where
        A: LoadAsset,
    {
        self.add_systems(
            schedule::PostStartup,
            start_loading::<A>.in_set(AssetLoaderSystems::StartLoading),
        )
        .add_systems(
            schedule::PostUpdate,
            start_loading::<A>.in_set(AssetLoaderSystems::StartLoading),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, SystemSet)]
pub enum AssetLoaderSystems {
    StartLoading,
}
