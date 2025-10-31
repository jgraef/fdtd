use std::ops::{
    Bound,
    RangeBounds,
};

use nalgebra::{
    Matrix3,
    Point3,
    Vector3,
};

use crate::{
    boundary_condition::{
        AnyBoundaryCondition,
        BoundaryCondition,
    },
    lattice::Lattice,
    simulation::Axis,
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

/// See [`partial_derivate`] for details.
pub fn jacobian<T>(
    x: &Point3<usize>,
    dx0: &Vector3<usize>,
    dx1: &Vector3<usize>,
    grid: &Lattice<T>,
    field: impl Fn(&T) -> Vector3<f64>,
    spatial_resolution: &Vector3<f64>,
    boundary_conditions: &[AnyBoundaryCondition; 3],
) -> Jacobian {
    Jacobian {
        matrix: Matrix3::from_columns(&[
            partial_derivative(
                Axis::X,
                x,
                dx0,
                dx1,
                grid,
                &field,
                spatial_resolution,
                boundary_conditions,
            ),
            partial_derivative(
                Axis::Y,
                x,
                dx0,
                dx1,
                grid,
                &field,
                spatial_resolution,
                boundary_conditions,
            ),
            partial_derivative(
                Axis::Z,
                x,
                dx0,
                dx1,
                grid,
                &field,
                spatial_resolution,
                boundary_conditions,
            ),
        ]),
    }
}

pub struct Jacobian {
    pub matrix: Matrix3<f64>,
}

impl Jacobian {
    pub fn curl(&self) -> Vector3<f64> {
        Vector3::new(
            self.matrix.m32 - self.matrix.m23,
            self.matrix.m13 - self.matrix.m31,
            self.matrix.m21 - self.matrix.m12,
        )
    }

    pub fn divgerence(&self) -> f64 {
        self.matrix.trace()
    }
}

/// Calculates a partial derivative at `x` along `axis`.
///
/// `dx0` and `dx1` specify which points around `x` to use for the central
/// difference derivatives. `dx0` will be subtracted from `x`` to get `x1` and
/// `dx1` will be added to `x` to get `x2`. The central difference is then
/// between `x1` and `x2`. This is useful when we want to e.g. calculate the
/// curl of the E-field at point x for the H-field. For the H-field the left
/// point in the E-field for the central difference will be `x-(1, 1, 1)`, and
/// the right point will be `x`, because in our convention the E-field is
/// staggered by `(+0.5, +0.5, +0.5)`. To calculate the curl of the H-field for
/// the E-field you'd pass `(0, 0, 0)` and `(1, 1, 1)` for `dx0` and `dx1`.
///
/// `grid`: The grid in which the E-field and H-field are virtually colocated -
/// meaning they share the same grid cell in the [`Vec`]. For calculations we
/// use the Yee grid with the cell `(0, 0, 0)` having the E-field for
/// `(0.5, 0.5, 0.5)`.
///
/// `field`: Closure to access the field vector from a cell of which to
/// calculate the curl.
///
/// Note: This is technically generic over the type of cells in the lattice,
/// although in practive it will be a [`Cell`] struct.
///
/// # Boundary condition
///
/// To compute the spatial partial derivatives adjacent field values are needed.
/// Since these are not available outside of the lattice, all derivatives along
/// a boundary default to 0. This is effectively a Neumann boundary condition.
pub fn partial_derivative<T>(
    axis: Axis,
    x: &Point3<usize>,
    dx0: &Vector3<usize>,
    dx1: &Vector3<usize>,
    grid: &Lattice<T>,
    field: impl Fn(&T) -> Vector3<f64>,
    spatial_resolution: &Vector3<f64>,
    boundary_conditions: &[AnyBoundaryCondition; 3],
) -> Vector3<f64> {
    let i = axis.vector_index();
    let dx0 = dx0[i];
    let dx1 = dx1[i];
    let e = axis.basis().into_inner();
    let dx = spatial_resolution[i];

    let f0 = if x.coords[i] >= dx0 {
        grid.get(&(x - e * dx0)).map(&field)
    }
    else {
        None
    };
    let f1 = grid.get(&(x + e * dx1)).map(&field);

    // fixme: the boundary conditions should be invariant under dx
    boundary_conditions[i].apply_df(f0, f1) / dx
}
