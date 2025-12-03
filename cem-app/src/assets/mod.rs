mod plugin;
mod systems;

use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    system::SystemParam,
};

use crate::Error;

pub trait LoadAsset: Component {
    type State: LoadingState;

    fn start_loading(&self) -> Result<Self::State, Error>;
}

// note: this is just a future -.-
pub trait LoadingState: Send + Sync + 'static {
    type Output: Bundle;
    type Context: SystemParam + 'static;

    fn poll(
        &mut self,
        context: &mut <Self::Context as SystemParam>::Item<'_, '_>,
    ) -> Result<LoadingProgress<Self::Output>, Error>;
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
