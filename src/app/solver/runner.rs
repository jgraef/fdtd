use crate::app::{
    composer::{
        renderer::WgpuContext,
        scene::Scene,
    },
    solver::config::{
        SolverConfig,
        SolverConfigSpecifics,
    },
};

#[derive(Debug)]
pub struct SolverRunner {
    /// for when we eventually implement solvers that run on the GPU
    wgpu_context: WgpuContext,
}

impl SolverRunner {
    pub fn new(wgpu_context: &WgpuContext) -> Self {
        Self {
            wgpu_context: wgpu_context.clone(),
        }
    }

    /// TODO: We probably just want one parameter that impls some trait. That
    /// trait defines how a solver_config and scene is turned into the problem
    /// description for the runner (e.g. a `fdtd::Simulation`).
    pub fn run(&mut self, solver_config: &SolverConfig, scene: &Scene) {
        match solver_config.specifics {
            SolverConfigSpecifics::Fdtd { resolution } => {
                //self.run_fdtd(scene, resolution, physical_constants)
                let _ = (scene, resolution);
                todo!();
            }
            SolverConfigSpecifics::Feec {} => tracing::debug!("todo: feec solver"),
        }
    }
}
