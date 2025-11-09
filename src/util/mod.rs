pub mod arena;

use std::{
    fmt::Display,
    ops::{
        Deref,
        DerefMut,
    },
    path::Path,
    sync::Arc,
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

impl<P> From<FormatPath<P>> for egui::WidgetText
where
    P: AsRef<Path>,
{
    fn from(value: FormatPath<P>) -> Self {
        value.to_string().into()
    }
}
