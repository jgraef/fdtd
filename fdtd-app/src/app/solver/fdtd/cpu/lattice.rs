use std::ops::{
    Index,
    IndexMut,
    RangeBounds,
};

use nalgebra::Point3;

use crate::app::solver::fdtd::strider::{
    Strider,
    StriderIter,
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
            let point = strider.point(i);
            v.write(init(i, point));
        }

        let data = unsafe {
            // SAFETY: we interated over the whole data slice and wrote a value at every
            // index
            data.assume_init()
        };

        Self { data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn get_point(&self, strider: &Strider, point: &Point3<usize>) -> Option<&T> {
        let index = strider.index(point)?;
        Some(&self.data[index])
    }

    pub fn get_point_mut(&mut self, strider: &Strider, point: &Point3<usize>) -> Option<&mut T> {
        let index = strider.index(point)?;
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
    ) -> LatticeIter<'_, T> {
        LatticeIter {
            strider_iter: strider.iter(range),
            data: &self.data,
        }
    }

    pub fn iter_mut(
        &mut self,
        strider: &Strider,
        range: impl RangeBounds<Point3<usize>>,
    ) -> LatticeIterMut<'_, T> {
        LatticeIterMut {
            strider_iter: strider.iter(range),
            data: &mut self.data,
        }
    }

    // todo: range
    #[cfg(feature = "rayon")]
    pub fn par_iter_mut(
        &mut self,
        strider: &Strider,
    ) -> impl rayon::iter::ParallelIterator<Item = (usize, Point3<usize>, &mut T)>
    where
        T: Send + Sync,
    {
        use rayon::iter::{
            IndexedParallelIterator as _,
            IntoParallelRefMutIterator as _,
            ParallelIterator as _,
        };

        self.data.par_iter_mut().enumerate().map(|(index, value)| {
            let point = strider.point_unchecked(index);
            (index, point, value)
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
pub struct LatticeIter<'a, T> {
    strider_iter: StriderIter,
    data: &'a [T],
}

impl<'a, T> Iterator for LatticeIter<'a, T> {
    type Item = (usize, Point3<usize>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let (index, point) = self.strider_iter.next()?;
        Some((index, point, &self.data[index]))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.strider_iter.size_hint()
    }
}

impl<'a, T> ExactSizeIterator for LatticeIter<'a, T> where StriderIter: ExactSizeIterator {}

#[derive(Debug)]
pub struct LatticeIterMut<'a, T> {
    strider_iter: StriderIter,
    data: &'a mut [T],
}

impl<'a, T> Iterator for LatticeIterMut<'a, T> {
    type Item = (usize, Point3<usize>, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        let (index, point) = self.strider_iter.next()?;
        let data = unsafe {
            // SAFETY: No mutable borrow to data at the same index is handed out twice
            // (assuming strider.iter() works as expected) This is basically
            // what iter_mut does.
            &mut *(&mut self.data[index] as *mut T)
        };
        Some((index, point, data))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.strider_iter.size_hint()
    }
}

impl<'a, T> ExactSizeIterator for LatticeIterMut<'a, T> where StriderIter: ExactSizeIterator {}
