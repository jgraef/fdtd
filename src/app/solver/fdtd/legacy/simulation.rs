use std::fmt::Debug;

use nalgebra::{
    Point3,
    Vector3,
};

use crate::{
    app::solver::fdtd::{
        FdtdSolverConfig,
        Resolution,
        boundary_condition::{
            AnyBoundaryCondition,
            default_boundary_conditions,
        },
        cpu::{
            Axis,
            jacobian,
        },
        lattice::{
            Lattice,
            Strider,
        },
        legacy::{
            geometry::Rasterize,
            pml::PmlCell,
            source::Source,
        },
        util::{
            SwapBuffer,
            SwapBufferIndex,
        },
    },
    physics::{
        PhysicalConstants,
        material::Material,
    },
};

#[derive(Clone, Debug)]
pub struct Cell {
    /// Material properties
    material: Material,

    /// H
    magnetic_field: SwapBuffer<Vector3<f64>>,
    /// E
    electric_field: SwapBuffer<Vector3<f64>>,

    /// precomputed coefficients for update equations
    ///
    /// These depend on [`Self::material`] and must be recomputed if the
    /// material changes.
    magnetic_coefficients: Option<UpdateCoefficients>,
    electric_coefficients: Option<UpdateCoefficients>,

    /// index into `Simulation::source`, defining magnetic and electric current
    /// density functions
    source: Option<usize>,

    /// data relevant for cells that are PML.
    pml: Option<PmlCell>,
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
            magnetic_coefficients: None,
            electric_coefficients: None,
            source: None,
            pml: None,
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
        let half_sigmal_delta_t_over_perm = 0.5 * sigma * resolution.temporal / perm;
        Self {
            a: (1.0 - half_sigmal_delta_t_over_perm) / (1.0 + half_sigmal_delta_t_over_perm),
            b: resolution.temporal / (perm * (1.0 + half_sigmal_delta_t_over_perm)),
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

#[derive(derive_more::Debug)]
pub struct Simulation {
    resolution: Resolution,
    physical_constants: PhysicalConstants,

    tick: usize,
    time: f64,
    total_energy: f64,

    lattice: Lattice<Cell>,
    strider: Strider,
    boundary_conditions: [AnyBoundaryCondition; 3],

    #[debug(ignore)]
    sources: Vec<Box<dyn Source>>,
}

impl Simulation {
    pub fn new(config: &FdtdSolverConfig) -> Self {
        let strider = config.strider();
        let lattice = Lattice::from_default(&strider);
        let boundary_conditions = default_boundary_conditions(&strider.size());

        Self {
            physical_constants: config.physical_constants,
            resolution: config.resolution,
            tick: 0,
            time: 0.0,
            total_energy: 0.0,
            lattice,
            strider,
            boundary_conditions,
            sources: vec![],
        }
    }

    pub fn reset(&mut self) {
        self.tick = 0;
        self.time = 0.0;
        self.lattice
            .iter_mut(&self.strider, ..)
            .for_each(|(_, _, cell)| cell.reset());

        for source in &mut self.sources {
            source.reset();
        }
    }

    pub fn step(&mut self) {
        // note: CE page 68. we moved the delta_x from the coefficients into the sum,
        // which then becomes the curl plus source current density.
        // todo: integrate psi auxiliary fields

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
        for (index, point) in self.strider.iter(..) {
            let e_jacobian = jacobian(
                &point,
                &Vector3::repeat(1),
                &Vector3::zeros(),
                &self.strider,
                &self.lattice,
                |cell| cell.electric_field[previous],
                &self.resolution.spatial,
                &self.boundary_conditions,
            );
            let e_curl = e_jacobian.curl();

            let cell = &mut self.lattice[index];

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
        for (index, point) in self.strider.iter(..) {
            let h_jacobian = jacobian(
                &point,
                &Vector3::zeros(),
                &Vector3::repeat(1),
                &self.strider,
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

            let cell = &mut self.lattice[index];

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

    pub fn size(&self) -> Vector3<f64> {
        self.strider
            .size()
            .zip_map(&self.resolution.spatial, |x, dx| x as f64 * dx)
    }

    pub fn strider(&self) -> &Strider {
        &self.strider
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
            let cell = point
                .ok()
                .and_then(|x| self.lattice.get_point_mut(&self.strider, &x))
                .unwrap_or_else(|| {
                    panic!("point outside lattice: {point:?}");
                });

            cell.set_material(material);
        }
    }

    pub fn fill_with(&mut self, mut f: impl FnMut(Point3<f64>, &mut Cell)) {
        for (_index, point, cell) in self.lattice.iter_mut(&self.strider, ..) {
            let point_float = point
                .coords
                .cast()
                .component_mul(&self.resolution.spatial)
                .into();
            f(point_float, cell);
        }
    }

    pub fn add_source(&mut self, geometry: impl Rasterize, source: impl Source) {
        let index = self.sources.len();
        self.sources.push(Box::new(source));

        for point in geometry.rasterize(self) {
            let cell = point
                .ok()
                .and_then(|x| self.lattice.get_point_mut(&self.strider, &x))
                .unwrap_or_else(|| {
                    panic!("point outside lattice: {point:?}");
                });

            cell.source = Some(index);
        }
    }

    /// Returns field values along an axis-aligned line.
    pub(crate) fn field_values<'a, T, F>(
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

        let n = *axis.vector_component(&self.strider.size());
        let e = axis.basis().into_inner();
        let resolution = *axis.vector_component(&self.resolution.spatial);
        let swap_buffer_index = self.swap_buffer_index();

        (0..n).map(move |i| {
            let x = x0 + i * e;
            let cell = self.lattice.get_point(&self.strider, &x).unwrap();
            let value = f(&cell, swap_buffer_index);
            (i as f64 * resolution + x_correction, value)
        })
    }
}
