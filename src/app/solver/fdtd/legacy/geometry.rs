use either::Either;
use nalgebra::{
    Isometry3,
    Point,
    Point3,
    Vector3,
};

use crate::{
    app::solver::fdtd::legacy::Simulation,
    util::{
        InvalidPoint,
        round_to_grid,
    },
};

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Point3<f64>,
    pub max: Point3<f64>,
}

impl Aabb {
    pub fn from_point(point: Point3<f64>) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    pub fn union(&self, other: &Self) -> Self {
        Self {
            min: Point3::from(self.min.coords.zip_map(&other.min.coords, |a, b| a.min(b))),
            max: Point3::from(self.max.coords.zip_map(&other.max.coords, |a, b| a.max(b))),
        }
    }
}

pub trait Rasterize {
    fn rasterize<'a>(
        &'a self,
        simulation: &Simulation,
    ) -> impl Iterator<Item = Result<Point3<usize>, InvalidPoint>> + 'a;
}

pub trait BoundingBox {
    fn bounding_box(&self) -> Aabb;
}

impl Rasterize for Point3<f64> {
    fn rasterize<'a>(
        &'a self,
        simulation: &Simulation,
    ) -> impl Iterator<Item = Result<Point3<usize>, InvalidPoint>> + 'a {
        let x = round_to_grid(self, &Point3::origin(), &simulation.resolution().spatial);
        [x].into_iter()
    }
}

impl BoundingBox for Point3<f64> {
    fn bounding_box(&self) -> Aabb {
        Aabb::from_point(*self)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub transform: Isometry3<f64>,
    pub dimensions: Vector3<f64>,
}

impl Rasterize for Block {
    fn rasterize<'a>(
        &'a self,
        simulation: &Simulation,
    ) -> impl Iterator<Item = Result<Point3<usize>, InvalidPoint>> + 'a {
        // todo: rotation. currently this will returrn the whole aabb
        // fixme: this doesn't account for E and H fields being staggered

        let center = self.transform.transform_point(&Point::origin());

        let x0 = match round_to_grid(
            &(center - 0.5 * self.dimensions),
            &Point3::origin(),
            &simulation.resolution().spatial,
        ) {
            Ok(x) => x,
            Err(error) => return Either::Left([Err(error)].into_iter()),
        };
        let x1 = match round_to_grid(
            &(center + 0.5 * self.dimensions),
            &Point3::origin(),
            &simulation.resolution().spatial,
        ) {
            Ok(x) => x,
            Err(error) => return Either::Left([Err(error)].into_iter()),
        };

        Either::Right(
            simulation
                .strider()
                .iter(x0..=x1)
                .map(|(_index, point)| Ok(point)),
        )
    }
}

impl BoundingBox for Block {
    fn bounding_box(&self) -> Aabb {
        let x0 = self
            .transform
            .transform_point(&(Point3::origin() - Vector3::repeat(0.5)));
        let x1 = self
            .transform
            .transform_point(&(Point3::origin() + Vector3::repeat(0.5)));
        Aabb::from_point(x0).union(&Aabb::from_point(x1))
    }
}
