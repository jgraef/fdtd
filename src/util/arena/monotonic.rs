use std::{
    fmt::Debug,
    marker::PhantomData,
};

#[derive(Clone, Debug, Default)]
pub struct MonotonicArena<T> {
    slots: Vec<T>,
}

impl<T> MonotonicArena<T> {
    pub fn insert(&mut self, value: T) -> Handle<T> {
        let index = self.slots.len();
        self.slots.push(value);
        Handle::new(index)
    }

    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.slots.get(handle.index)
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.slots.get_mut(handle.index)
    }

    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            iter: self.slots.iter().enumerate(),
        }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            iter: self.slots.iter_mut().enumerate(),
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn clear(&mut self) {
        self.slots.clear();
    }
}

pub struct Handle<T> {
    index: usize,
    _phantom: PhantomData<fn(&T)>,
}

impl<T> Handle<T> {
    fn new(index: usize) -> Self {
        Self {
            index,
            _phantom: PhantomData,
        }
    }

    pub fn erased(&self) -> ErasedHandle {
        ErasedHandle { index: self.index }
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            _phantom: self._phantom.clone(),
        }
    }
}

impl<T> Copy for Handle<T> {}

impl<T> Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("index", &self.index)
            .finish()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ErasedHandle {
    index: usize,
}

impl ErasedHandle {
    pub fn typed<T>(&self) -> Handle<T> {
        Handle::new(self.index)
    }
}

#[derive(Clone, Debug)]
pub struct Iter<'a, T> {
    iter: std::iter::Enumerate<std::slice::Iter<'a, T>>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (Handle<T>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let (index, item) = self.iter.next()?;
        Some((Handle::new(index), item))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T> ExactSizeIterator for Iter<'a, T> {}

impl<'a, T> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let (index, item) = self.iter.next_back()?;
        Some((Handle::new(index), item))
    }
}

#[derive(Debug)]
pub struct IterMut<'a, T> {
    iter: std::iter::Enumerate<std::slice::IterMut<'a, T>>,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = (Handle<T>, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        let (index, item) = self.iter.next()?;
        Some((Handle::new(index), item))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T> ExactSizeIterator for IterMut<'a, T> {}

impl<'a, T> DoubleEndedIterator for IterMut<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let (index, item) = self.iter.next_back()?;
        Some((Handle::new(index), item))
    }
}
