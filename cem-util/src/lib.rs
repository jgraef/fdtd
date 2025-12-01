#![warn(clippy::todo, unused_qualifications)]

#[cfg(feature = "wgpu")]
pub mod wgpu;

#[cfg(feature = "image")]
pub mod image;

pub mod oneshot;

use std::{
    collections::{
        HashMap,
        hash_map,
    },
    hash::Hash,
    ops::{
        Bound,
        Range,
        RangeBounds,
    },
    sync::{
        Arc,
        Weak,
    },
};

pub fn format_size<T>(value: T) -> humansize::SizeFormatter<T, humansize::FormatSizeOptions>
where
    T: humansize::ToF64 + humansize::Unsigned,
{
    humansize::SizeFormatter::new(value, humansize::BINARY)
}

pub fn normalize_index_bounds(range: impl RangeBounds<usize>, len: usize) -> Range<usize> {
    let start = match range.start_bound() {
        Bound::Included(start) => *start,
        Bound::Excluded(start) => start + 1,
        Bound::Unbounded => 0,
    };

    let end = match range.end_bound() {
        Bound::Included(end) => end + 1,
        Bound::Excluded(end) => *end,
        Bound::Unbounded => len,
    };

    let end = end.max(start);

    Range { start, end }
}

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
            hash_map::Entry::Occupied(mut occupied_entry) => {
                if let Some(value) = occupied_entry.get().upgrade() {
                    value
                }
                else {
                    let value = init();
                    occupied_entry.insert(Arc::downgrade(&value));
                    value
                }
            }
            hash_map::Entry::Vacant(vacant_entry) => {
                let value = init();
                vacant_entry.insert(Arc::downgrade(&value));
                value
            }
        }
    }
}
