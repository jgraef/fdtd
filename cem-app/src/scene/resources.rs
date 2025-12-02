use std::{
    any::{
        Any,
        TypeId,
        type_name,
    },
    collections::{
        HashMap,
        hash_map,
    },
    fmt::Debug,
    marker::PhantomData,
};

pub trait Resource: Send + Sync + Any + 'static {}

impl<T> Resource for T where T: Send + Sync + 'static {}

#[derive(Default)]
pub struct Resources {
    resources: HashMap<TypeId, Box<dyn Any + Send + Sync + 'static>>,
}

impl Resources {
    pub fn contains<T>(&self) -> bool
    where
        T: Resource,
    {
        self.resources.contains_key(&TypeId::of::<T>())
    }

    pub fn entry<T>(&mut self) -> Entry<'_, T>
    where
        T: Resource,
    {
        match self.resources.entry(TypeId::of::<T>()) {
            hash_map::Entry::Occupied(entry) => {
                Entry::Occupied(OccupiedEntry {
                    entry,
                    _phantom: PhantomData,
                })
            }
            hash_map::Entry::Vacant(entry) => {
                Entry::Vacant(VacantEntry {
                    entry,
                    _phantom: PhantomData,
                })
            }
        }
    }

    pub fn insert<T>(&mut self, resource: T) -> Option<T>
    where
        T: Resource,
    {
        match self.entry::<T>() {
            Entry::Occupied(mut occupied_entry) => Some(occupied_entry.insert(resource)),
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(resource);
                None
            }
        }
    }

    pub fn get<T>(&self) -> Option<&T>
    where
        T: Resource,
    {
        self.resources.get(&TypeId::of::<T>()).map(|resource| {
            resource
                .downcast_ref::<T>()
                .unwrap_or_else(|| panic_wrong_type::<T>())
        })
    }

    pub fn get_mut<T>(&mut self) -> Option<&mut T>
    where
        T: Resource,
    {
        match self.entry::<T>() {
            Entry::Occupied(occupied_entry) => Some(occupied_entry.into_mut()),
            Entry::Vacant(_vacant_entry) => None,
        }
    }

    pub fn get_mut_or_insert_with<T, F>(&mut self, default: F) -> &mut T
    where
        T: Resource,
        F: FnOnce() -> T,
    {
        self.entry::<T>().or_insert_with(default)
    }

    pub fn get_mut_or_insert<T>(&mut self, default: T) -> &T
    where
        T: Resource,
    {
        self.get_mut_or_insert_with(move || default)
    }

    pub fn get_mut_or_insert_default<T>(&mut self) -> &T
    where
        T: Resource + Default,
    {
        self.get_mut_or_insert_with(Default::default)
    }

    #[track_caller]
    pub fn expect<T>(&self) -> &T
    where
        T: Resource,
    {
        self.resources
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic_missing_resource::<T>())
            .downcast_ref::<T>()
            .unwrap_or_else(|| panic_wrong_type::<T>())
    }

    #[track_caller]
    pub fn expect_mut<T>(&mut self) -> &mut T
    where
        T: Resource,
    {
        match self.entry::<T>() {
            Entry::Occupied(occupied_entry) => occupied_entry.into_mut(),
            Entry::Vacant(_vacant_entry) => panic_missing_resource::<T>(),
        }
    }
}

impl Debug for Resources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resources")
            .field("resources", &self.resources.len())
            .finish()
    }
}

#[derive(Debug)]
pub enum Entry<'a, T>
where
    T: Resource,
{
    Occupied(OccupiedEntry<'a, T>),
    Vacant(VacantEntry<'a, T>),
}

impl<'a, T> Entry<'a, T>
where
    T: Resource,
{
    pub fn or_insert(self, default: T) -> &'a mut T {
        self.or_insert_with(move || default)
    }

    pub fn or_insert_with<F>(self, default: F) -> &'a mut T
    where
        F: FnOnce() -> T,
    {
        match self {
            Entry::Occupied(occupied_entry) => occupied_entry.into_mut(),
            Entry::Vacant(vacant_entry) => vacant_entry.insert(default()),
        }
    }

    pub fn and_modify<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut T),
    {
        match &mut self {
            Entry::Occupied(occupied_entry) => {
                f(occupied_entry.get_mut());
            }
            Entry::Vacant(_vacant_entry) => {}
        }
        self
    }

    pub fn insert_entry(self, resource: T) -> OccupiedEntry<'a, T> {
        match self {
            Entry::Occupied(mut occupied_entry) => {
                let _old_value = occupied_entry.insert(resource);
                occupied_entry
            }
            Entry::Vacant(vacant_entry) => vacant_entry.insert_entry(resource),
        }
    }
}

