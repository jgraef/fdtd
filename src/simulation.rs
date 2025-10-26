use std::fmt::Debug;

use nalgebra::{
    Point3,
    Vector3,
};

use crate::grid::Grid;

#[derive(Clone, Debug)]
pub struct Simulation {
    dimensions: Vector3<usize>,

    physical_constants: PhysicalConstants,
    resolution: Resolution,

    time: usize,
    electric_grid: Grid<ElectricCell>,
    magnetic_grid: Grid<MagneticCell>,
}

#[derive(Clone, Debug)]
struct ElectricCell {
    relative_permittivity: f64,
    eletrical_conductivity: f64,
    electric_field: Vector3<f64>,
}

impl Default for ElectricCell {
    fn default() -> Self {
        Self {
            relative_permittivity: 1.0,
            eletrical_conductivity: 0.0,
            electric_field: Vector3::zeros(),
        }
    }
}

#[derive(Clone, Debug)]
struct MagneticCell {
    relative_permeability: f64,
    magnetic_field: Vector3<f64>,
}

impl Default for MagneticCell {
    fn default() -> Self {
        Self {
            relative_permeability: 1.0,
            magnetic_field: Vector3::zeros(),
        }
    }
}

impl Simulation {
    pub fn new(
        dimensions: Vector3<usize>,
        physical_constants: PhysicalConstants,
        resolution: Resolution,
    ) -> Self {
        let even_grid = Grid::new(dimensions, |_| Default::default());
        let odd_grid = Grid::new(dimensions, |_| Default::default());

        Self {
            dimensions,
            physical_constants,
            resolution,
            time: 0,
            electric_grid: even_grid,
            magnetic_grid: odd_grid,
        }
    }

    pub fn step(&mut self) {
        // update magnetic field
        for point in iter_coords(&self.dimensions) {
            let e_curl = curl(
                point,
                Vector3::repeat(1),
                &self.electric_grid,
                |cell| cell.electric_field,
                self.resolution.spatial,
            )
            .unwrap_or_default();
            let cell = self.magnetic_grid.get_mut(&point).unwrap();
            let permeability =
                cell.relative_permeability * self.physical_constants.vacuum_permeability;
            cell.magnetic_field -= self.resolution.temporal * 1.0 / permeability * e_curl;
        }

        // update electric field
        for point in iter_coords(&self.dimensions) {
            let h_curl = curl(
                point,
                Vector3::zeros(),
                &self.magnetic_grid,
                |cell| cell.magnetic_field,
                self.resolution.spatial,
            )
            .unwrap_or_default();
            let e_approx = interpolate(point, &self.electric_grid, |cell| cell.electric_field);
            let cell = self.electric_grid.get_mut(&point).unwrap();
            let permittivity =
                cell.relative_permittivity * self.physical_constants.vacuum_permittivity;
            cell.electric_field += self.resolution.temporal
                * (h_curl / permittivity - cell.eletrical_conductivity / permittivity * e_approx);
        }

        self.time += 1;
    }

    pub fn time(&self) -> f64 {
        self.time as f64 * self.resolution.temporal
    }

    pub fn e_field(&self) -> Vec<[f64; 2]> {
        let mut data = Vec::with_capacity(self.dimensions.x);

        for x in 0..self.dimensions.x {
            let e_cell = self.electric_grid.get(&Point3::new(x, 0, 0)).unwrap();
            data.push([x as f64 * self.resolution.spatial, e_cell.electric_field.y]);
        }

        data
    }
}

fn iter_coords(dimensions: &Vector3<usize>) -> impl Iterator<Item = Point3<usize>> {
    (0..dimensions.x).flat_map(move |x| {
        (0..dimensions.y).flat_map(move |y| (0..dimensions.z).map(move |z| Point3::new(x, y, z)))
    })
}

fn curl<T>(
    x: Point3<usize>,
    dx: Vector3<usize>,
    grid: &Grid<T>,
    field: impl Fn(&T) -> Vector3<f64>,
    spatial_resolution: f64,
) -> Option<Vector3<f64>> {
    let x = Point3::new(
        x.x.checked_sub(dx.x)?,
        x.y.checked_sub(dx.y)?,
        x.z.checked_sub(dx.z)?,
    );

    let df = |dx| {
        let x1 = field(grid.get(&x)?);
        let x2 = field(grid.get(&(x + dx))?);
        Some(x2 - x1)
    };
    let dfdx = df(Vector3::x()).unwrap_or_default();
    let dfdy = df(Vector3::y()).unwrap_or_default();
    let dfdz = df(Vector3::z()).unwrap_or_default();

    Some(Vector3::new(dfdy.z - dfdz.y, dfdz.x - dfdx.z, dfdx.y - dfdy.x) / spatial_resolution)
}

fn interpolate<T>(
    x: Point3<usize>,
    grid: &Grid<T>,
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

    pub fn speed_of_light(&self) -> f64 {
        (self.vacuum_permittivity * self.vacuum_permeability).powf(-0.5)
    }

    pub fn frequency_to_wavelength(&self, frequency: f64) -> f64 {
        self.speed_of_light() / frequency
    }

    pub fn wavelength_to_frequency(&self, wavelength: f64) -> f64 {
        self.speed_of_light() / wavelength
    }

    pub fn estimate_temporal_from_spatial_resolution(self, spatial_resolution: f64) -> f64 {
        spatial_resolution / (self.speed_of_light() * 3.0f64.sqrt())
    }

    pub fn estimate_spatial_from_temporal_resolution(&self, temporal_resolution: f64) -> f64 {
        temporal_resolution * self.speed_of_light() * 3.0f64.sqrt()
    }

    pub fn estimate_spatial_resolution_from_min_wavelength(&self, min_wavelength: f64) -> f64 {
        min_wavelength / (9.0f64 * 3.0f64.sqrt())
    }

    pub fn estimate_temporal_resolution_from_max_frequency(&self, max_frequency: f64) -> f64 {
        1.0f64 / (9.0f64 * 3.0f64 * max_frequency)
    }

    pub fn estimate_resolution_from_min_wavelength(&self, min_wavelength: f64) -> Resolution {
        let spatial = self.estimate_spatial_resolution_from_min_wavelength(min_wavelength);
        let temporal = self.estimate_temporal_from_spatial_resolution(spatial);
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
    pub spatial: f64,
    pub temporal: f64,
}
