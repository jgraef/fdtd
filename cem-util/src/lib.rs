#![warn(clippy::todo, unused_qualifications)]

#[cfg(feature = "egui")]
pub mod egui;

#[cfg(feature = "image")]
pub mod image;

#[cfg(feature = "palette")]
pub mod palette;

#[cfg(feature = "wgpu")]
pub mod wgpu;

#[cfg(feature = "serde")]
pub mod serde;

pub mod boo;
pub mod cache;
pub mod exclusive;
pub mod io;
pub mod oneshot;
pub mod path;

use std::{
    ops::{
        Bound,
        Deref,
        DerefMut,
        Range,
        RangeBounds,
    },
    sync::Arc,
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

#[derive(Debug, Default)]
pub struct ReusableSharedBuffer<T> {
    value: Arc<T>,
}

impl<T> ReusableSharedBuffer<T> {
    pub fn new(value: T) -> Self {
        Self {
            value: Arc::new(value),
        }
    }

    pub fn get(&self) -> Arc<T> {
        self.value.clone()
    }

    pub fn write(&mut self, allocate: impl FnOnce() -> T) -> ReusableSharedBufferGuard<'_, T> {
        let mut reallocated = false;
        if Arc::get_mut(&mut self.value).is_none() {
            self.value = Arc::new(allocate());
            reallocated = true;
        }

        let value = Arc::get_mut(&mut self.value).unwrap();

        ReusableSharedBufferGuard { value, reallocated }
    }
}

#[derive(Debug)]
pub struct ReusableSharedBufferGuard<'a, T> {
    value: &'a mut T,
    reallocated: bool,
}

impl<'a, T> ReusableSharedBufferGuard<'a, T> {
    pub fn reallocated(&self) -> bool {
        self.reallocated
    }
}

impl<'a, T> Deref for ReusableSharedBufferGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &*self.value
    }
}

impl<'a, T> DerefMut for ReusableSharedBufferGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}
