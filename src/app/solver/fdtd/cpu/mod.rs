mod lattice;
pub mod project;
mod util;

use std::{
    convert::Infallible,
    ops::{
        Range,
        RangeBounds,
    },
    time::Duration,
};

use nalgebra::{
    Point3,
    Vector3,
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
        UpdatePass,
        UpdatePassForcing,
        config::{
            EvaluateStopCondition,
            StopCondition,
        },
        fdtd::{
            FdtdSolverConfig,
            Resolution,
            boundary_condition::{
                AnyBoundaryCondition,
                default_boundary_conditions,
            },
            cpu::{
                lattice::{
                    Lattice,
                    LatticeIter,
                    LatticeIterMut,
                },
                util::jacobian,
            },
            strider::Strider,
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
pub trait LatticeForEach: Send + Sync + 'static {
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
#[cfg(feature = "rayon")]
#[derive(Clone, Debug)]
pub struct MultiThreaded {
    thread_pool: Option<std::sync::Arc<rayon::ThreadPool>>,
}

#[cfg(feature = "rayon")]
impl LatticeForEach for MultiThreaded {
    fn for_each<T, F>(&self, strider: &Strider, lattice: &mut Lattice<T>, f: F)
    where
        T: Send + Sync,
        F: Fn(usize, Point3<usize>, &mut T) + Send + Sync,
    {
        use rayon::iter::ParallelIterator as _;

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

#[cfg(feature = "rayon")]
impl MultiThreaded {
    /// Use default number of threads (see [`rayon::current_num_threads`])
    pub fn from_default_thread_pool() -> Self {
        Self { thread_pool: None }
    }

    pub fn from_num_threads(num_threads: usize) -> Result<Self, rayon::ThreadPoolBuildError> {
        Ok(Self::from_thread_pool(
            rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()?,
        ))
    }

    pub fn from_thread_pool(thread_pool: rayon::ThreadPool) -> Self {
        Self {
            thread_pool: Some(std::sync::Arc::new(thread_pool)),
        }
    }

    /// Use max number of threads (see [`rayon::max_num_threads`])
    pub fn max_threads() -> Result<Self, rayon::ThreadPoolBuildError> {
        Self::from_num_threads(rayon::max_num_threads())
    }
}

#[cfg(feature = "rayon")]
impl Default for MultiThreaded {
    fn default() -> Self {
        Self::from_default_thread_pool()
    }
}

#[cfg(feature = "rayon")]
impl FdtdCpuBackend<MultiThreaded> {
    pub fn multi_threaded(num_threads: Option<usize>) -> Result<Self, rayon::ThreadPoolBuildError> {
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

impl<Threading> SolverBackend<FdtdSolverConfig, Point3<usize>> for FdtdCpuBackend<Threading>
where
    Threading: LatticeForEach + Clone,
{
    type Instance = FdtdCpuSolverInstance<Threading>;
    type Error = Infallible;

    fn create_instance<D>(
        &self,
        config: &FdtdSolverConfig,
        domain_description: D,
    ) -> Result<Self::Instance, Self::Error>
    where
        D: DomainDescription<Point3<usize>>,
    {
        Ok(FdtdCpuSolverInstance::new(
            config,
            domain_description,
            self.threading.clone(),
        ))
    }

    fn memory_required(&self, config: &FdtdSolverConfig) -> Option<usize> {
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

impl<Threading> SolverInstance for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    type State = FdtdCpuSolverState;
    type UpdatePass<'a>
        = FdtdCpuUpdatePass<'a, Threading>
    where
        Self: 'a;

    fn create_state(&self) -> Self::State {
        FdtdCpuSolverState::new(&self.strider)
    }

    fn begin_update<'a>(&'a self, state: &'a mut Self::State) -> FdtdCpuUpdatePass<'a, Threading> {
        FdtdCpuUpdatePass::new(self, state)
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

    fn field(&self, field_component: FieldComponent) -> &SwapBuffer<Lattice<Vector3<f64>>> {
        match field_component {
            FieldComponent::H => &self.h_field,
            FieldComponent::E => &self.e_field,
        }
    }

    fn field_mut(
        &mut self,
        field_component: FieldComponent,
    ) -> &mut SwapBuffer<Lattice<Vector3<f64>>> {
        match field_component {
            FieldComponent::H => &mut self.h_field,
            FieldComponent::E => &mut self.e_field,
        }
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

#[derive(Debug)]
pub struct FdtdCpuUpdatePass<'a, Threading> {
    instance: &'a FdtdCpuSolverInstance<Threading>,
    state: &'a mut FdtdCpuSolverState,
}

impl<'a, Threading> FdtdCpuUpdatePass<'a, Threading>
where
    Threading: LatticeForEach,
{
    fn new(
        instance: &'a FdtdCpuSolverInstance<Threading>,
        state: &'a mut FdtdCpuSolverState,
    ) -> Self {
        // reset previous source values
        for (index, _source) in state.source_buffer.drain(..) {
            state.source_field[index] = 0;
        }

        // prepare sources
        assert!(state.source_buffer.is_empty());
        state.source_buffer.push(Default::default());

        Self { instance, state }
    }
}

impl<'a, Threading> UpdatePassForcing<Point3<usize>> for FdtdCpuUpdatePass<'a, Threading>
where
    Threading: LatticeForEach,
{
    fn set_forcing(&mut self, point: &Point3<usize>, value: &SourceValues) {
        let cell_index = self
            .instance
            .strider
            .index(point)
            .unwrap_or_else(|| panic!("set_forcing called with invalid point: {point:?}"));

        let source_index = &mut self.state.source_field[cell_index];
        if *source_index == 0 {
            // cell doesn't have a source set, push into buffer
            *source_index = self.state.source_buffer.len();
            self.state.source_buffer.push((cell_index, *value));
        }
        else {
            // source for this cell was already assigned, overwrite value in buffer.
            assert_eq!(self.state.source_buffer[*source_index].0, cell_index);
            self.state.source_buffer[*source_index].1 = *value;
        }
    }
}

impl<'a, Threading> UpdatePass for FdtdCpuUpdatePass<'a, Threading>
where
    Threading: LatticeForEach,
{
    fn finish(self) {
        // note: CE page 68. we moved the delta_x from the coefficients into the sum,
        // which then becomes the curl plus source current density.
        // todo: integrate psi auxiliary fields

        let previous = SwapBufferIndex::from_tick(self.state.tick);
        let next = previous.other();

        //let mut energy = 0.0;

        // update magnetic field
        let (h_field_next, h_field_previous) = self.state.h_field.pair_mut(next);
        self.instance.threading.for_each(
            &self.instance.strider,
            h_field_next,
            |index, point, h_field_next| {
                let e_jacobian = jacobian(
                    &point,
                    &Vector3::repeat(1),
                    &Vector3::zeros(),
                    &self.instance.strider,
                    &self.state.e_field[previous],
                    &self.instance.resolution.spatial,
                    &self.instance.boundary_conditions,
                );
                let e_curl = e_jacobian.curl();

                let source_id = self.state.source_field[index];
                let m_source = if source_id != 0 {
                    self.state.source_buffer[source_id].1.m
                }
                else {
                    Default::default()
                };

                let psi = Vector3::zeros();

                let update_coefficients = self.instance.update_coefficients[index];

                // note: the E and H field equations are almost identical, but here the curl is
                // negative.
                *h_field_next = update_coefficients.d_a * h_field_previous[index]
                    + update_coefficients.d_b * (-e_curl - m_source + psi);

                // note: this is just for debugging
                //energy += cell[current].h.norm_squared()
                //    / (cell.material.relative_permeability
                //        * self.physical_constants.vacuum_permeability);
            },
        );

        // update electric field
        //let time = state.time + 0.5 * self.resolution.temporal;
        let (e_field_next, e_field_previous) = self.state.e_field.pair_mut(next);
        self.instance.threading.for_each(
            &self.instance.strider,
            e_field_next,
            |index, point, e_field_next| {
                let h_jacobian = jacobian(
                    &point,
                    &Vector3::zeros(),
                    &Vector3::repeat(1),
                    &self.instance.strider,
                    // NOTE: this is `current` not `previous`, because we have already updated the
                    // H field with the new values in `current`.
                    &self.state.h_field[next],
                    &self.instance.resolution.spatial,
                    &self.instance.boundary_conditions,
                );
                let h_curl = h_jacobian.curl();

                let source_id = self.state.source_field[index];
                let j_source = if source_id != 0 {
                    self.state.source_buffer[source_id].1.j
                }
                else {
                    Default::default()
                };

                let psi = Vector3::zeros();

                let update_coefficients = self.instance.update_coefficients[index];

                *e_field_next = update_coefficients.c_a * e_field_previous[index]
                    + update_coefficients.c_b * (h_curl - j_source + psi);

                // note: this is just for debugging
                //energy += cell[current].e.norm_squared()
                //    * cell.material.relative_permittivity
                //    * self.physical_constants.vacuum_permittivity;
            },
        );

        self.state.tick += 1;
        self.state.time += self.instance.resolution.temporal;
        //self.total_energy = 0.5 * energy * self.resolution.spatial.product();
    }
}

impl<Threading> Field<Point3<usize>> for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    type View<'a>
        = CpuFieldView<'a>
    where
        Self: 'a;

    fn field<'a, R>(
        &'a self,
        state: &'a FdtdCpuSolverState,
        range: R,
        field_component: FieldComponent,
    ) -> Self::View<'a>
    where
        R: RangeBounds<Point3<usize>>,
    {
        let range = normalize_point_bounds(range, *self.strider.size());

        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick);
        let lattice = &state.field(field_component)[swap_buffer_index];

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

impl FieldMut<Point3<usize>> for FdtdCpuSolverInstance {
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
        R: RangeBounds<Point3<usize>>,
    {
        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick);
        let lattice = &mut state.field_mut(field_component)[swap_buffer_index];

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
