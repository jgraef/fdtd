use std::{
    fmt::Debug,
    ops::{
        Index,
        IndexMut,
    },
};

use nalgebra::{
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
    boundary_condition::{
        AnyBoundaryCondition,
        default_boundary_conditions,
    },
    geometry::Rasterize,
    lattice::Lattice,
    material::Material,
    source::Source,
    util::jacobian,
};

#[derive(Clone, Debug)]
pub struct Cell {
    /// Material properties
    material: Material,

    /// H
    magnetic_field: SwapBuffer<Vector3<f64>>,
    /// E
    electric_field: SwapBuffer<Vector3<f64>>,

    /// index into `Simulation::source`, defining magnetic and electric current
    /// density functions
    source: Option<usize>,

    magnetic_coefficients: Option<UpdateCoefficients>,
    electric_coefficients: Option<UpdateCoefficients>,
}

impl Default for Cell {
    fn default() -> Self {
        Self::from_material(Material::default())
    }
}

impl Cell {
    pub fn from_material(material: Material) -> Self {
        Self {
            material,
            magnetic_field: Default::default(),
            electric_field: Default::default(),
            source: None,
            magnetic_coefficients: None,
            electric_coefficients: None,
        }
    }

    pub fn reset(&mut self) {
        self.magnetic_field = Default::default();
        self.electric_field = Default::default();
    }

    pub fn material(&self) -> &Material {
        &self.material
    }

    pub fn set_material(&mut self, material: Material) {
        self.material = material;
        self.magnetic_coefficients = None;
        self.electric_coefficients = None;
    }

    pub fn electric_field(&self, swap_buffer_index: SwapBufferIndex) -> &Vector3<f64> {
        &self.electric_field[swap_buffer_index]
    }

    pub fn electric_field_mut(&mut self, swap_buffer_index: SwapBufferIndex) -> &Vector3<f64> {
        &mut self.electric_field[swap_buffer_index]
    }

    pub fn magnetic_field(&self, swap_buffer_index: SwapBufferIndex) -> &Vector3<f64> {
        &self.magnetic_field[swap_buffer_index]
    }

    pub fn magnetic_field_mut(&mut self, swap_buffer_index: SwapBufferIndex) -> &Vector3<f64> {
        &mut self.magnetic_field[swap_buffer_index]
    }

    fn magnetic_coefficients<'a>(
        &'a mut self,
        resolution: &Resolution,
        physical_constants: &PhysicalConstants,
    ) -> &'a UpdateCoefficients {
        self.magnetic_coefficients.get_or_insert_with(|| {
            UpdateCoefficients::new_magnetic(resolution, physical_constants, &self.material)
        })
    }

    fn electric_coefficients<'a>(
        &'a mut self,
        resolution: &Resolution,
        physical_constants: &PhysicalConstants,
    ) -> &'a UpdateCoefficients {
        self.electric_coefficients.get_or_insert_with(|| {
            UpdateCoefficients::new_electric(resolution, physical_constants, &self.material)
        })
    }
}

/// See CE page 67
/// These correspond to either `C_a` and `C_b` for the electric field update, or
/// `D_a` and `D_b` for the magnetic field update.
/// We don't calculate a `C_b_i` for each axis (`i`) though and instead do the
/// scaling by the spatial resolution later.
#[derive(Clone, Copy, Debug, Default)]
struct UpdateCoefficients {
    a: f64,
    b: f64,
}

impl UpdateCoefficients {
    /// - `sigma`: Either electrical or magnetic conductivity
    /// - `perm`: Either permittivity or permability
    pub fn new(resolution: &Resolution, sigma: f64, perm: f64) -> Self {
        let sigma_delta_t = sigma * resolution.temporal;
        Self {
            a: (1.0 - 0.5 * sigma_delta_t / perm) / (1.0 + 0.5 * sigma_delta_t / perm),
            b: resolution.temporal / (perm * (1.0 + 0.5 * sigma_delta_t / perm)),
        }
    }

    pub fn new_electric(
        resolution: &Resolution,
        physical_constants: &PhysicalConstants,
        material: &Material,
    ) -> Self {
        Self::new(
            resolution,
            material.eletrical_conductivity,
            material.relative_permittivity * physical_constants.vacuum_permittivity,
        )
    }

    pub fn new_magnetic(
        resolution: &Resolution,
        physical_constants: &PhysicalConstants,
        material: &Material,
    ) -> Self {
        Self::new(
            resolution,
            material.magnetic_conductivity,
            material.relative_permeability * physical_constants.vacuum_permeability,
        )
    }
}

/// Buffer holding 2 values.
///
/// One value is the current value, the other one is the value from the previous
/// step. Which one is which depends on the [`SwapBufferIndex`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SwapBuffer<T> {
    buffer: [T; 2],
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

