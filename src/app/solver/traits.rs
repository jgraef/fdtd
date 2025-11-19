use std::time::Duration;

use crate::{
    app::solver::config::{
        EvaluateStopCondition,
        StopCondition,
    },
    physics::material::MaterialDistribution,
};

pub trait Solver {
    type Config;
    type Point;
    type Instance: SolverInstance<Point = Self::Point>;
    type Error: std::error::Error;

    fn create_instance<M>(
        &self,
        config: &Self::Config,
        material: M,
    ) -> Result<Self::Instance, Self::Error>
    where
        M: MaterialDistribution<Self::Point>;

    fn memory_required(&self, config: &Self::Config) -> Option<usize> {
        let _ = config;
        None
    }

    /// Convenience method to create instance and state
    fn create_stateful_instance<M>(
        &self,
        config: &Self::Config,
        material: M,
    ) -> Result<StatefulInstance<Self::Instance>, Self::Error>
    where
        M: MaterialDistribution<Self::Point>,
    {
        let instance = self.create_instance(config, material)?;
        let state = instance.create_state();
        Ok(StatefulInstance { instance, state })
    }
}

pub trait SolverInstance {
    type State;
    type Point: 'static;

    fn create_state(&self) -> Self::State;
    fn update(&self, state: &mut Self::State);

    fn read_state<'a, R>(&'a self, state: &'a mut Self::State, reader: &'a R) -> R::Iter<'a>
    where
        R: ReadState<Self>,
    {
        reader.read_state(self, state)
    }

    // todo: needs methods for converting from/to solver coordinates
}

pub trait ReadState<I>
where
    I: SolverInstance + ?Sized,
{
    type Value: 'static;
    type Iter<'a>: Iterator<Item = (I::Point, &'a Self::Value)>
    where
        Self: 'a,
        I: 'a;

    fn read_state<'a>(&'a self, instance: &'a I, state: &'a I::State) -> Self::Iter<'a>;
}

#[derive(Clone, Copy, Debug)]
pub struct StatefulInstance<I>
where
    I: SolverInstance,
{
    pub instance: I,
    pub state: I::State,
}

impl<I> StatefulInstance<I>
where
    I: SolverInstance,
{
    pub fn update(&mut self) {
        self.instance.update(&mut self.state);
    }
}

impl<I> StatefulInstance<I>
where
    I: SolverInstance + EvaluateStopCondition,
{
    pub fn evaluate_stop_condition(
        &self,
        stop_condition: &StopCondition,
        time_elapsed: Duration,
    ) -> bool {
        self.instance
            .evaluate_stop_condition(&self.state, stop_condition, time_elapsed)
    }
}
