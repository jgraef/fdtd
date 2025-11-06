use std::{
    fmt::Debug,
    marker::PhantomData,
};

#[derive(Clone, Debug, Default)]
pub struct GenerationalArena<T> {
    slots: Vec<Slot<T>>,
    vacant: Option<usize>,
    num_occupied: usize,
}

impl<T> GenerationalArena<T> {
    pub fn insert(&mut self, value: T) -> Handle<T> {
        if let Some(index) = self.vacant {
            let slot = &mut self.slots[index];
            match slot {
                Slot::Vacant {
                    next_vacant,
                    generation,
                } => {
                    self.vacant = *next_vacant;
                    let generation = *generation;
                    *slot = Slot::Occupied { value, generation };
                    self.num_occupied += 1;
                    Handle::new(index, generation)
                }
                _ => unreachable!("vacant slot in use"),
            }
        }
        else {
            let index = self.slots.len();
            self.slots.push(Slot::Occupied {
                value,
                generation: 0,
            });
            self.num_occupied += 1;
            Handle::new(index, 0)
        }
    }

    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        match self.slots.get(handle.index) {
            Some(Slot::Occupied { value, generation }) => {
                assert!(handle.generation <= *generation);
                (handle.generation == *generation).then_some(value)
            }
            _ => None,
        }
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        match self.slots.get_mut(handle.index) {
            Some(Slot::Occupied { value, generation }) => {
                assert!(handle.generation <= *generation);
                (handle.generation == *generation).then_some(value)
            }
            _ => None,
        }
    }

    pub fn contains(&self, handle: Handle<T>) -> bool {
        match self.slots.get(handle.index) {
            Some(Slot::Occupied { generation, .. }) => {
                assert!(handle.generation <= *generation);
                handle.generation == *generation
            }
            _ => false,
        }
    }

    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            iter: self.slots.iter().enumerate(),
            remaining: self.num_occupied,
        }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            iter: self.slots.iter_mut().enumerate(),
            remaining: self.num_occupied,
        }
    }

    pub fn len(&self) -> usize {
        self.num_occupied
    }

    pub fn is_empty(&self) -> bool {
        self.num_occupied == 0
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.num_occupied = 0;
    }

    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        match self.slots.get_mut(handle.index) {
            Some(Slot::Occupied { generation, .. }) => {
                let generation = *generation;
                assert!(handle.generation <= generation);
                if handle.generation == generation {
                    let old_slot = std::mem::replace(
                        &mut self.slots[handle.index],
                        Slot::Vacant {
                            next_vacant: self.vacant,
                            generation: generation + 1,
                        },
                    );
                    let old_value = match old_slot {
                        Slot::Occupied { value, .. } => value,
                        _ => unreachable!(),
                    };
                    self.vacant = Some(handle.index);
                    Some(old_value)
                }
                else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
enum Slot<T> {
    Occupied {
        value: T,
        generation: usize,
    },
    Vacant {
        next_vacant: Option<usize>,
        generation: usize,
    },
}

pub struct Handle<T> {
    index: usize,
    generation: usize,
    _phantom: PhantomData<fn(&T)>,
}

impl<T> Handle<T> {
    fn new(index: usize, generation: usize) -> Self {
        Self {
            index,
            generation,
            _phantom: PhantomData,
        }
    }

    pub fn erased(&self) -> ErasedHandle {
        ErasedHandle {
            index: self.index,
            generation: self.generation,
        }
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            generation: self.generation.clone(),
            _phantom: self._phantom.clone(),
        }
    }
}

impl<T> Copy for Handle<T> {}

impl<T> Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ErasedHandle {
    index: usize,
    generation: usize,
}

impl ErasedHandle {
    pub fn typed<T>(&self) -> Handle<T> {
        Handle::new(self.index, self.generation)
    }
}

#[derive(Clone, Debug)]
pub struct Iter<'a, T> {
    iter: std::iter::Enumerate<std::slice::Iter<'a, Slot<T>>>,
    remaining: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (Handle<T>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (index, slot) = self.iter.next()?;
            match slot {
                Slot::Occupied { value, generation } => {
                    self.remaining -= 1;
                    return Some((Handle::new(index, *generation), value));
                }
                _ => {}
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a, T> ExactSizeIterator for Iter<'a, T> {}

impl<'a, T> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            let (index, slot) = self.iter.next_back()?;
            match slot {
                Slot::Occupied { value, generation } => {
                    self.remaining -= 1;
                    return Some((Handle::new(index, *generation), value));
                }
                _ => {}
            }
        }
    }
}

#[derive(Debug)]
pub struct IterMut<'a, T> {
    iter: std::iter::Enumerate<std::slice::IterMut<'a, Slot<T>>>,
    remaining: usize,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = (Handle<T>, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (index, slot) = self.iter.next()?;
            match slot {
                Slot::Occupied { value, generation } => {
                    self.remaining -= 1;
                    return Some((Handle::new(index, *generation), value));
                }
                _ => {}
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a, T> ExactSizeIterator for IterMut<'a, T> {}

impl<'a, T> DoubleEndedIterator for IterMut<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            let (index, slot) = self.iter.next_back()?;
            match slot {
                Slot::Occupied { value, generation } => {
                    self.remaining -= 1;
                    return Some((Handle::new(index, *generation), value));
                }
                _ => {}
            }
        }
    }
}
