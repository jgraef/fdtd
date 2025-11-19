use std::time::{
    Duration,
    Instant,
};

use colorgrad::Gradient;
use nalgebra::Point3;

use crate::{
    app::{
        composer::{
            renderer::WgpuContext,
            scene::Scene,
        },
        solver::{
            config::{
                EvaluateStopCondition,
                Parallelization,
                SolverConfig,
                SolverConfigCommon,
                SolverConfigFdtd,
                SolverConfigSpecifics,
                StopCondition,
            },
            fdtd::{
                self,
                FdtdSolverConfig,
                cpu::FdtdCpuSolver,
                wgpu::FdtdWgpuSolver,
            },
            traits::{
                Solver,
                SolverInstance,
            },
        },
    },
    physics::material::{
        Material,
        MaterialDistribution,
    },
    util::format_size,
};

#[derive(Debug)]
pub struct SolverRunner {
    fdtd_wgpu: FdtdWgpuSolver,
}

impl SolverRunner {
    pub fn new(wgpu_context: &WgpuContext) -> Self {
        Self {
            fdtd_wgpu: FdtdWgpuSolver::new(&wgpu_context.device, &wgpu_context.queue),
        }
    }

    /// TODO: We probably just want one parameter that impls some trait. That
    /// trait defines how a solver_config and scene is turned into the problem
    /// description for the runner (e.g. a `fdtd::Simulation`).
    pub fn run(&mut self, solver_config: &SolverConfig, scene: &mut Scene) {
        match &solver_config.specifics {
            SolverConfigSpecifics::Fdtd(fdtd_config) => {
                self.run_fdtd(scene, &solver_config.common, fdtd_config);
            }
            SolverConfigSpecifics::Feec(_feec_config) => tracing::debug!("todo: feec solver"),
        }
    }

    fn run_fdtd(
        &self,
        scene: &mut Scene,
        common_config: &SolverConfigCommon,
        fdtd_config: &SolverConfigFdtd,
    ) {
        match &common_config.parallelization {
            None => run_fdtd_with_solver(scene, common_config, fdtd_config, &FdtdCpuSolver),
            Some(Parallelization::MultiThreaded { num_threads: _ }) => {
                /*run_fdtd_with_solver(
                    scene,
                    common_config,
                    fdtd_config,
                    &FdtdCpuSolver::multi_threaded(),
                )*/
                todo!();
            }
            Some(Parallelization::Wgpu) => {
                run_fdtd_with_solver(scene, common_config, fdtd_config, &self.fdtd_wgpu)
            }
        }
    }
}

fn run_fdtd_with_solver<S>(
    scene: &mut Scene,
    common_config: &SolverConfigCommon,
    fdtd_config: &SolverConfigFdtd,
    solver: &S,
) where
    S: Solver<Config = FdtdSolverConfig, Point = Point3<usize>>,
    S::Instance: EvaluateStopCondition + Send + 'static,
    <S::Instance as SolverInstance>::State: Send + 'static,
{
    let time_start = Instant::now();

    let aabb = common_config.volume.aabb(scene);
    let _rotation = common_config.volume.rotation(); // ignored for now

    let _origin = aabb.mins;
    let mut size = aabb.extents();

    // only a 2d plane for now
    size.z = 0.0;

    let mut config = FdtdSolverConfig {
        resolution: fdtd_config.resolution,
        physical_constants: common_config.physical_constants,
        size: size.cast(),
    };

    // overwriting temporal resolution to satisfy courant condition
    // todo: whether the courant condition is satisfied should be checked by the
    // solver config ui.
    config.resolution.temporal = 0.2
        * fdtd::estimate_temporal_from_spatial_resolution(
            common_config.physical_constants.speed_of_light(),
            &config.resolution.spatial,
        );

    // good config for debugging
    /*let config = fdtd::SimulationConfig {
        resolution: fdtd::Resolution {
            spatial: Vector3::repeat(1.0),
            temporal: 0.25,
        },
        physical_constants: PhysicalConstants::REDUCED,
        origin: None,
        size: Vector3::new(100.0, 100.0, 0.0),
    };*/

    let memory_required = solver
        .memory_required(&config)
        .expect("fdtd always returns memory required");
    tracing::debug!(
        ?size,
        resolution = ?config.resolution,
        memory_required = %format_size(memory_required),
        lattice_size = ?config.size(),
        "creating fdtd simulation"
    );

    // todo: remove this. we want a ui flow that prepares the solver-run anyway, so
    // we could display and warn about memory requirements there.
    // for now this is just a safe-guard that I don't crash my system xD
    if memory_required > 200_000_000 {
        tracing::warn!("abort. too much memory required");
        return;
    }

    let materials = SceneMaterials::new(scene);

    let instance = solver
        .create_instance(&config, materials)
        .expect("fdtd solver instance creation never fails");

    let state = instance.create_state();

    // todo: setup sources and observers

    tracing::debug!("time to create simulation: {:?}", time_start.elapsed());

    // run simulation
    spawn_solver(instance, state, fdtd_config.stop_condition);
}

