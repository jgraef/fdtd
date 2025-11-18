pub mod geometry;
pub mod pml;
pub mod simulation;
pub mod source;

pub use simulation::Simulation;

pub fn estimate_memory_usage(&config: &crate::app::solver::fdtd::FdtdSolverConfig) -> usize {
    std::mem::size_of::<simulation::Cell>() * config.num_cells()
}
