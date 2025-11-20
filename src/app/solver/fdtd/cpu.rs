use std::{
    convert::Infallible,
    ops::{
        Range,
        RangeBounds,
    },
    sync::Arc,
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
use rayon::{
    ThreadPool,
    ThreadPoolBuildError,
    ThreadPoolBuilder,
    iter::ParallelIterator,
};

use crate::{
    app::solver::{
        DomainDescription,
        Field,
        FieldComponent,
        FieldMut,
        FieldView,
        SolverBackend,
        SolverInstance,
        SourceValues,
        Time,
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
                LatticeIter,
                LatticeIterMut,
                Strider,
            },
            util::{
                SwapBuffer,
                SwapBufferIndex,
                UpdateCoefficients,
                evaluate_stop_condition,
            },
        },
    },
    physics::PhysicalConstants,
    util::normalize_point_bounds,
};

/// Defines how a single/multi-threading iterates over the lattice in the state
/// update.
pub trait LatticeForEach: Send + Sync {
    fn for_each<T, F>(&self, strider: &Strider, lattice: &mut Lattice<T>, f: F)
    where
        T: Send + Sync,
        F: Fn(usize, Point3<usize>, &mut T) + Send + Sync;
}

/// Use single-threading
#[derive(Clone, Copy, Debug, Default)]
pub struct SingleThreaded;

impl LatticeForEach for SingleThreaded {
    fn for_each<T, F>(&self, strider: &Strider, lattice: &mut Lattice<T>, f: F)
    where
        T: Send + Sync,
        F: Fn(usize, Point3<usize>, &mut T) + Send + Sync,
    {
        lattice
            .iter_mut(strider, ..)
            .for_each(|(index, point, value)| f(index, point, value))
    }
}

/// Use multi-threading
#[derive(Clone, Debug)]
pub struct MultiThreaded {
    thread_pool: Option<Arc<ThreadPool>>,
}

impl LatticeForEach for MultiThreaded {
    fn for_each<T, F>(&self, strider: &Strider, lattice: &mut Lattice<T>, f: F)
    where
        T: Send + Sync,
        F: Fn(usize, Point3<usize>, &mut T) + Send + Sync,
    {
        let mut f = || {
            lattice
                .par_iter_mut(strider)
                .for_each(|(index, point, value)| f(index, point, value))
        };

        if let Some(thread_pool) = &self.thread_pool {
            thread_pool.install(f);
        }
        else {
            f();
        }
    }
}

impl MultiThreaded {
    /// Use default number of threads (see [`rayon::current_num_threads`])
    pub fn from_default_thread_pool() -> Self {
        Self { thread_pool: None }
    }

    pub fn from_num_threads(num_threads: usize) -> Result<Self, ThreadPoolBuildError> {
        Ok(Self::from_thread_pool(
            ThreadPoolBuilder::new().num_threads(num_threads).build()?,
        ))
    }

    pub fn from_thread_pool(thread_pool: ThreadPool) -> Self {
        Self {
            thread_pool: Some(Arc::new(thread_pool)),
        }
    }

    /// Use max number of threads (see [`rayon::max_num_threads`])
    pub fn max_threads() -> Result<Self, ThreadPoolBuildError> {
        Self::from_num_threads(rayon::max_num_threads())
    }
}

