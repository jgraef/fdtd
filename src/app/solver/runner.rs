use std::{
    fs::File,
    io::BufWriter,
    time::{
        Duration,
        Instant,
    },
};

use colorgrad::Gradient;
use nalgebra::{
    Point3,
    Vector3,
};

use crate::{
    Error,
    app::{
        composer::{
            renderer::{
                WgpuContext,
                texture::TextureWriter,
            },
            scene::Scene,
        },
        solver::{
            config::{
                SolverConfig,
                SolverConfigCommon,
                SolverConfigFdtd,
                SolverConfigSpecifics,
                StopCondition,
            },
            observer::Observer,
            util::{
                WriteImage,
                gif::GifOutput,
            },
        },
    },
    fdtd,
    physics::PhysicalConstants,
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
    pub fn run(&mut self, solver_config: &SolverConfig, scene: &mut Scene) {
        match &solver_config.specifics {
            SolverConfigSpecifics::Fdtd(fdtd_config) => {
                self.run_fdtd(scene, &solver_config.common, fdtd_config);
            }
            SolverConfigSpecifics::Feec(_feec_config) => tracing::debug!("todo: feec solver"),
        }
    }

    fn run_fdtd(
        &mut self,
        scene: &mut Scene,
        common_config: &SolverConfigCommon,
        fdtd_config: &SolverConfigFdtd,
    ) {
        let aabb = common_config.volume.aabb(scene);
        let _rotation = common_config.volume.rotation(); // ignored for now

        let origin = aabb.mins;
        let mut size = aabb.extents();
        size.y = 0.0;
        size.z = 0.0;

        // todo: make fdtd generic over float, remove casts
        /*let config = fdtd::SimulationConfig {
            resolution: fdtd_config.resolution,
            physical_constants: common_config.physical_constants,
            origin: Some(origin.cast()),
            size: size.cast(),
        };*/
        let config = fdtd::SimulationConfig {
            resolution: fdtd::Resolution {
                spatial: Vector3::repeat(1.0),
                temporal: 0.25,
            },
            physical_constants: PhysicalConstants::REDUCED,
            origin: None,
            size: Vector3::new(100.0, 100.0, 0.0),
        };

        let memory_required = config.memory_usage_estimate();
        tracing::debug!(?origin, ?size, resolution = ?config.resolution, memory_required = %format_size(memory_required), lattice_size = ?config.lattice_size(), "creating fdtd simulation");

        // todo: remove this. we want a ui flow that prepares the solver-run anyway, so
        // we could display and warn about memory requirements there.
        // for now this is just a safe-guard that I don't crash my system xD
        if memory_required > 100_000_000 {
            tracing::warn!("abort. too much memory required");
            return;
        }

        let time_start = Instant::now();

        let mut simulation = fdtd::Simulation::new(&config);
        tracing::debug!(simulation_origin = ?simulation.origin());

        /*{
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
        }*/

        {
            // for testing we'll add a source at the origin
            // todo: remove this

            //let mut source_position = aabb.center();
            //source_position.y = origin.y;
            //source_position.z = origin.z;
            let source_position = Point3::new(-40.0, -40.0, 0.0);

            simulation.add_source(
                source_position.cast::<f64>(),
                fdtd::source::ContinousWave {
                    electric_current_density_amplitude: Vector3::z(),
                    magnetic_current_density_amplitude: Vector3::zeros(),
                    electric_current_density_phase: 0.0,
                    magnetic_current_density_phase: 0.0,
                    frequency: 0.1,
                },
            );
        }

        let mut run_observers = {
            let lattice_size = simulation.lattice().dimensions();

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
            move |simulation: &fdtd::Simulation| {
                let swap_buffer_index = simulation.swap_buffer_index();
                let z = lattice_size.z / 2;

                // only one for now
                let Some(image_output) = observers.first_mut()
                else {
                    return;
                };

                image_output
                    .write_field_values(
                        |point| {
                            let point = point.cast();
                            let cell = simulation
                                .lattice()
                                .get(&Point3::new(point.x, point.y, z))
                                .unwrap();

                            let e_field = cell.electric_field(swap_buffer_index);
                            //(0.5 + 0.5 * e_field.y).clamp(0.0, 1.0) as f32
                            e_field.z.clamp(-1.0, 1.0) as f32
                        },
                        &gradient,
                    )
                    .unwrap();
            }
        };

        tracing::debug!("time to create simulation: {:?}", time_start.elapsed());

        let _join_handle = std::thread::spawn({
            let stop_condition = fdtd_config.stop_condition;

            move || {
                let time_start = Instant::now();

                loop {
                    let time_elapsed = time_start.elapsed();

                    if evaluate_stop_condition_for_fdtd(&stop_condition, &simulation, time_elapsed)
                    {
                        tracing::debug!("stop condition reached");
                        break;
                    }

                    //tracing::debug!(tick = simulation.tick(), elapsed = ?time_elapsed);

                    simulation.step();

                    run_observers(&simulation);

                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
    }
}

fn evaluate_stop_condition_for_fdtd(
    stop_condition: &StopCondition,
    simulation: &fdtd::Simulation,
    time_elapsed: Duration,
) -> bool {
    match stop_condition {
        StopCondition::Never => false,
        StopCondition::StepLimit { limit } => simulation.tick() >= *limit,
        StopCondition::SimulatedTimeLimit { limit } => simulation.time() as f32 >= *limit,
        StopCondition::RealtimeLimit { limit } => time_elapsed >= *limit,
    }
}

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
