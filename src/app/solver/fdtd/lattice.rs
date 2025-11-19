use std::ops::{
    Index,
    IndexMut,
    RangeBounds,
};

use nalgebra::{
    Point3,
    Vector3,
    Vector4,
};

use crate::util::{
    PointIter,
    iter_points,
};

#[derive(Clone, Debug)]
pub struct Lattice<T> {
    data: Box<[T]>,
}

impl<T> Lattice<T>
where
    T: Default,
{
    pub fn from_default(strider: &Strider) -> Self {
        Self::from_fn(strider, |_, _| Default::default())
    }
}

impl<T> Lattice<T>
where
    T: Clone,
{
    pub fn from_value(strider: &Strider, value: T) -> Self {
        Self::from_fn(strider, |_, _| value.clone())
    }
}

impl<T> Lattice<T> {
    pub fn from_fn(
        strider: &Strider,
        mut init: impl FnMut(usize, Option<Point3<usize>>) -> T,
    ) -> Self {
        let mut data = Box::new_uninit_slice(strider.len());

        // instead of asking the strider to iterate for us, we need to make sure we
        // cover the whole buffer and leave no uninitialized places.
        for (i, v) in data.iter_mut().enumerate() {
            let point = strider.from_index(i);
            v.write(init(i, point));
        }

        //strider.iter(..).for_each(|(index, point)| {
        //    data[index].write(init(&point));
        //});

        let data = unsafe {
            // SAFETY: assuming strider.iter() iterates over all points, this initializes
            // all data todo: write tests for strider.iter()
            data.assume_init()
        };

        Self { data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn get_point(&self, strider: &Strider, point: &Point3<usize>) -> Option<&T> {
        let index = strider.to_index(point)?;
        Some(&self.data[index])
    }

    pub fn get_point_mut(&mut self, strider: &Strider, point: &Point3<usize>) -> Option<&mut T> {
        let index = strider.to_index(point)?;
        Some(&mut self.data[index])
    }

    pub fn get_index(&self, index: usize) -> Option<&T> {
        self.data.get(index)
    }

    pub fn get_index_mut(&mut self, index: usize) -> Option<&mut T> {
        self.data.get_mut(index)
    }

    pub fn iter(
        &self,
        strider: &Strider,
        range: impl RangeBounds<Point3<usize>>,
    ) -> impl Iterator<Item = (usize, Point3<usize>, &T)> {
        strider
            .iter(range)
            .map(|(index, point)| (index, point, &self.data[index]))
    }

    pub fn iter_mut(
        &mut self,
        strider: &Strider,
        range: impl RangeBounds<Point3<usize>>,
    ) -> impl Iterator<Item = (usize, Point3<usize>, &mut T)> {
        strider.iter(range).map(|(index, point)| {
            let data = unsafe {
                // SAFETY: No mutable borrow to data at the same index is handed out twice
                // (assuming strider.iter() works as expected) This is basically
                // what iter_mut does.
                &mut *(&mut self.data[index] as *mut T)
            };
            (index, point, data)
        })
    }
}

impl<T> Index<usize> for Lattice<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}

impl<T> IndexMut<usize> for Lattice<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Strider {
    strides: Vector4<usize>,
    size: Vector3<usize>,
}

impl Strider {
    pub fn new(size: &Vector3<usize>) -> Self {
        Self {
            strides: strides_for_size(size),
            size: *size,
        }
    }

    pub fn from_index(&self, mut index: usize) -> Option<Point3<usize>> {
        (index < self.strides.w).then(|| {
            let z = index / self.strides.z;
            index %= self.strides.z;
            let y = index / self.strides.y;
            index %= self.strides.y;
            let x = index / self.strides.x;
            Point3::new(x, y, z)
        })
    }

    fn to_index_unchecked(&self, point: &Point3<usize>) -> usize {
        point.coords.dot(&self.strides.xyz())
    }

    pub fn to_index(&self, point: &Point3<usize>) -> Option<usize> {
        self.is_inside(point)
            .then(|| self.to_index_unchecked(point))
    }

    pub fn strides(&self) -> &Vector4<usize> {
        &self.strides
    }

    pub fn size(&self) -> &Vector3<usize> {
        &self.size
    }

    pub fn len(&self) -> usize {
        self.strides.w
    }

    pub fn iter(&self, range: impl RangeBounds<Point3<usize>>) -> StriderPointIter {
        StriderPointIter {
            points: iter_points(range, self.size),
            strider: *self,
        }
    }

    fn is_inside(&self, point: &Point3<usize>) -> bool {
        point.x < self.size.x && point.y < self.size.y && point.z < self.size.z
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StriderPointIter {
    points: PointIter,
    strider: Strider,
}

impl Iterator for StriderPointIter {
    type Item = (usize, Point3<usize>);

    fn next(&mut self) -> Option<Self::Item> {
        let point = self.points.next()?;
        let index = self.strider.to_index_unchecked(&point);
        Some((index, point))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.points.size_hint()
    }
}

// the where bound is just so we get a compiler error if PointIter happens to be
// not an ExactSizeIterator anymore.
impl ExactSizeIterator for StriderPointIter where PointIter: ExactSizeIterator {}

pub fn strides_for_size(size: &Vector3<usize>) -> Vector4<usize> {
    let mut strides = Vector4::zeros();
    strides.x = 1;
    strides.y = strides.x * size.x;
    strides.z = strides.y * size.y;
    strides.w = strides.z * size.z;
    strides
}