impl Default for MultiThreaded {
    fn default() -> Self {
        Self::from_default_thread_pool()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FdtdCpuBackend<Threading = SingleThreaded> {
    /// Whether to use single-threading or multi-threading
    pub threading: Threading,
}

impl Default for FdtdCpuBackend<SingleThreaded> {
    fn default() -> Self {
        Self::single_threaded()
    }
}

impl<Threading> FdtdCpuBackend<Threading> {
    pub fn new(threading: Threading) -> Self {
        Self { threading }
    }
}

impl FdtdCpuBackend<SingleThreaded> {
    pub fn single_threaded() -> Self {
        Self {
            threading: SingleThreaded,
        }
    }
}

impl FdtdCpuBackend<MultiThreaded> {
    pub fn multi_threaded(num_threads: Option<usize>) -> Result<Self, ThreadPoolBuildError> {
        let threading = if let Some(num_threads) = num_threads {
            MultiThreaded::from_num_threads(num_threads)?
        }
        else {
            MultiThreaded::from_default_thread_pool()
        };

        Ok(Self { threading })
    }

    pub fn num_threads(&self) -> usize {
        self.threading
            .thread_pool
            .as_ref()
            .map_or_else(rayon::current_num_threads, |thread_pool| {
                thread_pool.current_num_threads()
            })
    }
}

impl<Threading> SolverBackend for FdtdCpuBackend<Threading>
where
    Threading: LatticeForEach + Clone,
{
    type Config = FdtdSolverConfig;
    type Point = Point3<usize>;
    type Instance = FdtdCpuSolverInstance<Threading>;
    type Error = Infallible;

    fn create_instance<D>(
        &self,
        config: &Self::Config,
        domain_description: D,
    ) -> Result<Self::Instance, Self::Error>
    where
        D: DomainDescription<Self::Point>,
    {
        Ok(FdtdCpuSolverInstance::new(
            config,
            domain_description,
            self.threading.clone(),
        ))
    }

    fn memory_required(&self, config: &Self::Config) -> Option<usize> {
        let per_cell = std::mem::size_of::<UpdateCoefficients>()
            + 4 * std::mem::size_of::<SwapBuffer<Vector3<usize>>>();
        Some(per_cell * config.num_cells())
    }
}

#[derive(Clone, Debug)]
pub struct FdtdCpuSolverInstance<Threading = SingleThreaded> {
    strider: Strider,
    resolution: Resolution,
    physical_constants: PhysicalConstants,
    update_coefficients: Lattice<UpdateCoefficients>,
    boundary_conditions: [AnyBoundaryCondition; 3],
    threading: Threading,
}

impl<Threading> FdtdCpuSolverInstance<Threading> {
    fn new(
        config: &FdtdSolverConfig,
        domain_description: impl DomainDescription<Point3<usize>>,
        threading: Threading,
    ) -> Self {
        let strider = config.strider();
        let update_coefficients = Lattice::from_fn(&strider, |_index, point| {
            point
                .map(|point| {
                    UpdateCoefficients::new(
                        &config.resolution,
                        &config.physical_constants,
                        &domain_description.material(&point),
                    )
                })
                .unwrap_or_default()
        });

        let boundary_conditions = default_boundary_conditions(strider.size());

        Self {
            strider,
            resolution: config.resolution,
            physical_constants: config.physical_constants,
            update_coefficients,
            boundary_conditions,
            threading,
        }
    }
}

impl<Threading> FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    fn update_impl(
        &self,
        state: &mut FdtdCpuSolverState,
        sources: impl IntoIterator<Item = (Point3<usize>, SourceValues)>,
    ) {
        // note: CE page 68. we moved the delta_x from the coefficients into the sum,
        // which then becomes the curl plus source current density.
        // todo: integrate psi auxiliary fields

        let previous = SwapBufferIndex::from_tick(state.tick);
        let next = previous.other();

        // reset previous source values
        for (index, _source) in state.source_buffer.drain(..) {
            state.source_field[index] = 0;
        }

        // prepare sources
        assert!(state.source_buffer.is_empty());
        state.source_buffer.push(Default::default());
        for (point, source) in sources.into_iter() {
            if let Some(index) = self.strider.index(&point) {
                state.source_field[index] = state.source_buffer.len();
                state.source_buffer.push((index, source));
            }
        }

        //let mut energy = 0.0;

        // update magnetic field
        let (h_field_next, h_field_previous) = state.h_field.pair_mut(next);
        self.threading
            .for_each(&self.strider, h_field_next, |index, point, h_field_next| {
                let e_jacobian = jacobian(
                    &point,
                    &Vector3::repeat(1),
                    &Vector3::zeros(),
                    &self.strider,
                    &state.e_field[previous],
                    &self.resolution.spatial,
                    &self.boundary_conditions,
                );
                let e_curl = e_jacobian.curl();

                let source_id = state.source_field[index];
                let m_source = if source_id != 0 {
                    state.source_buffer[source_id].1.m_source
                }
                else {
                    Default::default()
                };

                let psi = Vector3::zeros();

                let update_coefficients = self.update_coefficients[index];

                // note: the E and H field equations are almost identical, but here the curl is
                // negative.
                *h_field_next = update_coefficients.d_a * h_field_previous[index]
                    + update_coefficients.d_b * (-e_curl - m_source + psi);

                // note: this is just for debugging
                //energy += cell[current].h.norm_squared()
                //    / (cell.material.relative_permeability
                //        * self.physical_constants.vacuum_permeability);
            });

        // update electric field
        //let time = state.time + 0.5 * self.resolution.temporal;
        let (e_field_next, e_field_previous) = state.e_field.pair_mut(next);
        self.threading
            .for_each(&self.strider, e_field_next, |index, point, e_field_next| {
                let h_jacobian = jacobian(
                    &point,
                    &Vector3::zeros(),
                    &Vector3::repeat(1),
                    &self.strider,
                    // NOTE: this is `current` not `previous`, because we have already updated the
                    // H field with the new values in `current`.
                    &state.h_field[next],
                    &self.resolution.spatial,
                    &self.boundary_conditions,
                );
                let h_curl = h_jacobian.curl();

                let source_id = state.source_field[index];
                let j_source = if source_id != 0 {
                    state.source_buffer[source_id].1.j_source
                }
                else {
                    Default::default()
                };

                let psi = Vector3::zeros();

                let update_coefficients = self.update_coefficients[index];

                *e_field_next = update_coefficients.c_a * e_field_previous[index]
                    + update_coefficients.c_b * (h_curl - j_source + psi);

                // note: this is just for debugging
                //energy += cell[current].e.norm_squared()
                //    * cell.material.relative_permittivity
                //    * self.physical_constants.vacuum_permittivity;
            });

        state.tick += 1;
        state.time += self.resolution.temporal;
        //self.total_energy = 0.5 * energy * self.resolution.spatial.product();
    }
}

impl<Threading> SolverInstance for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    type State = FdtdCpuSolverState;
    type Point = Point3<usize>;
    type Source = SourceValues;

