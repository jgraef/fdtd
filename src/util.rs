use std::ops::{
    Bound,
    RangeBounds,
};

use nalgebra::{
    Point3,
    Vector3,
};

pub fn iter_points(
    range: impl RangeBounds<Point3<usize>>,
    dimensions: Vector3<usize>,
) -> PointIter {
    let x0 = match range.start_bound() {
        Bound::Included(x0) => x0.coords,
        Bound::Excluded(x0) => x0.coords + Vector3::repeat(1),
        Bound::Unbounded => Vector3::zeros(),
    };

    let x1 = match range.end_bound() {
        Bound::Included(x1) => x1.coords + Vector3::repeat(1),
        Bound::Excluded(x1) => x1.coords,
        Bound::Unbounded => dimensions,
    };

    let x1 = x0.zip_map(&x1, |x0, x1| x0.max(x1));

    PointIter {
        x0,
        x1,
        x: Some(x0),
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
        let x1 = Point3::new(2, 3, 4);

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
            ]
        );
    }
}

pub fn round_to_grid(
    x: &Point3<f64>,
    origin: &Vector3<f64>,
    spatial_resolution: &Vector3<f64>,
) -> Point3<usize> {
    (x.coords + origin)
        .component_div(spatial_resolution)
        .map(|c| c.round() as usize)
        .into()
}
