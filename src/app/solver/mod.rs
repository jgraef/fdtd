pub mod config;
pub mod fdtd;
pub mod observer;
pub mod runner;
pub mod ui;
pub mod util;

use std::ops::RangeBounds;

use nalgebra::Vector3;

use crate::physics::material::Material;

pub trait SolverBackend {
    // todo: this should be a type parameter on the trait, no?
    type Config;

    type Point;

    type Instance: SolverInstance<Point = Self::Point>;

    type Error: std::error::Error;

    fn create_instance<D>(
        &self,
        config: &Self::Config,
        domain_description: D,
    ) -> Result<Self::Instance, Self::Error>
    where
        D: DomainDescription<Self::Point>;

    fn memory_required(&self, config: &Self::Config) -> Option<usize> {
        let _ = config;
        None
    }
}

// note: this was originally called `MaterialDistribution`, and could well be
// still. But we might need to add other things that are not directly related to
// the material, so we'll keep it named this. If it turns out we only need the
// material properties, we can rename it again.
//
// we could also let create_instance return a builder object, which then has
// methods on it:
//  - fill_with(&mut self, impl FnMut(&P) -> Material) // similar to this trait
//  - set(&mut self, point: &P, material: P) // set individual cells
// this could also provide a good point to query required memory (although that
// improvement it minimal). and in the future we could add other configuration
// options. i think probably only the domain size needs to be known a priori to
// allocate buffers.
pub trait DomainDescription<P> {
    fn material(&self, point: &P) -> Material;
}

pub trait SolverInstance {
    type State;
    type Point: 'static;
    type Source;

    fn create_state(&self) -> Self::State;

    // todo: split this into an UpdatePass type?
    fn update<S>(&self, state: &mut Self::State, sources: S)
    where
        S: IntoIterator<Item = (Self::Point, Self::Source)>;

    // todo: needs methods for converting from/to solver coordinates
}

pub trait Time {
    fn time(&self) -> f64;
    fn tick(&self) -> usize;
}

pub trait Field: SolverInstance {
    type View<'a>: FieldView<Self::Point>
    where
        Self: 'a;

    fn field<'a, R>(
        &'a self,
        state: &'a Self::State,
        range: R,
        field_component: FieldComponent,
    ) -> Self::View<'a>
    where
        R: RangeBounds<Self::Point>;
}

// todo: remove. this is not good. we can't always guarantuee that we can hand
// out `&mut Vector3<f64>`s
pub trait FieldMut: SolverInstance {
    type IterMut<'a>: Iterator<Item = (Self::Point, &'a mut Vector3<f64>)>
    where
        Self: 'a;

    fn field_mut<'a, R>(
        &'a self,
        state: &'a mut Self::State,
        range: R,
        field_component: FieldComponent,
    ) -> Self::IterMut<'a>
    where
        R: RangeBounds<Self::Point>;
}

pub trait FieldView<P> {
    type Iter<'a>: Iterator<Item = (P, Vector3<f64>)>
    where
        Self: 'a;

    fn at(&self, point: &P) -> Option<Vector3<f64>>;
    fn iter<'a>(&'a self) -> Self::Iter<'a>;
}

#[derive(Clone, Copy, Debug)]
pub enum FieldComponent {
    E,
    H,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SourceValues {
    pub j_source: Vector3<f64>,
    pub m_source: Vector3<f64>,
}
