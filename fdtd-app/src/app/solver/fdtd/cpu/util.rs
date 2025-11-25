use nalgebra::{
    Matrix3,
    Point3,
    Scalar,
    UnitVector3,
    Vector3,
};
use num::{
    One,
    Zero,
};

use crate::app::solver::fdtd::{
    boundary_condition::{
        AnyBoundaryCondition,
        BoundaryCondition,
    },
    cpu::lattice::Lattice,
    strider::Strider,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn vector_index(&self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    pub fn vector_component<'a, T>(&self, vector: &'a Vector3<T>) -> &'a T {
        &vector[self.vector_index()]
    }

    pub fn vector_component_mut<'a, T>(&self, vector: &'a mut Vector3<T>) -> &'a mut T {
        &mut vector[self.vector_index()]
    }

    pub fn basis<T>(&self) -> UnitVector3<T>
    where
        T: Scalar + Zero + One,
    {
        let mut e = Vector3::<T>::zeros();
        *self.vector_component_mut(&mut e) = T::one();
        // note: one component is 1, all others are 0, therefore this vector is
        // normalized
        UnitVector3::new_unchecked(e)
    }

    pub fn from_vector<T>(vector: &Vector3<T>) -> Option<Self>
    where
        T: Scalar + Zero,
    {
        let z = vector.map(|x| !x.is_zero());
        match (z.x, z.y, z.z) {
            (true, false, false) => Some(Self::X),
            (false, true, false) => Some(Self::Y),
            (false, false, true) => Some(Self::Z),
            _ => None,
        }
    }
}

/// See [`partial_derivate`] for details.
#[allow(clippy::too_many_arguments)]
pub(super) fn jacobian(
    x: &Point3<usize>,
    dx0: &Vector3<usize>,
    dx1: &Vector3<usize>,
    strider: &Strider,
    lattice: &Lattice<Vector3<f64>>,
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
                strider,
                lattice,
                spatial_resolution,
                boundary_conditions,
            ),
            partial_derivative(
                Axis::Y,
                x,
                dx0,
                dx1,
                strider,
                lattice,
                spatial_resolution,
                boundary_conditions,
            ),
            partial_derivative(
                Axis::Z,
                x,
                dx0,
                dx1,
                strider,
                lattice,
                spatial_resolution,
                boundary_conditions,
            ),
        ]),
    }
}

// we might use this in other places, so we could move it to crate::util
pub(super) struct Jacobian {
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
#[allow(clippy::too_many_arguments)]
fn partial_derivative(
    axis: Axis,
    x: &Point3<usize>,
    dx0: &Vector3<usize>,
    dx1: &Vector3<usize>,
    strider: &Strider,
    lattice: &Lattice<Vector3<f64>>,
    spatial_resolution: &Vector3<f64>,
    boundary_conditions: &[AnyBoundaryCondition; 3],
) -> Vector3<f64> {
    let i = axis.vector_index();
    let dx0 = dx0[i];
    let dx1 = dx1[i];
    let e = axis.basis().into_inner();
    let dx = spatial_resolution[i];

    let f0 = if x.coords[i] >= dx0 {
        lattice.get_point(strider, &(x - e * dx0)).copied()
    }
    else {
        None
    };
    let f1 = lattice.get_point(strider, &(x + e * dx1)).copied();

    // fixme: the boundary conditions should be invariant under dx
    boundary_conditions[i].apply_df(f0, f1) / dx
}
