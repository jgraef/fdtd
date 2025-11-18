use std::{
    convert::Infallible,
    time::Duration,
};

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

use crate::{
    app::solver::{
        config::{
            EvaluateStopCondition,
            StopCondition,
        },
        fdtd::{
            FdtdSolverConfig,
            Resolution,
            boundary_condition::{
                AnyBoundaryCondition,
                BoundaryCondition,
                default_boundary_conditions,
            },
            lattice::{
                Lattice,
                Strider,
            },
            util::{
                SwapBuffer,
                SwapBufferIndex,
                UpdateCoefficients,
                evaluate_stop_condition,
            },
        },
        traits::{
            Solver,
            SolverInstance,
        },
    },
    physics::{
        PhysicalConstants,
        material::MaterialDistribution,
    },
};

#[derive(Clone, Copy, Debug, Default)]
pub struct FdtdCpuSolver;

impl Solver for FdtdCpuSolver {
    type Config = FdtdSolverConfig;
    type Point = Point3<usize>;
    type Instance = FdtdCpuSolverInstance;
    type Error = Infallible;

    fn create_instance<M>(
        &self,
        config: &Self::Config,
        material: M,
    ) -> Result<Self::Instance, Self::Error>
    where
        M: MaterialDistribution<Self::Point>,
    {
        Ok(FdtdCpuSolverInstance::new(config, material))
    }

    fn memory_required(&self, config: &Self::Config) -> Option<usize> {
        let per_cell = std::mem::size_of::<UpdateCoefficients>()
            + std::mem::size_of::<SwapBuffer<FieldVectors>>();
        Some(per_cell * config.num_cells())
    }
}

#[derive(Clone, Debug)]
pub struct FdtdCpuSolverInstance {
    strider: Strider,
    resolution: Resolution,
    physical_constants: PhysicalConstants,
    update_coefficients: Lattice<UpdateCoefficients>,
    boundary_conditions: [AnyBoundaryCondition; 3],
}

impl FdtdCpuSolverInstance {
    fn new(config: &FdtdSolverConfig, material: impl MaterialDistribution<Point3<usize>>) -> Self {
        let strider = config.strider();
        let update_coefficients = Lattice::from_fn(&strider, |_index, point| {
            point
                .map(|point| {
                    UpdateCoefficients::new(
                        &config.resolution,
                        &config.physical_constants,
                        &material.at(&point),
                    )
                })
                .unwrap_or_default()
        });

        let boundary_conditions = default_boundary_conditions(&strider.size());

        Self {
            strider,
            resolution: config.resolution,
            physical_constants: config.physical_constants,
            update_coefficients,
            boundary_conditions,
        }
    }

