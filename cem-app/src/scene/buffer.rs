use std::collections::HashMap;

#[derive(derive_more::Debug, Default)]
pub struct SceneBuffer {
    #[debug(skip)]
    buffer: HashMap<hecs::Entity, hecs::EntityBuilder>,
}

impl SceneBuffer {
    pub fn insert<B>(&mut self, source: hecs::Entity, bundle: B)
    where
        B: hecs::DynamicBundle,
    {
        let builder = self.buffer.entry(source).or_default();
        builder.add_bundle(bundle);
    }

    pub fn restore(mut self, world: &mut hecs::World) {
        for (entity, mut builder) in self.buffer.drain() {
            let _ = world.insert(entity, builder.build());
        }
    }
}