fn spawn_solver<I>(instance: I, mut state: I::State, stop_condition: StopCondition)
where
    I: SolverInstance + EvaluateStopCondition + Send + 'static,
    I::State: Send + 'static,
{
    let _join_handle = std::thread::spawn(move || {
        let time_start = Instant::now();

        loop {
            let time_elapsed = time_start.elapsed();

            if instance.evaluate_stop_condition(&mut state, &stop_condition, time_elapsed) {
                tracing::debug!("stop condition reached");
                break;
            }

            //tracing::debug!(tick = simulation.tick(), elapsed = ?time_elapsed);

            instance.update(&mut state);

            //run_observers(&simulation);
            std::thread::sleep(Duration::from_millis(10));
        }
    });
}

struct SceneMaterials<'a, 'b> {
    scene: &'a Scene,
    materials: hecs::ViewBorrow<'a, &'b Material>,
}

impl<'a, 'b> SceneMaterials<'a, 'b> {
    pub fn new(scene: &'a Scene) -> Self {
        // access to the material properties
        let materials = scene.entities.view::<&Material>();

        Self { scene, materials }
    }
}

impl<'a, 'b> MaterialDistribution<Point3<usize>> for SceneMaterials<'a, 'b> {
    fn at(&self, point: &Point3<usize>) -> Material {
        // todo: map back to proper world coordinates
        let point = point.cast::<f32>();

        let mut point_materials = self
            .scene
            .point_query(&point)
            .filter_map(|entity| self.materials.get(entity))
            .copied();

        // for now we'll just use the first material we find.
        // if nothing is found, use the default (vacuum)
        point_materials.next().unwrap_or_default()
    }
}

/*
fn run_fdtd_old(
    scene: &mut Scene,
    common_config: &SolverConfigCommon,
    fdtd_config: &SolverConfigFdtd,
) {
    {
        // for testing we'll add a source at the origin
        // todo: remove this

        let mut source_position = aabb.center();
        source_position.z = origin.z;
        //let source_position = Point3::new(-40.0, -40.0, 0.0);

        /*let source = fdtd::source::ContinousWave {
            electric_current_density_amplitude: Vector3::z() / config.resolution.temporal,
            magnetic_current_density_amplitude: Vector3::zeros(),
            electric_current_density_phase: 0.0,
            magnetic_current_density_phase: 0.0,
            frequency: 2.0,
        };*/
        let source = fdtd::legacy::source::GaussianPulse {
            electric_current_density_amplitude: Vector3::z() / config.resolution.temporal,
            magnetic_current_density_amplitude: Vector3::zeros(),
            time: config.resolution.temporal * 50.0,
            duration: config.resolution.temporal * 10.0,
        };

        tracing::debug!(position = ?source_position, ?source, "source");

        simulation.add_source(source_position.cast::<f64>(), source);
    }

    let mut run_observers = {
        let lattice_size = *simulation.strider().size();

        // create an "observer". later we want this to be defined by the user and just
        // attach our TextureOutput to it. We need to create the texture output since
        // only we know the texture size. write to wgpu texture

        // for now we'll grab just one, since we can't define what to render anyway

        let mut observers = vec![];

        for (entity, observer) in scene.entities.query_mut::<&Observer>() {
            // todo: use observer extents

            if observer.display_as_texture {
                // todo: check if this already exists
                let texture_output = TextureWriter::new(lattice_size.xy().cast());
                scene
                    .command_buffer
                    .insert_one(entity, texture_output.clone());
                observers.push(texture_output);
            }

            if let Some(path) = &observer.write_to_gif {
                let create_gif_output = || {
                    Ok::<_, Error>(GifOutput::new(
                        BufWriter::new(File::create(path)?),
                        lattice_size.xy().cast(),
                        Duration::from_millis(10),
                    )?)
                };

                match create_gif_output() {
                    Ok(_output) => {
                        // todo: add to observers. will need `Box<dyn _>`
                    }
                    Err(error) => {
                        tracing::error!(path = %path.display(), "failed to create GIF output: {}", error);
                    }
                }
            }

            // for now only one
            break;
        }

        // apply deferred commands
        scene.apply_deferred();

        let gradient = TestGradient;

        // wrap image output into closure that takes the field values from the
        // simulation and writes a frame with it everytime it's called
        move |simulation: &fdtd::legacy::Simulation| {
            let swap_buffer_index = simulation.swap_buffer_index();
            let z = lattice_size.z / 2;

            // only one for now
            let Some(image_output) = observers.first_mut()
            else {
                return;
            };

            image_output
                .write_colors(|point| {
                    let point = point.cast();
                    let cell = simulation
                        .lattice()
                        .get_point(simulation.strider(), &Point3::new(point.x, point.y, z))
                        .unwrap();

                    let e_field = cell.electric_field(swap_buffer_index);
                    //(0.5 + 0.5 * e_field.y).clamp(0.0, 1.0) as f32
                    let e_field = e_field.z.clamp(-1.0, 1.0) as f32;
                    let eps = cell.material().relative_permittivity as f32;

                    let mut color: Srgba = gradient.at(e_field).to_array().into();
                    color.green = (eps / 20.0).clamp(0.0, 1.0);

                    color
                })
                .unwrap();
        }
    };
}
     */

#[derive(Clone, Debug)]
struct TestGradient;

impl Gradient for TestGradient {
    fn at(&self, t: f32) -> colorgrad::Color {
        let mut red = 0.0;
        let mut blue = 0.0;
        if t > 0.0 {
            red = t.min(1.0);
        }
        else {
            blue = (-t).min(1.0);
        }
        [red, 0.0, blue, 1.0].into()
    }

    fn domain(&self) -> (f32, f32) {
        (-1.0, 1.0)
    }
}