    fn update_impl(&self, state: &mut FdtdCpuSolverState) {
        // note: CE page 68. we moved the delta_x from the coefficients into the sum,
        // which then becomes the curl plus source current density.
        // todo: integrate psi auxiliary fields

        let previous = SwapBufferIndex::from_tick(state.tick);
        let current = previous.other();

        //let mut energy = 0.0;

        // prepare sources
        // todo: we might need to pass some info to `prepare` so it knows what time is
        // for the magnetic and electric field
        //for source in &mut self.sources {
        //    source.prepare(self.time);
        //}

        // update magnetic field
        for (index, point) in self.strider.iter(..) {
            let e_jacobian = jacobian(
                &point,
                &Vector3::repeat(1),
                &Vector3::zeros(),
                &self.strider,
                &state.lattice,
                |cell| cell[previous].e,
                &self.resolution.spatial,
                &self.boundary_conditions,
            );
            let e_curl = e_jacobian.curl();

            let cell = &mut state.lattice[index];

            /*let m_source = if let Some(index) = cell.source {
                let m_source = &mut self.sources[index];
                let point_dx = Point3::from(
                    point
                        .map(|x| x as f64)
                        .coords
                        .component_mul(&self.resolution.spatial),
                );
                m_source.magnetic_current_density(self.time, &point_dx)
            }
            else {
                Vector3::zeros()
            };*/
            let m_source = Vector3::zeros();

            let psi = Vector3::zeros();

            let update_coefficients = self.update_coefficients[index];

            // note: the E and H field equations are almost identical, but here the curl is
            // negative.
            cell[current].h = update_coefficients.d_a * cell[previous].h
                + update_coefficients.d_b * (-e_curl - m_source + psi);

            // note: this is just for debugging
            //energy += cell[current].h.norm_squared()
            //    / (cell.material.relative_permeability
            //        * self.physical_constants.vacuum_permeability);
        }

        // update electric field
        //let time = state.time + 0.5 * self.resolution.temporal;
        for (index, point) in self.strider.iter(..) {
            let h_jacobian = jacobian(
                &point,
                &Vector3::zeros(),
                &Vector3::repeat(1),
                &self.strider,
                &state.lattice,
                |cell| {
                    // NOTE: this is `current` not `previous`, because we have already updated the H
                    // field with the new values in `current`.
                    cell[current].h
                },
                &self.resolution.spatial,
                &self.boundary_conditions,
            );
            let h_curl = h_jacobian.curl();

            let cell = &mut state.lattice[index];

            /*let j_source = if let Some(index) = cell.source {
                let j_source = &mut self.sources[index];
                let point_dx = Point3::from(
                    point
                        .map(|x| x as f64 + 0.5)
                        .coords
                        .component_mul(&self.resolution.spatial),
                );
                j_source.electric_current_density(time, &point_dx)
            }
            else {
                Vector3::zeros()
            };*/
            let j_source = Vector3::zeros();

            let psi = Vector3::zeros();

            let update_coefficients = self.update_coefficients[index];

            cell[current].e = update_coefficients.c_a * cell[previous].e
                + update_coefficients.c_b * (h_curl - j_source + psi);

            // note: this is just for debugging
            //energy += cell[current].e.norm_squared()
            //    * cell.material.relative_permittivity
            //    * self.physical_constants.vacuum_permittivity;
        }

        state.tick += 1;
        state.time += self.resolution.temporal;
        //self.total_energy = 0.5 * energy * self.resolution.spatial.product();
    }
}

impl SolverInstance for FdtdCpuSolverInstance {
    type State = FdtdCpuSolverState;
    type Point = Point3<usize>;

    fn create_state(&self) -> Self::State {
        FdtdCpuSolverState::new(&self.strider)
    }

    fn update(&self, state: &mut Self::State) {
        self.update_impl(state);
    }
}

impl EvaluateStopCondition for FdtdCpuSolverInstance {
    fn evaluate_stop_condition(
        &self,
        state: &Self::State,
        stop_condition: &StopCondition,
        time_elapsed: Duration,
    ) -> bool {
        evaluate_stop_condition(stop_condition, time_elapsed, state.tick, state.time)
    }
}

pub struct FdtdCpuSolverState {
    lattice: Lattice<SwapBuffer<FieldVectors>>,
    tick: usize,
    time: f64,
}

impl FdtdCpuSolverState {
    fn new(strider: &Strider) -> Self {
        let lattice = Lattice::from_default(strider);

        Self {
            lattice,
            tick: 0,
            time: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct FieldVectors {
    e: Vector3<f64>,
    h: Vector3<f64>,
}

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
pub(super) fn jacobian<T>(
    x: &Point3<usize>,
    dx0: &Vector3<usize>,
    dx1: &Vector3<usize>,
    strider: &Strider,
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
                strider,
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
                strider,
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
                strider,
                grid,
                &field,
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
fn partial_derivative<T>(
    axis: Axis,
    x: &Point3<usize>,
    dx0: &Vector3<usize>,
    dx1: &Vector3<usize>,
    strider: &Strider,
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
        grid.get_point(strider, &(x - e * dx0)).map(&field)
    }
    else {
        None
    };
    let f1 = grid.get_point(strider, &(x + e * dx1)).map(&field);

    // fixme: the boundary conditions should be invariant under dx
    boundary_conditions[i].apply_df(f0, f1) / dx
}