    fn create_state(&self) -> Self::State {
        FdtdCpuSolverState::new(&self.strider)
    }

    fn update<S>(&self, state: &mut Self::State, sources: S)
    where
        S: IntoIterator<Item = (Point3<usize>, SourceValues)>,
    {
        self.update_impl(state, sources);
    }
}

impl<Threading> EvaluateStopCondition for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    fn evaluate_stop_condition(
        &self,
        state: &Self::State,
        stop_condition: &StopCondition,
        time_elapsed: Duration,
    ) -> bool {
        evaluate_stop_condition(stop_condition, time_elapsed, state.tick, state.time)
    }
}

#[derive(Clone, Debug)]
pub struct FdtdCpuSolverState {
    h_field: SwapBuffer<Lattice<Vector3<f64>>>,
    e_field: SwapBuffer<Lattice<Vector3<f64>>>,
    source_field: Lattice<usize>,
    source_buffer: Vec<(usize, SourceValues)>,
    tick: usize,
    time: f64,
}

impl FdtdCpuSolverState {
    fn new(strider: &Strider) -> Self {
        Self {
            h_field: SwapBuffer::from_fn(|_| Lattice::from_default(strider)),
            e_field: SwapBuffer::from_fn(|_| Lattice::from_default(strider)),
            source_field: Lattice::from_default(strider),
            source_buffer: vec![],
            tick: 0,
            time: 0.0,
        }
    }