#[derive(derive_more::Debug)]
pub struct Simulation {
    resolution: Resolution,
    physical_constants: PhysicalConstants,

    tick: usize,
    time: f64,
    origin: Vector3<f64>,
    total_energy: f64,

    lattice: Lattice<Cell>,
    boundary_conditions: [AnyBoundaryCondition; 3],

    #[debug(ignore)]
    sources: Vec<Box<dyn Source>>,
}

impl Simulation {
    pub fn new(
        size: Vector3<f64>,
        physical_constants: PhysicalConstants,
        resolution: Resolution,
    ) -> Self {
        let lattice_size = size
            .component_div(&resolution.spatial)
            .map(|x| (x.ceil() as usize).max(1));
        let origin = 0.5 * size;

        let lattice = Lattice::new(lattice_size, |_| Cell::default());
        let boundary_conditions = default_boundary_conditions(&lattice_size);

        Self {
            physical_constants,
            resolution,
            tick: 0,
            time: 0.0,
            origin,
            total_energy: 0.0,
            lattice,
            boundary_conditions,
            sources: vec![],
        }
    }

    pub fn reset(&mut self) {
        self.tick = 0;
        self.time = 0.0;
        self.lattice.iter_mut().for_each(|(_, cell)| cell.reset());

        for source in &mut self.sources {
            source.reset();
        }
    }

    pub fn step(&mut self) {
        let previous = SwapBufferIndex::from_tick(self.tick);
        let current = previous.other();

        let mut energy = 0.0;

        // prepare sources
        // todo: we might need to pass some info to `prepare` so it knows what time is
        // for the magnetic and electric field
        for source in &mut self.sources {
            source.prepare(self.time);
        }

        // update magnetic field
        for point in self.lattice.iter_points() {
            let e_jacobian = jacobian(
                &point,
                &Vector3::repeat(1),
                &Vector3::zeros(),
                &self.lattice,
                |cell| cell.electric_field[previous],
                &self.resolution.spatial,
                &self.boundary_conditions,
            );
            let e_curl = e_jacobian.curl();

            let cell = self.lattice.get_mut(&point).unwrap();

            let m_source = if let Some(index) = cell.source {
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
            };

            let coefficients =
                *cell.magnetic_coefficients(&self.resolution, &self.physical_constants);

            let psi = Vector3::zeros();

            // note: the E and H field equations are almost identical, but here the curl is
            // negative.
            cell.magnetic_field[current] = coefficients.a * cell.magnetic_field[previous]
                + coefficients.b * (-e_curl - m_source + psi);

            // note: this is just for debugging
            energy += cell.magnetic_field[current].norm_squared()
                / (cell.material.relative_permeability
                    * self.physical_constants.vacuum_permeability);
        }

        // update electric field
        let time = self.time + 0.5 * self.resolution.temporal;
        for point in self.lattice.iter_points() {
            let h_jacobian = jacobian(
                &point,
                &Vector3::zeros(),
                &Vector3::repeat(1),
                &self.lattice,
                |cell| {
                    // NOTE: this is `current` not `previous`, because we have already updated the H
                    // field with the new values in `current`.
                    cell.magnetic_field[current]
                },
                &self.resolution.spatial,
                &self.boundary_conditions,
            );
            let h_curl = h_jacobian.curl();

            let cell = self.lattice.get_mut(&point).unwrap();

            let j_source = if let Some(index) = cell.source {
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
            };

            let psi = Vector3::zeros();

            let coefficients =
                *cell.electric_coefficients(&self.resolution, &self.physical_constants);
            cell.electric_field[current] = coefficients.a * cell.electric_field[previous]
                + coefficients.b * (h_curl - j_source + psi);

            // note: this is just for debugging
            energy += cell.electric_field[current].norm_squared()
                * cell.material.relative_permittivity
                * self.physical_constants.vacuum_permittivity;
        }

        self.tick += 1;
        self.time += self.resolution.temporal;
        self.total_energy = 0.5 * energy * self.resolution.spatial.product();
    }

