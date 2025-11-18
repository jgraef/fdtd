pub mod arena;
pub mod serde;
pub mod wgpu;

use std::{
    fmt::Display,
    ops::{
        Bound,
        Deref,
        DerefMut,
        RangeBounds,
    },
    path::Path,
    sync::Arc,
};

use directories::UserDirs;
use nalgebra::{
    Point3,
    Vector3,
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

impl<P> From<FormatPath<P>> for egui::WidgetText
where
    P: AsRef<Path>,
{
    fn from(value: FormatPath<P>) -> Self {
        value.to_string().into()
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
            Boo::Borrowed(value) => &**value,
            Boo::Owned(value) => &value,
        }
    }
}

impl<'a, T> Deref for Boo<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Boo::Borrowed(value) => &**value,
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
            Moo::Mut(value) => &**value,
            Moo::Owned(value) => &value,
        }
    }
}

impl<'a, T> AsMut<T> for Moo<'a, T> {
    fn as_mut(&mut self) -> &mut T {
        match self {
            Moo::Mut(value) => *value,
            Moo::Owned(value) => value,
        }
    }
}

impl<'a, T> Deref for Moo<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Moo::Mut(value) => &**value,
            Moo::Owned(value) => value,
        }
    }
}

impl<'a, T> DerefMut for Moo<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Moo::Mut(value) => *value,
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

pub fn format_size<T>(value: T) -> humansize::SizeFormatter<T, humansize::FormatSizeOptions>
where
    T: humansize::ToF64 + humansize::Unsigned,
{
    humansize::SizeFormatter::new(value, humansize::BINARY)
}

pub fn iter_points(range: impl RangeBounds<Point3<usize>>, size: Vector3<usize>) -> PointIter {
    let x0 = match range.start_bound() {
        Bound::Included(x0) => x0.coords,
        Bound::Excluded(x0) => x0.coords + Vector3::repeat(1),
        Bound::Unbounded => Vector3::zeros(),
    };

    let x1 = match range.end_bound() {
        Bound::Included(x1) => x1.coords + Vector3::repeat(1),
        Bound::Excluded(x1) => x1.coords,
        Bound::Unbounded => size,
    };

    let x1 = x0.zip_map(&x1, |x0, x1| x0.max(x1));

    PointIter {
        x0,
        x1,
        x: (x0 != x1).then_some(x0),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PointIter {
    x0: Vector3<usize>,
    x1: Vector3<usize>,
    x: Option<Vector3<usize>>,
}

impl Iterator for PointIter {
    type Item = Point3<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = |mut x_n: Vector3<usize>| {
            x_n.x += 1;
            if x_n.x >= self.x1.x {
                x_n.x = self.x0.x;
                x_n.y += 1;
                if x_n.y >= self.x1.y {
                    x_n.y = self.x0.y;
                    x_n.z += 1;
                    if x_n.z >= self.x1.z {
                        return None;
                    }
                }
            }
            Some(x_n)
        };

        if let Some(x) = self.x {
            self.x = next(x);
            Some(Point3::from(x))
        }
        else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let dx = self.x1 - self.x0;
        let n = (self.x1.z - self.x0.z) * dx.y * dx.x
            + (self.x1.y - self.x0.y) * dx.x
            + (self.x1.x - self.x0.x);
        (n, Some(n))
    }
}

impl ExactSizeIterator for PointIter {}

pub fn round_to_grid(
    x: &Point3<f64>,
    origin: &Point3<f64>,
    spatial_resolution: &Vector3<f64>,
) -> Result<Point3<usize>, InvalidPoint> {
    let x = (x - origin).component_div(spatial_resolution);
    x.iter()
        .all(|c| *c >= 0.0)
        .then(|| x.map(|c| c.round() as usize).into())
        .ok_or_else(|| InvalidPoint { point: x.into() })
}

#[derive(Clone, Copy, Debug)]
pub struct InvalidPoint {
    pub point: Point3<f64>,
}

#[cfg(test)]
mod tests {
    use nalgebra::Point3;

    use crate::util::iter_points;

    #[test]
    fn it_iters_inclusive() {
        let x0 = Point3::new(1, 2, 3);
        let x1 = Point3::new(2, 3, 4);
        let points = iter_points(x0..=x1, x1.coords).collect::<Vec<_>>();
        assert_eq!(
            points,
            vec![
                Point3::new(1, 2, 3),
                Point3::new(2, 2, 3),
                Point3::new(1, 3, 3),
                Point3::new(2, 3, 3),
                Point3::new(1, 2, 4),
                Point3::new(2, 2, 4),
                Point3::new(1, 3, 4),
                Point3::new(2, 3, 4),
            ]
        );
    }

    #[test]
    fn it_iters_exclusive() {
        let x0 = Point3::new(1, 2, 3);
        let x1 = Point3::new(3, 4, 5);

        let points = iter_points(x0..x1, x1.coords).collect::<Vec<_>>();
        assert_eq!(
            points,
            vec![
                Point3::new(1, 2, 3),
                Point3::new(2, 2, 3),
                Point3::new(1, 3, 3),
                Point3::new(2, 3, 3),
                Point3::new(1, 2, 4),
                Point3::new(2, 2, 4),
                Point3::new(1, 3, 4),
                Point3::new(2, 3, 4),
            ]
        );
    }
}