    pub fn tick(&self) -> usize {
        self.tick
    }

    pub fn time(&self) -> f64 {
        self.time
    }
}

impl Time for FdtdCpuSolverState {
    fn tick(&self) -> usize {
        self.tick
    }

    fn time(&self) -> f64 {
        self.time
    }
}

impl<Threading> Field for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    type View<'a>
        = CpuFieldView<'a>
    where
        Self: 'a;

    fn field<'a, R>(
        &'a self,
        state: &'a Self::State,
        range: R,
        field_component: FieldComponent,
    ) -> Self::View<'a>
    where
        R: RangeBounds<Self::Point>,
    {
        let range = normalize_point_bounds(range, *self.strider.size());

        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick);

        let swap_buffer = match field_component {
            FieldComponent::H => &state.h_field,
            FieldComponent::E => &state.e_field,
        };

        let lattice = &swap_buffer[swap_buffer_index];

        CpuFieldView {
            range,
            strider: &self.strider,
            lattice,
        }
    }
}

#[derive(Debug)]
pub struct CpuFieldView<'a> {
    range: Range<Point3<usize>>,
    strider: &'a Strider,
    lattice: &'a Lattice<Vector3<f64>>,
}

impl<'a> FieldView<Point3<usize>> for CpuFieldView<'a> {
    type Iter<'b>
        = CpuFieldIter<'b>
    where
        Self: 'b;

    fn at(&self, point: &Point3<usize>) -> Option<Vector3<f64>> {
        if self.range.contains(point) {
            let value = self.lattice.get_point(self.strider, point)?;
            Some(*value)
        }
        else {
            None
        }
    }

    fn iter<'b>(&'b self) -> Self::Iter<'b> {
        CpuFieldIter {
            lattice_iter: self.lattice.iter(self.strider, self.range.clone()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CpuFieldIter<'a> {
    lattice_iter: LatticeIter<'a, Vector3<f64>>,
}

impl<'a> Iterator for CpuFieldIter<'a> {
    type Item = (Point3<usize>, Vector3<f64>);

    fn next(&mut self) -> Option<Self::Item> {
        let (_index, point, value) = self.lattice_iter.next()?;
        Some((point, *value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.lattice_iter.size_hint()
    }
}

impl<'a> ExactSizeIterator for CpuFieldIter<'a> where
    LatticeIter<'a, Vector3<f64>>: ExactSizeIterator
{
}

impl FieldMut for FdtdCpuSolverInstance {
    type IterMut<'a>
        = CpuFieldRegionIterMut<'a>
    where
        Self: 'a;

    fn field_mut<'a, R>(
        &'a self,
        state: &'a mut Self::State,
        range: R,
        field_component: FieldComponent,
    ) -> Self::IterMut<'a>
    where
        R: RangeBounds<Self::Point>,
    {
        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick);

        let swap_buffer = match field_component {
            FieldComponent::H => &mut state.h_field,
            FieldComponent::E => &mut state.e_field,
        };

        let lattice = &mut swap_buffer[swap_buffer_index];

        CpuFieldRegionIterMut {
            lattice_iter: lattice.iter_mut(&self.strider, range),
        }
    }
}

#[derive(Debug)]
pub struct CpuFieldRegionIterMut<'a> {
    lattice_iter: LatticeIterMut<'a, Vector3<f64>>,
}

impl<'a> Iterator for CpuFieldRegionIterMut<'a> {
    type Item = (Point3<usize>, &'a mut Vector3<f64>);

    fn next(&mut self) -> Option<Self::Item> {
        let (_index, point, value) = self.lattice_iter.next()?;
        Some((point, value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.lattice_iter.size_hint()
    }
}

impl<'a> ExactSizeIterator for CpuFieldRegionIterMut<'a> where
    LatticeIter<'a, Vector3<f64>>: ExactSizeIterator
{
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
