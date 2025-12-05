use std::{
    collections::{
        HashMap,
        hash_map::Entry,
    },
    hash::Hash,
    sync::{
        Arc,
        Weak,
    },
};

#[derive(Clone, Debug)]
pub struct WeakCache<K, V> {
    hash_map: HashMap<K, Weak<V>>,
}

impl<K, V> Default for WeakCache<K, V> {
    fn default() -> Self {
        Self {
            hash_map: HashMap::new(),
        }
    }
}

impl<K, V> WeakCache<K, V>
where
    K: Eq + Hash,
{
    pub fn get_or_insert_with(&mut self, key: K, init: impl FnOnce() -> Arc<V>) -> Arc<V> {
        match self.hash_map.entry(key) {
            Entry::Occupied(mut occupied_entry) => {
                if let Some(value) = occupied_entry.get().upgrade() {
                    value
                }
                else {
                    let value = init();
                    occupied_entry.insert(Arc::downgrade(&value));
                    value
                }
            }
            Entry::Vacant(vacant_entry) => {
                let value = init();
                vacant_entry.insert(Arc::downgrade(&value));
                value
            }
        }
    }
}
