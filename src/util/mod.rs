pub mod arena;

use std::{
    ops::{
        Deref,
        DerefMut,
    },
    sync::Arc,
};

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
        if Arc::get_mut(&mut self.value).is_none() {
            self.value = Arc::new(allocate());
        }

        let value = Arc::get_mut(&mut self.value).unwrap();

        ReusableSharedBufferGuard { value }
    }
}

#[derive(Debug)]
pub struct ReusableSharedBufferGuard<'a, T> {
    value: &'a mut T,
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
