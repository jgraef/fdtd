mod plugin;
mod systems;

use bevy_ecs::{
    component::Component,
    system::{
        EntityCommands,
        SystemParam,
    },
};
pub use plugin::{
    AssetExt,
    AssetLoaderSystems,
    AssetPlugin,
};

pub trait LoadAsset: Component {
    type Context: SystemParam + 'static;
    type Error: std::error::Error;

    fn load(
        &self,
        entity: EntityCommands,
        context: &mut <Self::Context as SystemParam>::Item<'_, '_>,
    ) -> Result<(), Self::Error>;
}
