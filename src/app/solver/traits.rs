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
    fn update<S>(&self, state: &mut Self::State, sources: S)
    where
        S: IntoIterator<Item = (Self::Point, Self::Source)>;

    // note: `read/write_state` are way too general. rather make traits to
    // read/write field values, but we would need to know what exact usage pattern
    // we have for this (i.e.. how we project data out of the simulation domain).
    fn read_state<'a, R>(&'a self, state: &'a Self::State, reader: &'a R) -> R::Value<'a>
    where
        R: ReadState<Self>,
    {
        reader.read_state(self, state)
    }

    fn write_state<'a, W>(&'a self, state: &'a mut Self::State, writer: &'a W) -> W::Value<'a>
    where
        W: WriteState<Self>,
    {
        writer.write_state(self, state)
    }

    // todo: needs methods for converting from/to solver coordinates
}

pub trait ReadState<I>
where
    I: SolverInstance + ?Sized,
{
    type Value<'a>
    where
        Self: 'a,
        I: 'a;

    fn read_state<'a>(&'a self, instance: &'a I, state: &'a I::State) -> Self::Value<'a>;
}

pub trait WriteState<I>
where
    I: SolverInstance + ?Sized,
{
    type Value<'a>
    where
        Self: 'a,
        I: 'a;

    fn write_state<'a>(&'a self, instance: &'a I, state: &'a mut I::State) -> Self::Value<'a>;
}

pub trait Time {
    fn time(&self) -> f64;
    fn tick(&self) -> usize;
}
