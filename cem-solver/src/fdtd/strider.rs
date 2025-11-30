use std::ops::{
    Range,
    RangeBounds,
};

use nalgebra::{
    Point3,
    Vector3,
    Vector4,
};

use crate::fdtd::util::{
    PointIter,
    iter_points,
    normalize_point_bounds,
};

#[derive(Clone, Copy, Debug)]
pub struct Strider {
    strides: Vector4<usize>,
    size: Vector3<usize>,
    //offset: usize,
}

impl Strider {
    pub fn new(size: &Vector3<usize>) -> Self {
        Self {
            strides: strides_for_size(size),
            size: *size,
            //offset: 0,
        }
    }

    pub fn point_unchecked(&self, mut index: usize) -> Point3<usize> {
        let z = index / self.strides.z;
        index %= self.strides.z;
        let y = index / self.strides.y;
        index %= self.strides.y;
        let x = index / self.strides.x;
        Point3::new(x, y, z)
    }

    pub fn point(&self, index: usize) -> Option<Point3<usize>> {
        (index < self.strides.w).then(|| self.point_unchecked(index))
    }

    fn index_unchecked(&self, point: &Point3<usize>) -> usize {
        point.coords.dot(&self.strides.xyz())
    }

    pub fn index(&self, point: &Point3<usize>) -> Option<usize> {
        self.is_inside(point).then(|| self.index_unchecked(point))
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

    pub fn iter(&self, range: impl RangeBounds<Point3<usize>>) -> StriderIter {
        StriderIter {
            points: iter_points(range, self.size),
            strider: *self,
        }
    }

    fn is_inside(&self, point: &Point3<usize>) -> bool {
        point.x < self.size.x && point.y < self.size.y && point.z < self.size.z
    }

    pub fn contiguous_index_range(
        &self,
        range: impl RangeBounds<Point3<usize>>,
    ) -> Result<Range<usize>, Range<usize>> {
        // i think this works lol, but we should write some tests

        let points = normalize_point_bounds(range, self.size);

        let checked_dec = |x: Point3<usize>| {
            Some(Point3::new(
                x.x.checked_sub(1)?,
                x.y.checked_sub(1)?,
                x.z.checked_sub(1)?,
            ))
        };

        let start_index = self.index_unchecked(&points.start);
        let indices = Range {
            start: start_index,
            end: self.index_unchecked(&checked_dec(points.end).ok_or(start_index..start_index)?)
                + 1,
        };

        let num_points = (points.end - points.start).product();
        let num_indices = indices.end - indices.start;

        if num_points == num_indices {
            Ok(indices)
        }
        else {
            Err(indices)
        }
    }

    /*pub fn region(&self, range: impl RangeBounds<Point3<usize>>) -> Self {
        let range = normalize_point_bounds(range, self.size);
        let offset = self.index(&range.start);
        Self {
            strides: self.strides,
            size: range.end - range.start,
            offset,
        }
    }*/
}

#[derive(Clone, Copy, Debug)]
pub struct StriderIter {
    points: PointIter,
    strider: Strider,
}

impl Iterator for StriderIter {
    type Item = (usize, Point3<usize>);

    fn next(&mut self) -> Option<Self::Item> {
        let point = self.points.next()?;
        let index = self.strider.index_unchecked(&point);
        Some((index, point))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.points.size_hint()
    }
}

// the where bound is just so we get a compiler error if PointIter happens to be
// not an ExactSizeIterator anymore.
impl ExactSizeIterator for StriderIter where PointIter: ExactSizeIterator {}

pub fn strides_for_size(size: &Vector3<usize>) -> Vector4<usize> {
    let mut strides = Vector4::zeros();
    strides.x = 1;
    strides.y = strides.x * size.x;
    strides.z = strides.y * size.y;
    strides.w = strides.z * size.z;
    strides
}
