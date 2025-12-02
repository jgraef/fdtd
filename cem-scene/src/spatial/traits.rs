use std::fmt::Debug;

use nalgebra::{
    Isometry3,
    Point3,
};
use parry3d::{
    bounding_volume::Aabb,
    query::Ray,
};

pub trait AnyCollider: ComputeAabb + RayCast + PointQuery + Debug + Send + Sync + 'static {}

impl<T> AnyCollider for T where T: ComputeAabb + RayCast + PointQuery + Debug + Send + Sync + 'static
{}

pub trait ComputeAabb {
    fn compute_aabb(&self, transform: &Isometry3<f32>) -> Aabb;
}

impl<T> ComputeAabb for T
where
    T: parry3d::shape::Shape,
{
    fn compute_aabb(&self, transform: &Isometry3<f32>) -> Aabb {
        parry3d::shape::Shape::compute_aabb(self, transform)
    }
}

pub trait RayCast {
    fn supported(&self) -> bool {
        true
    }

    fn cast_ray(
        &self,
        transform: &Isometry3<f32>,
        ray: &Ray,
        max_time_of_impact: f32,
        solid: bool,
    ) -> Option<f32>;
}

impl<T> RayCast for T
where
    T: parry3d::query::RayCast,
{
    fn cast_ray(
        &self,
        transform: &Isometry3<f32>,
        ray: &Ray,
        max_time_of_impact: f32,
        solid: bool,
    ) -> Option<f32> {
        parry3d::query::RayCast::cast_ray(self, transform, ray, max_time_of_impact, solid)
    }
}

pub trait PointQuery {
    fn supported(&self) -> bool {
        true
    }

    fn contains_point(&self, transform: &Isometry3<f32>, point: &Point3<f32>) -> bool;
}

impl<T> PointQuery for T
where
    T: parry3d::query::PointQuery,
{
    fn contains_point(&self, transform: &Isometry3<f32>, point: &Point3<f32>) -> bool {
        parry3d::query::PointQuery::contains_point(self, transform, point)
    }
}
