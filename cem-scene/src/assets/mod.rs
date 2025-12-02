mod plugin;
mod systems;

use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    system::SystemParam,
};
pub use plugin::{
    AssetExt,
    AssetLoaderSystems,
    AssetPlugin,
};

pub trait LoadAsset: Component {
    type State: LoadingState;

    fn start_loading(&self) -> Result<Self::State, AssetError>;
}

// note: this is just a future -.-
pub trait LoadingState: Send + Sync + 'static {
    type Output: Bundle;
    type Context: SystemParam + 'static;

    fn poll(
        &mut self,
        context: &mut <Self::Context as SystemParam>::Item<'_, '_>,
    ) -> Result<LoadingProgress<Self::Output>, AssetError>;
}

pub enum LoadingProgress<T> {
    Pending,
    Ready(T),
}

impl<T> From<Option<T>> for LoadingProgress<T> {
    fn from(value: Option<T>) -> Self {
        value.map_or(LoadingProgress::Pending, LoadingProgress::Ready)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("{0}")]
    Custom(#[source] Box<dyn std::error::Error>),
}

impl AssetError {
    pub fn custom(error: impl Into<Box<dyn std::error::Error>>) -> Self {
        Self::Custom(error.into())
    }
}
