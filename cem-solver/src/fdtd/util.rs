use std::ops::{
    Bound,
    Index,
    IndexMut,
    Range,
    RangeBounds,
};

use nalgebra::{
    Point3,
    Vector3,
};

use crate::{
    fdtd::Resolution,
    material::{
        Material,
        PhysicalConstants,
    },
};

/// Buffer holding 2 values.
///
/// One value is the current value, the other one is the value from the previous
/// step. Which one is which depends on the [`SwapBufferIndex`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SwapBuffer<T> {
    buffer: [T; 2],
}

impl<T> From<[T; 2]> for SwapBuffer<T> {
    fn from(value: [T; 2]) -> Self {
        Self { buffer: value }
    }
}

impl<T> SwapBuffer<T> {
    pub fn from_fn(mut f: impl FnMut(SwapBufferIndex) -> T) -> Self {
        Self::from(std::array::from_fn::<T, 2, _>(|index| {
            f(SwapBufferIndex { index })
        }))
    }

    pub fn pair_mut(&mut self, index: SwapBufferIndex) -> (&mut T, &mut T) {
        let (first, rest) = self.buffer.split_first_mut().unwrap();
        let second = &mut rest[0];
        if index.index == 0 {
            (first, second)
        }
        else {
            (second, first)
        }
    }
}

impl<T> Index<SwapBufferIndex> for SwapBuffer<T> {
    type Output = T;

    fn index(&self, index: SwapBufferIndex) -> &Self::Output {
        &self.buffer[index.index]
    }
}

impl<T> IndexMut<SwapBufferIndex> for SwapBuffer<T> {
    fn index_mut(&mut self, index: SwapBufferIndex) -> &mut Self::Output {
        &mut self.buffer[index.index]
    }
}

/// Index into a [`SwapBuffer`].
///
/// This can be derived from the simulation tick.
#[derive(Clone, Copy, Debug)]
pub struct SwapBufferIndex {
    index: usize,
}

impl SwapBufferIndex {
    pub fn from_tick(tick: usize) -> Self {
        Self { index: tick % 2 }
    }

    pub fn other(&self) -> Self {
        Self {
            index: (self.index + 1) % 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateCoefficients {
    pub c_a: f64,
    pub c_b: f64,
    pub d_a: f64,
    pub d_b: f64,
}

impl UpdateCoefficients {
    pub fn new(
        resolution: &Resolution,
        physical_constants: &PhysicalConstants,
        material: &Material,
    ) -> Self {
        let c_or_d = |perm, sigma| {
            let half_sigmal_delta_t_over_perm = 0.5 * sigma * resolution.temporal / perm;

            let a: f64 =
                (1.0 - half_sigmal_delta_t_over_perm) / (1.0 + half_sigmal_delta_t_over_perm);
            let b: f64 = resolution.temporal / (perm * (1.0 + half_sigmal_delta_t_over_perm));

            assert!(!a.is_nan());
            assert!(!b.is_nan());

            (a, b)
        };

        let (c_a, c_b) = c_or_d(
            material.relative_permittivity * physical_constants.vacuum_permittivity,
            material.eletrical_conductivity,
        );
        let (d_a, d_b) = c_or_d(
            material.relative_permeability * physical_constants.vacuum_permeability,
            material.magnetic_conductivity,
        );

        Self { c_a, c_b, d_a, d_b }
    }
}

pub fn iter_points(range: impl RangeBounds<Point3<usize>>, size: Vector3<usize>) -> PointIter {
    let Range { start, end } = normalize_point_bounds(range, size);

    PointIter {
        x0: start.coords,
        x1: end.coords,
        x: (start != end).then_some(start.coords),
    }
}

pub fn normalize_point_bounds(
    range: impl RangeBounds<Point3<usize>>,
    size: Vector3<usize>,
) -> Range<Point3<usize>> {
    let start = match range.start_bound() {
        Bound::Included(start) => *start,
        Bound::Excluded(start) => start + Vector3::repeat(1),
        Bound::Unbounded => Point3::origin(),
    };

    let end = match range.end_bound() {
        Bound::Included(end) => end + Vector3::repeat(1),
        Bound::Excluded(end) => *end,
        Bound::Unbounded => size.into(),
    };

    let end = start
        .coords
        .zip_map(&end.coords, |x0, x1| x0.max(x1))
        .into();

    Range { start, end }
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

#[cfg(test)]
mod tests {
    use nalgebra::Point3;

    use crate::fdtd::util::iter_points;

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