    pub fn tick(&self) -> usize {
        self.tick
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    /// Returns the [`SwapBufferIndex`] for the last computed tick.
    ///
    /// To get the [`SwapBufferIndex`] for the tick before that you can use
    /// [`SwapBufferIndex::other`].
    pub fn swap_buffer_index(&self) -> SwapBufferIndex {
        SwapBufferIndex::from_tick(self.tick)
    }

    pub fn physical_constants(&self) -> &PhysicalConstants {
        &self.physical_constants
    }

    pub fn resolution(&self) -> &Resolution {
        &self.resolution
    }

    pub(crate) fn origin(&self) -> &Vector3<f64> {
        &self.origin
    }

    pub fn size(&self) -> Vector3<f64> {
        self.lattice
            .dimensions()
            .zip_map(&self.resolution.spatial, |x, dx| x as f64 * dx)
    }

    pub fn total_energy(&self) -> f64 {
        self.total_energy
    }

    pub fn lattice(&self) -> &Lattice<Cell> {
        &self.lattice
    }

    pub fn lattice_mut(&mut self) -> &mut Lattice<Cell> {
        &mut self.lattice
    }

    pub fn add_material(&mut self, geometry: impl Rasterize, material: Material) {
        for point in geometry.rasterize(self) {
            if let Some(cell) = self.lattice.get_mut(&point) {
                cell.material = material;
            }
        }
    }

    pub fn add_source(&mut self, geometry: impl Rasterize, source: impl Source) {
        let index = self.sources.len();
        self.sources.push(Box::new(source));

        for point in geometry.rasterize(self) {
            if let Some(cell) = self.lattice.get_mut(&point) {
                cell.source = Some(index);
            }
        }
    }

    /// Returns field values along an axis-aligned line.
    pub fn field_values<'a, T, F>(
        &'a self,
        mut x0: Point3<usize>,
        axis: Axis,
        x_correction: f64,
        f: F,
    ) -> impl Iterator<Item = (f64, T)> + 'a
    where
        F: Fn(&Cell, SwapBufferIndex) -> T + 'a,
    {
        *axis.vector_component_mut(&mut x0.coords) = 0;

        let n = *axis.vector_component(&self.lattice.dimensions());
        let e = axis.basis().into_inner();
        let resolution = *axis.vector_component(&self.resolution.spatial);
        let origin = *axis.vector_component(&self.origin);
        let swap_buffer_index = self.swap_buffer_index();

        (0..n).map(move |i| {
            let x = x0 + i * e;
            let cell = self.lattice.get(&x).unwrap();
            let value = f(&cell, swap_buffer_index);
            (i as f64 * resolution - origin + x_correction, value)
        })
    }
}

#[derive(Clone, Copy)]
pub struct PhysicalConstants {
    pub vacuum_permittivity: f64,
    pub vacuum_permeability: f64,
}

impl Debug for PhysicalConstants {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicalConstants")
            .field("vacuum_permittivity", &self.vacuum_permittivity)
            .field("vacuum_permeability", &self.vacuum_permeability)
            .field("speed_of_light", &self.speed_of_light())
            .finish()
    }
}

impl Default for PhysicalConstants {
    fn default() -> Self {
        Self::SI
    }
}

impl PhysicalConstants {
    pub const SI: Self = Self {
        vacuum_permittivity: 8.8541878188e-12,
        vacuum_permeability: 1.25663706127e-6,
    };

    pub const REDUCED: Self = Self {
        vacuum_permittivity: 1.0,
        vacuum_permeability: 1.0,
    };

    pub fn speed_of_light(&self) -> f64 {
        (self.vacuum_permittivity * self.vacuum_permeability).powf(-0.5)
    }

    pub fn frequency_to_wavelength(&self, frequency: f64) -> f64 {
        self.speed_of_light() / frequency
    }

    pub fn wavelength_to_frequency(&self, wavelength: f64) -> f64 {
        self.speed_of_light() / wavelength
    }

    pub fn estimate_temporal_from_spatial_resolution(
        self,
        spatial_resolution: &Vector3<f64>,
    ) -> f64 {
        spatial_resolution.min() / (self.speed_of_light() * 3.0f64.sqrt())
    }

    pub fn estimate_spatial_from_temporal_resolution(
        &self,
        temporal_resolution: f64,
    ) -> Vector3<f64> {
        Vector3::repeat(temporal_resolution * self.speed_of_light() * 3.0f64.sqrt())
    }

    pub fn estimate_spatial_resolution_from_min_wavelength(
        &self,
        min_wavelength: f64,
    ) -> Vector3<f64> {
        Vector3::repeat(min_wavelength / (9.0f64 * 3.0f64.sqrt()))
    }

    pub fn estimate_temporal_resolution_from_max_frequency(&self, max_frequency: f64) -> f64 {
        1.0f64 / (9.0f64 * 3.0f64 * max_frequency)
    }

    pub fn estimate_resolution_from_min_wavelength(&self, min_wavelength: f64) -> Resolution {
        let spatial = self.estimate_spatial_resolution_from_min_wavelength(min_wavelength);
        let temporal = self.estimate_temporal_from_spatial_resolution(&spatial);
        Resolution { spatial, temporal }
    }

    pub fn estimate_resolution_from_max_frequency(&self, max_frequency: f64) -> Resolution {
        let temporal = self.estimate_temporal_resolution_from_max_frequency(max_frequency);
        let spatial = self.estimate_spatial_from_temporal_resolution(temporal);
        Resolution { spatial, temporal }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Resolution {
    pub spatial: Vector3<f64>,
    pub temporal: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
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
