pub mod egui;
pub mod palette;
pub mod scene;
pub mod serde;

use std::{
    fmt::Display,
    ops::{
        Deref,
        DerefMut,
    },
    path::Path,
    sync::Arc,
    thread::JoinHandle,
};

use directories::UserDirs;

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

#[macro_export]
macro_rules! lipsum {
    ($n:expr) => {{
        static TEXT: ::std::sync::OnceLock<String> = ::std::sync::OnceLock::new();
        TEXT.get_or_init(|| ::lipsum::lipsum($n)).as_str()
    }};
}

/// Format a path for display
pub fn format_path<P>(path: P) -> FormatPath<P>
where
    P: AsRef<Path>,
{
    FormatPath { path }
}

#[derive(Clone, Copy, Debug)]
pub struct FormatPath<P> {
    pub path: P,
}

impl<P> Display for FormatPath<P>
where
    P: AsRef<Path>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = self.path.as_ref();

        if let Some(user_dirs) = UserDirs::new() {
            let home = user_dirs.home_dir();

            if let Ok(relative_path) = path.strip_prefix(home) {
                return write!(f, "~/{}", relative_path.to_string_lossy());
            }
        }

        write!(f, "{}", path.to_string_lossy())
    }
}

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

pub fn spawn_thread<F, R>(name: impl ToString, f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(f)
        .expect("std::thread::spawn failed")
}
