use crate::physics::material::MaterialDistribution;

pub trait Solver {
    type Config;
    type Point;
    type Instance: SolverInstance<Point = Self::Point>;
    type Error;

    fn create_instance<M>(
        &self,
        config: &Self::Config,
        material: M,
    ) -> Result<Self::Instance, Self::Error>
    where
        M: MaterialDistribution<Self::Point>;

    fn memory_usage_estimate(&self, config: &Self::Config) -> Option<usize> {
        let _ = config;
        None
    }
}

pub trait SolverInstance {
    type State;
    type Point;

    fn create_state(&self) -> Self::State;
    fn update(&self, state: &mut Self::State);

    // todo: accessors for field values
    // todo: needs methods for converting from/to solver coordinates
}
