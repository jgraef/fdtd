use std::time::Instant;

use nalgebra::Vector3;

use crate::{
    app::{
        composer::{
            renderer::WgpuContext,
            scene::Scene,
        },
        solver::config::{
            SolverConfig,
            SolverConfigCommon,
            SolverConfigFdtd,
            SolverConfigSpecifics,
        },
    },
    fdtd,
    physics::material::Material,
    util::format_size,
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
            SolverConfigSpecifics::Fdtd(fdtd_config) => {
                self.run_fdtd(scene, &solver_config.common, fdtd_config);
            }
            SolverConfigSpecifics::Feec(_feec_config) => tracing::debug!("todo: feec solver"),
        }
    }

    fn run_fdtd(
        &mut self,
        scene: &Scene,
        common_config: &SolverConfigCommon,
        fdtd_config: &SolverConfigFdtd,
    ) {
        let aabb = common_config.volume.aabb(scene);
        let _rotation = common_config.volume.rotation(); // ignored for now

        let origin = aabb.mins;
        let size = aabb.extents();

        // todo: make fdtd generic over float, remove casts
        let config = fdtd::SimulationConfig {
            resolution: fdtd_config.resolution,
            physical_constants: common_config.physical_constants,
            origin: Some(origin.cast()),
            size: size.cast(),
        };

        let memory_required = config.memory_usage_estimate();
        tracing::debug!(?origin, ?size, memory_required = %format_size(memory_required), lattice_size = ?config.lattice_size(), "creating fdtd simulation");

        // todo: remove this. we want a ui flow that prepares the solver-run anyway, so
        // we could display and warn about memory requirements there.
        // for now this is just a safe-guard that I don't crash my system xD
        if memory_required > 100_000_000 {
            tracing::warn!("abort. too much memory required");
            return;
        }

        let time_start = Instant::now();

        let mut simulation = fdtd::Simulation::new(&config);

        // access to the material properties
        //
        // todo: move this out of the fdtd module
        let entity_materials = scene.entities.view::<&Material>();

        // todo: would be nice if we could do the rasterization in a thread, since this
        // might block the UI for a moment. maybe we can used a scoped thread?
        //
        // otherwise we might want to copy the relevant data into a buffer:
        //  - entities with: Transform, SharedShape, fdtd::Material
        //  - bvh
        // we could then also crop/transform by selected volume at this step.
        simulation.fill_with(|point, cell| {
            let point = point.cast::<f32>();

            // this produces an iterator of materials present at this point
            //
            // todo: ideally we would use a contact query here (against the whole cell
            // cuboid) and use the contact information to smooth edges
            let mut point_materials = scene
                .point_query(&point)
                .filter_map(|entity| entity_materials.get(entity))
                .copied();

            // for now we'll just use the first material we find.
            // if nothing is found, use the default (vacuum)
            let material = point_materials.next().unwrap_or_default();

            cell.set_material(material)
        });

        {
            // for testing we'll add a source at the origin
            // todo: remove this

            let center = aabb.center();
            simulation.add_source(
                center.cast::<f64>(),
                fdtd::source::GaussianPulse {
                    electric_current_density_amplitude: Vector3::y(),
                    magnetic_current_density_amplitude: Vector3::zeros(),
                    time: 20.0,
                    duration: 10.0,
                },
            );
        }

        tracing::debug!("time to create simulation: {:?}", time_start.elapsed());

        let _join_handle = std::thread::spawn(move || {
            // todo
        });

        //
        //todo!();
    }
}
