use std::ops::{
    Deref,
    DerefMut,
};

#[derive(Clone, Copy, Debug)]
pub enum Boo<'a, T> {
    Borrowed(&'a T),
    Owned(T),
}

impl<'a, T> AsRef<T> for Boo<'a, T> {
    fn as_ref(&self) -> &T {
        match self {
            Boo::Borrowed(value) => value,
            Boo::Owned(value) => value,
        }
    }
}

impl<'a, T> Deref for Boo<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Boo::Borrowed(value) => value,
            Boo::Owned(value) => value,
        }
    }
}

impl<'a, T> From<&'a T> for Boo<'a, T> {
    fn from(value: &'a T) -> Self {
        Self::Borrowed(value)
    }
}

impl<'a, T> From<T> for Boo<'a, T> {
    fn from(value: T) -> Self {
        Self::Owned(value)
    }
}

impl<'a, T> Default for Boo<'a, T>
where
    T: Default,
{
    fn default() -> Self {
        Self::Owned(Default::default())
    }
}

#[derive(Debug)]
pub enum Moo<'a, T> {
    Mut(&'a mut T),
    Owned(T),
}

impl<'a, T> AsRef<T> for Moo<'a, T> {
    fn as_ref(&self) -> &T {
        match self {
            Moo::Mut(value) => value,
            Moo::Owned(value) => value,
        }
    }
}

impl<'a, T> AsMut<T> for Moo<'a, T> {
    fn as_mut(&mut self) -> &mut T {
        match self {
            Moo::Mut(value) => value,
            Moo::Owned(value) => value,
        }
    }
}

impl<'a, T> Deref for Moo<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Moo::Mut(value) => value,
            Moo::Owned(value) => value,
        }
    }
}

impl<'a, T> DerefMut for Moo<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Moo::Mut(value) => value,
            Moo::Owned(value) => value,
        }
    }
}

impl<'a, T> From<&'a mut T> for Moo<'a, T> {
    fn from(value: &'a mut T) -> Self {
        Self::Mut(value)
    }
}

impl<'a, T> From<T> for Moo<'a, T> {
    fn from(value: T) -> Self {
        Self::Owned(value)
    }
}

impl<'a, T> Default for Moo<'a, T>
where
    T: Default,
{
    fn default() -> Self {
        Self::Owned(Default::default())
    }
}
