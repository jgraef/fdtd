use bevy_ecs::{
    component::Component,
    entity::Entity,
    system::{
        Commands,
        Query,
        StaticSystemParam,
    },
};

use crate::assets::{
    LoadAsset,
    LoadingProgress,
    LoadingState,
};

pub fn start_loading<L: LoadAsset>(
    query: Query<(Entity, &'static L)>,
    mut loader_context: StaticSystemParam<<<L as LoadAsset>::State as LoadingState>::Context>,
    mut commands: Commands,
) {
    query.iter().for_each(|(entity, loader)| {
        let mut entity = commands.entity(entity);

        // remove first, so if an error occurs during loading, the loader will still be
        // removed.
        entity.remove::<L>();

        match loader.start_loading() {
            Ok(mut loading_state) => {
                // try to load it immediately
                match loading_state.poll(&mut loader_context) {
                    Ok(LoadingProgress::Pending) => {
                        entity.insert(LoadingStateContainer(loading_state));
                    }
                    Ok(LoadingProgress::Ready(loaded)) => {
                        entity.insert(loaded);
                    }
                    Err(error) => {
                        entity.remove::<L>();
                        tracing::error!(%error, "error while loading asset");
                    }
                }
            }
            Err(error) => {
                entity.remove::<L>();
                tracing::error!(%error, "error while loading asset");
            }
        }
    });
}

pub fn poll_loaders<L: LoadingState>(
    mut query: Query<(Entity, &mut LoadingStateContainer<L>)>,
    mut loader_context: StaticSystemParam<L::Context>,
    mut commands: Commands,
) {
    query.iter_mut().for_each(|(entity, mut loading_state)| {
        match loading_state.0.poll(&mut loader_context) {
            Ok(LoadingProgress::Pending) => {}
            Ok(LoadingProgress::Ready(loaded)) => {
                let mut entity = commands.entity(entity);
                entity.remove::<LoadingStateContainer<L>>();
                entity.insert(loaded);
            }
            Err(error) => {
                let mut entity = commands.entity(entity);
                entity.remove::<LoadingStateContainer<L>>();
                tracing::error!(%error, "error while loading asset");
            }
        }
    });
}

/// A simple container we wrap around loading states, so that implementors for
/// LoadAsset can use Self as the state without confusing the loading systems.
#[derive(Debug, Component)]
pub struct LoadingStateContainer<T>(T);