#[derive(derive_more::Debug)]
pub struct OccupiedEntry<'a, T>
where
    T: Resource,
{
    #[debug(skip)]
    entry: hash_map::OccupiedEntry<'a, TypeId, Box<dyn Any + Send + Sync + 'static>>,
    _phantom: PhantomData<&'a mut T>,
}

impl<'a, T> OccupiedEntry<'a, T>
where
    T: Resource,
{
    pub fn remove(self) -> T {
        let resource = self.entry.remove();
        *resource
            .downcast::<T>()
            .unwrap_or_else(|_| panic_wrong_type::<T>())
    }

    pub fn get(&self) -> &T {
        self.entry
            .get()
            .downcast_ref::<T>()
            .unwrap_or_else(|| panic_wrong_type::<T>())
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.entry
            .get_mut()
            .downcast_mut::<T>()
            .unwrap_or_else(|| panic_wrong_type::<T>())
    }

    pub fn into_mut(self) -> &'a mut T {
        self.entry
            .into_mut()
            .downcast_mut::<T>()
            .unwrap_or_else(|| panic_wrong_type::<T>())
    }

    pub fn insert(&mut self, resource: T) -> T {
        *self
            .entry
            .insert(Box::new(resource))
            .downcast::<T>()
            .unwrap_or_else(|_| panic_wrong_type::<T>())
    }
}

#[derive(derive_more::Debug)]
pub struct VacantEntry<'a, T> {
    #[debug(skip)]
    entry: hash_map::VacantEntry<'a, TypeId, Box<dyn Any + Send + Sync + 'static>>,
    _phantom: PhantomData<&'a mut T>,
}

impl<'a, T> VacantEntry<'a, T>
where
    T: Resource,
{
    pub fn insert(self, resource: T) -> &'a mut T {
        self.insert_entry(resource).into_mut()
    }

    pub fn insert_entry(self, resource: T) -> OccupiedEntry<'a, T> {
        let entry = self.entry.insert_entry(Box::new(resource));
        OccupiedEntry {
            entry,
            _phantom: PhantomData,
        }
    }
}

#[track_caller]
fn panic_wrong_type<T>() -> ! {
    panic!("expected type: {}", type_name::<T>());
}

#[track_caller]
fn panic_missing_resource<T>() -> ! {
    panic!("Missing resource: {}", type_name::<T>());
}

#[cfg(test)]
mod tests {
    use crate::scene::resources::Resources;

    #[test]
    fn resource_is_contained_after_insert() {
        struct A;

        let mut resources = Resources::default();

        resources.insert(A);
        assert!(resources.contains::<A>());
        let _a = resources.get::<A>().unwrap();
        let _a = resources.get_mut::<A>().unwrap();
    }

    #[test]
    fn can_hold_multiple_resources() {
        struct A;
        struct B;

        let mut resources = Resources::default();

        resources.insert(A);
        assert!(resources.contains::<A>());

        let _a = resources.get::<A>().unwrap();
        let _a = resources.get_mut::<A>().unwrap();
        assert!(!resources.contains::<B>());
        assert!(resources.get::<B>().is_none());

        resources.insert(B);
        let _a = resources.get::<A>().unwrap();
        let _a = resources.get_mut::<A>().unwrap();
        let _b = resources.get::<B>().unwrap();
        let _b = resources.get_mut::<B>().unwrap();
    }

    #[test]
    fn get_mut_or_insert_with_doesnt_insert_twice() {
        struct A(u32);

        let mut resources = Resources::default();

        let a = resources.get_mut_or_insert(A(1));
        assert_eq!(a.0, 1);
        assert!(resources.contains::<A>());

        let a = resources.get_mut_or_insert(A(2));
        assert_eq!(a.0, 1);
    }
}
