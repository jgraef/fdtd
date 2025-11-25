pub mod config;
pub mod fdtd;
pub mod observer;
pub mod project;
pub mod runner;
pub mod source;
pub mod ui;

use std::{
    fmt::Debug,
    ops::RangeBounds,
};

use nalgebra::Vector3;

use crate::{
    app::solver::source::SourceValues,
    physics::material::Material,
};

/// TODO: Reconcile the use of a config and domain description. Should they be
/// the same thing? E.g. our FDTD implementation takes domain-specific
/// parameters like size and spatial resolution in the config, but they belong
/// more into the domain description.
pub trait SolverBackend<Config, Point> {
    type Instance: SolverInstance;

    type Error: std::error::Error;

    fn create_instance<D>(
        &self,
        config: &Config,
        domain_description: D,
    ) -> Result<Self::Instance, Self::Error>
    where
        D: DomainDescription<Point>;

    fn memory_required(&self, config: &Config) -> Option<usize> {
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

/// todo: needs methods for converting from/to solver coordinates
pub trait SolverInstance {
    type State;
    type UpdatePass<'a>: UpdatePass
    where
        Self: 'a;

    fn create_state(&self) -> Self::State;

    fn begin_update<'a>(&'a self, state: &'a mut Self::State) -> Self::UpdatePass<'a>;
}

pub trait UpdatePass
where
    Self: Sized,
{
    fn finish(self);
}

pub trait UpdatePassForcing<Point>: UpdatePass {
    fn set_forcing(&mut self, point: &Point, value: &SourceValues);
}

pub trait Time {
    fn time(&self) -> f64;
    fn tick(&self) -> usize;
}

pub trait Field<Point>: SolverInstance {
    type View<'a>: FieldView<Point>
    where
        Self: 'a;

    fn field<'a, R>(
        &'a self,
        state: &'a Self::State,
        range: R,
        field_component: FieldComponent,
    ) -> Self::View<'a>
    where
        R: RangeBounds<Point>;
}

// todo: remove. this is not good. we can't always guarantuee that we can hand
// out `&mut Vector3<f64>`s
pub trait FieldMut<Point>: SolverInstance {
    type IterMut<'a>: Iterator<Item = (Point, &'a mut Vector3<f64>)>
    where
        Self: 'a;

    fn field_mut<'a, R>(
        &'a self,
        state: &'a mut Self::State,
        range: R,
        field_component: FieldComponent,
    ) -> Self::IterMut<'a>
    where
        R: RangeBounds<Point>;
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
