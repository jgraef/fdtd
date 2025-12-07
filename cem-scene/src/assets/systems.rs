use bevy_ecs::{
    entity::Entity,
    system::{
        Commands,
        Query,
        StaticSystemParam,
    },
};

use crate::assets::LoadAsset;

pub fn start_loading<L: LoadAsset>(
    query: Query<(Entity, &'static L)>,
    mut loader_context: StaticSystemParam<<L as LoadAsset>::Context>,
    mut commands: Commands,
) {
    query.iter().for_each(|(entity, loader)| {
        let mut entity = commands.entity(entity);
        // remove first, so if an error occurs during loading, the loader will still be
        // removed.
        entity.remove::<L>();

        if let Err(error) = loader.load(entity, &mut *loader_context) {
            tracing::error!(%error, "error while loading asset");
        }
    });
}
