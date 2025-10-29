use nalgebra::{
    Isometry3,
    Point,
    Point3,
    Vector3,
};

use crate::simulation::Simulation;

pub trait Rasterize {
    fn rasterize(&self, simulation: &Simulation) -> impl Iterator<Item = Point3<usize>> + '_;
}

#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub transform: Isometry3<f64>,
    pub dimensions: Vector3<f64>,
}

impl Rasterize for Block {
    fn rasterize(&self, simulation: &Simulation) -> impl Iterator<Item = Point3<usize>> + '_ {
        // todo: rotation
        // fixme: this doesn't account for E and H fields being staggered

        let center = self.transform.transform_point(&Point::origin());

        let x0 = round_to_grid(simulation, &(center - 0.5 * self.dimensions));
        let x1 = round_to_grid(simulation, &(center + 0.5 * self.dimensions));

        iter_box_volume(x0, x1)
    }
}

impl Rasterize for Point3<f64> {
    fn rasterize(&self, simulation: &Simulation) -> impl Iterator<Item = Point3<usize>> + '_ {
        let x = round_to_grid(simulation, self);
        [x].into_iter()
    }
}

fn round_to_grid(simulation: &Simulation, x: &Point3<f64>) -> Point3<usize> {
    let dx = simulation.resolution().spatial;
    let x = (x.coords + simulation.origin()).component_div(&dx);
    x.map(|c| c.round() as usize).into()
}

fn iter_box_volume(x0: Point3<usize>, x1: Point3<usize>) -> impl Iterator<Item = Point3<usize>> {
    let y0 = x0.coords.zip_map(&x1.coords, |a, b| a.min(b));
    let y1 = x0.coords.zip_map(&x1.coords, |a, b| a.max(b));

    (y0.x..=y1.x).flat_map(move |x| {
        (y0.y..=y1.y).flat_map(move |y| (y0.z..=y1.z).map(move |z| Point3::new(x, y, z)))
    })
}
