use std::{
    any::{
        TypeId,
        type_name,
    },
    collections::{
        HashMap,
        hash_map,
    },
    fmt::Debug,
    sync::Arc,
};

use bevy_ecs::resource::Resource;

use crate::SceneBuilder;

pub trait Plugin: Debug + Send + Sync + 'static {
    fn name(&self) -> &'static str {
        type_name::<Self>()
    }

    fn setup(&self, builder: &mut SceneBuilder);
}

#[derive(Clone, Debug, Default, Resource)]
pub struct PluginRegistry {
    registered_plugins: HashMap<TypeId, Arc<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn register<P>(&mut self, plugin: P) -> Option<Arc<dyn Plugin>>
    where
        P: Plugin,
    {
        match self.registered_plugins.entry(TypeId::of::<P>()) {
            hash_map::Entry::Occupied(_occupied_entry) => None,
            hash_map::Entry::Vacant(vacant_entry) => {
                let plugin = Arc::new(plugin);
                vacant_entry.insert(plugin.clone());
                Some(plugin)
            }
        }
    }

    pub fn register_all(&mut self, source: &Self) -> impl Iterator<Item = Arc<dyn Plugin>> {
        source
            .registered_plugins
            .iter()
            .filter_map(move |(type_id, plugin)| {
                match self.registered_plugins.entry(*type_id) {
                    hash_map::Entry::Occupied(_occupied_entry) => None,
                    hash_map::Entry::Vacant(vacant_entry) => {
                        Some(vacant_entry.insert(plugin.clone()).clone())
                    }
                }
            })
    }
}
