use crate::{
    app::{
        composer::{
            renderer::WgpuContext,
            scene::Scene,
        },
        solver::config::{
            SolverConfig,
            SolverConfigSpecifics,
            Volume,
        },
    },
    fdtd::{
        self,
        PhysicalConstants,
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
        match &solver_config.specifics {
            SolverConfigSpecifics::Fdtd { resolution } => {
                self.run_fdtd(
                    scene,
                    resolution,
                    &solver_config.volume,
                    &solver_config.physical_constants,
                );
            }
            SolverConfigSpecifics::Feec {} => tracing::debug!("todo: feec solver"),
        }
    }

    fn run_fdtd(
        &mut self,
        scene: &Scene,
        resolution: &fdtd::Resolution,
        volume: &Volume,
        physical_constants: &PhysicalConstants,
    ) {
        let _ = (scene, resolution, volume, physical_constants);
        //let mut simulation = fdtd::Simulation::new(size, *physical_constants,
        // *resolution);
        todo!();
    }
}
