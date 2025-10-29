use std::fmt::Debug;

use nalgebra::{
    Point3,
    Vector3,
};

use crate::{
    geometry::Rasterize,
    lattice::Lattice,
    material::Material,
    source::Source,
};

#[derive(derive_more::Debug)]
pub struct Simulation {
    resolution: Resolution,
    physical_constants: PhysicalConstants,

    tick: usize,
    time: f64,
    origin: Vector3<f64>,
    lattice: Lattice<Cell>,
    total_energy: f64,

    #[debug(ignore)]
    sources: Vec<Box<dyn Source>>,
}

#[derive(Clone, Debug)]
pub struct Cell {
    /// Material properties
    pub material: Material,
    /// H
    pub magnetic_field: [Vector3<f64>; 2],
    /// E
    pub electric_field: [Vector3<f64>; 2],
    /// index into `Simulation::source`, defining magnetic and electric current
    /// density functions
    source: Option<usize>,
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
            magnetic_field: [Vector3::zeros(); 2],
            electric_field: [Vector3::zeros(); 2],
            source: None,
        }
    }

    pub fn reset(&mut self) {
        self.magnetic_field = [Vector3::zeros(); 2];
        self.electric_field = [Vector3::zeros(); 2];
    }
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

        Self {
            physical_constants,
            resolution,
            tick: 0,
            time: 0.0,
            origin,
            lattice,
            total_energy: 0.0,
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
        let current = (self.tick + 1) % 2;
        let previous = self.tick % 2;

        let mut energy = 0.0;

        // prepare sources
        // todo: we might need to pass some info to prepare so it knows what time is for
        // the magnetic and electric field
        for source in &mut self.sources {
            source.prepare(self.time);
        }

        // update magnetic field
        for point in iter_coords(&self.lattice.dimensions()) {
            let e_curl = curl(
                point,
                Vector3::repeat(1),
                Vector3::zeros(),
                &self.lattice,
                |cell| cell.electric_field[previous],
                &self.resolution.spatial,
            );

            let h_approx = Vector3::zeros();

            let cell = self.lattice.get_mut(&point).unwrap();

            let permeability =
                cell.material.relative_permeability * self.physical_constants.vacuum_permeability;

            let m_source = if let Some(index) = cell.source {
                let j_source = &mut self.sources[index];
                let point_dx = Point3::from(
                    point
                        .map(|x| x as f64)
                        .coords
                        .component_mul(&self.resolution.spatial),
                );
                j_source.magnetic_current_density(self.time, &point_dx)
            }
            else {
                Vector3::zeros()
            };

            cell.magnetic_field[current] = cell.magnetic_field[previous]
                - self.resolution.temporal / permeability
                    * (e_curl + m_source + cell.material.magnetic_conductivity * h_approx);

            /*if point.x == 10 && point.y == 0 && point.z == 0 {
                cell.magnetic_field[current] =
                    Vector3::y() * (-((self.time - 20.0) * 0.1).powi(2)).exp();
            }*/

            energy += cell.magnetic_field[current].norm_squared() / permeability;
        }

        // update electric field
        let time = self.time + 0.5 * self.resolution.temporal;
        for point in iter_coords(&self.lattice.dimensions()) {
            let h_curl = curl(
                point,
                Vector3::zeros(),
                Vector3::repeat(1),
                &self.lattice,
                |cell| {
                    // NOTE: this is `current` not `previous`, because we have already updated the H
                    // field with the new values in `current`.
                    cell.magnetic_field[current]
                },
                &self.resolution.spatial,
            );

            //let e_approx = interpolate(point, &self.electric_grid, |cell|
            // cell.electric_field_prev);
            let e_approx = Vector3::zeros();

            let cell = self.lattice.get_mut(&point).unwrap();

            let permittivity =
                cell.material.relative_permittivity * self.physical_constants.vacuum_permittivity;

            let j_source = if let Some(index) = cell.source {
                // todo: use time instead of self.time and add 0.5*dx offset to coordinates
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

            cell.electric_field[current] = cell.electric_field[previous]
                + self.resolution.temporal / permittivity
                    * (h_curl - j_source - cell.material.eletrical_conductivity * e_approx);

            energy += permittivity * cell.electric_field[current].norm_squared();
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

    pub fn physical_constants(&self) -> &PhysicalConstants {
        &self.physical_constants
    }

    pub fn resolution(&self) -> &Resolution {
        &self.resolution
    }

    pub(crate) fn origin(&self) -> &Vector3<f64> {
        &self.origin
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

    pub fn e_field(&self) -> Vec<[f64; 2]> {
        let current = self.tick % 2;
        let mut data = Vec::with_capacity(self.lattice.dimensions().x);

        for x in 0..self.lattice.dimensions().x {
            let e_cell = self.lattice.get(&Point3::new(x, 0, 0)).unwrap();
            data.push([
                (x as f64 + 0.5) / self.resolution.spatial.x - self.origin.x,
                e_cell.electric_field[current].y,
            ]);
        }

        data
    }

    pub fn h_field(&self) -> Vec<[f64; 2]> {
        let current = self.tick % 2;
        let mut data = Vec::with_capacity(self.lattice.dimensions().x);

        for x in 0..self.lattice.dimensions().x {
            let h_cell = self.lattice.get(&Point3::new(x, 0, 0)).unwrap();
            data.push([
                (x as f64) / self.resolution.spatial.x - self.origin.x,
                h_cell.magnetic_field[current].z,
            ]);
        }

        data
    }

    pub fn epsilon(&self) -> Vec<[f64; 2]> {
        let mut data = Vec::with_capacity(self.lattice.dimensions().x);

        for x in 0..self.lattice.dimensions().x {
            let e_cell = self.lattice.get(&Point3::new(x, 0, 0)).unwrap();
            data.push([
                (x as f64 + 0.5) / self.resolution.spatial.x - self.origin.x,
                e_cell.material.relative_permittivity,
            ]);
        }

        data
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
}

fn iter_coords(dimensions: &Vector3<usize>) -> impl Iterator<Item = Point3<usize>> {
    (0..dimensions.x).flat_map(move |x| {
        (0..dimensions.y).flat_map(move |y| (0..dimensions.z).map(move |z| Point3::new(x, y, z)))
    })
}

fn curl<T>(
    x: Point3<usize>,
    dx0: Vector3<usize>,
    dx1: Vector3<usize>,
    grid: &Lattice<T>,
    field: impl Fn(&T) -> Vector3<f64>,
    spatial_resolution: &Vector3<f64>,
) -> Vector3<f64> {
    let df = |e, d0, d1, dx| {
        if x.coords.dot(&e) >= d0 {
            let cell0 = grid.get(&(x - e * d0));
            let cell1 = grid.get(&(x + e * d1));
            if let (Some(cell0), Some(cell1)) = (cell0, cell1) {
                let f0 = field(cell0);
                let f1 = field(cell1);
                (f1 - f0) / dx
            }
            else {
                Default::default()
            }
        }
        else {
            Default::default()
        }
    };

    let dfdx = df(Vector3::x(), dx0.x, dx1.x, spatial_resolution.x);
    let dfdy = df(Vector3::y(), dx0.y, dx1.y, spatial_resolution.y);
    let dfdz = df(Vector3::z(), dx0.z, dx1.z, spatial_resolution.z);

    Vector3::new(dfdy.z - dfdz.y, dfdz.x - dfdx.z, dfdx.y - dfdy.x)
}

fn interpolate<T>(
    x: Point3<usize>,
    grid: &Lattice<T>,
    field: impl Fn(&T) -> Vector3<f64>,
) -> Vector3<f64> {
    let mut n = 0;
    let mut s = Vector3::default();
    let mut fold = |p| {
        if let Some(cell) = grid.get(&p) {
            s += field(cell);
            n += 1;
        }
    };
    let mut fold_pair = |x, dx| {
        fold(x);
        fold(x + dx);
    };

    fold_pair(x, Vector3::x());
    fold_pair(x, Vector3::y());
    fold_pair(x, Vector3::z());

    s / (n as f64)
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
