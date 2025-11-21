use std::time::{
    Duration,
    Instant,
};

use colorgrad::Gradient;
use nalgebra::{
    Point3,
    Vector3,
};
use palette::Srgba;

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
            DomainDescription,
            Field,
            FieldComponent,
            FieldView,
            SolverBackend,
            SolverInstance,
            SourceValues,
            Time,
            UpdatePass,
            UpdatePassForcing,
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
                cpu::FdtdCpuBackend,
                source::{
                    GaussianPulse,
                    SourceFunction,
                },
                wgpu::FdtdWgpuBackend,
            },
            util::WriteImage,
        },
    },
    physics::material::Material,
    util::format_size,
};

#[derive(Debug)]
pub struct SolverRunner {
    fdtd_wgpu: FdtdWgpuBackend,
}

impl SolverRunner {
    pub fn new(wgpu_context: &WgpuContext) -> Self {
        Self {
            fdtd_wgpu: FdtdWgpuBackend::new(&wgpu_context.device, &wgpu_context.queue),
        }
    }

    /// TODO: We probably just want one parameter that impls some trait. That
    /// trait defines how a solver_config and scene is turned into the problem
    /// description for the runner (e.g. a `fdtd::Simulation`).
    pub fn run(&mut self, solver_config: &SolverConfig, scene: &mut Scene) -> Result<(), Error> {
        match &solver_config.specifics {
            SolverConfigSpecifics::Fdtd(fdtd_config) => {
                self.run_fdtd(scene, &solver_config.common, fdtd_config)?;
            }
            SolverConfigSpecifics::Feec(_feec_config) => tracing::debug!("todo: feec solver"),
        }

        Ok(())
    }

    fn run_fdtd(
        &self,
        scene: &mut Scene,
        common_config: &SolverConfigCommon,
        fdtd_config: &SolverConfigFdtd,
    ) -> Result<(), Error> {
        let mut run_single_threaded = || {
            run_fdtd_with_backend(
                scene,
                common_config,
                fdtd_config,
                &FdtdCpuBackend::single_threaded(),
            )
        };

        match &common_config.parallelization {
            None => {
                run_single_threaded();
            }
            Some(Parallelization::MultiThreaded { num_threads }) => {
                if num_threads.is_some_and(|num_threads| num_threads <= 1) {
                    tracing::debug!(
                        ?num_threads,
                        "switching to single-threaded backend, because num_threads <= 1"
                    );
                    run_single_threaded();
                }
                else {
                    #[cfg(not(feature = "rayon"))]
                    {
                        let _ = num_threads;
                        tracing::warn!(
                            "Compiled without rayon feature. Falling back to single-threaded"
                        );
                        run_single_threaded();
                    }

                    #[cfg(feature = "rayon")]
                    run_fdtd_with_backend(
                        scene,
                        common_config,
                        fdtd_config,
                        &FdtdCpuBackend::multi_threaded(*num_threads)?,
                    )
                }
            }
            Some(Parallelization::Wgpu) => {
                run_fdtd_with_backend(scene, common_config, fdtd_config, &self.fdtd_wgpu)
            }
        }

        Ok(())
    }
}

fn run_fdtd_with_backend<Backend>(
    scene: &mut Scene,
    common_config: &SolverConfigCommon,
    fdtd_config: &SolverConfigFdtd,
    backend: &Backend,
) where
    Backend: SolverBackend<FdtdSolverConfig, Point3<usize>>,
    Backend::Instance:
        EvaluateStopCondition + SolverInstance + Field<Point3<usize>> + Send + 'static,
    <Backend::Instance as SolverInstance>::State: Time + Send + 'static,
    for<'a> <Backend::Instance as SolverInstance>::UpdatePass<'a>: UpdatePassForcing<Point3<usize>>,
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

    let memory_required = backend.memory_required(&config);
    let memory_required_str = memory_required.map_or_else(
        || "unknown".to_owned(),
        |memory_required| format_size(memory_required).to_string(),
    );
    let lattice_size = config.size();

    tracing::debug!(
        ?size,
        resolution = ?config.resolution,
        memory_required = memory_required_str,
        ?lattice_size,
        "creating fdtd simulation"
    );

    // todo: remove this. we want a ui flow that prepares the solver-run anyway, so
    // we could display and warn about memory requirements there.
    // for now this is just a safe-guard that I don't crash my system xD
    if memory_required.is_some_and(|memory_required| memory_required > 200_000_000) {
        tracing::warn!("abort. too much memory required");
        return;
    }

    let materials = SceneDomainDescription::new(scene);

    let instance = backend
        .create_instance(&config, materials)
        .expect("fdtd solver instance creation never fails");

    let state = instance.create_state();

    // create sources
    // todo: from scene
    let mut sources = Sources::default();
    /*let source = fdtd::source::ContinousWave {
        electric_current_density_amplitude: Vector3::z() / config.resolution.temporal,
        magnetic_current_density_amplitude: Vector3::zeros(),
        electric_current_density_phase: 0.0,
        magnetic_current_density_phase: 0.0,
        frequency: 2.0,
    };*/
    sources.add(
        (lattice_size / 2).into(),
        Vector3::z() / config.resolution.temporal,
        Vector3::zeros(),
        GaussianPulse {
            time: config.resolution.temporal * 50.0,
            duration: config.resolution.temporal * 10.0,
        },
    );

    // create observers
    let observers = Observers::from_scene(scene, &lattice_size);

    tracing::debug!("time to create simulation: {:?}", time_start.elapsed());

    // run simulation
    spawn_solver(
        instance,
        state,
        fdtd_config.stop_condition,
        sources,
        observers,
    );
}

fn spawn_solver<Instance>(
    instance: Instance,
    mut state: Instance::State,
    stop_condition: StopCondition,
    sources: Sources,
    mut observers: Observers,
) where
    Instance: SolverInstance + EvaluateStopCondition + Field<Point3<usize>> + Send + 'static,
    Instance::State: Time + Send + 'static,
    for<'a> Instance::UpdatePass<'a>: UpdatePassForcing<Point3<usize>>,
{
    let _join_handle = std::thread::spawn(move || {
        let time_start = Instant::now();

        loop {
            let time_elapsed = time_start.elapsed();

            if instance.evaluate_stop_condition(&state, &stop_condition, time_elapsed) {
                tracing::debug!("stop condition reached");
                break;
            }

            //tracing::debug!(tick = simulation.tick(), elapsed = ?time_elapsed);

            let time = state.time();
            let mut update_pass = instance.begin_update(&mut state);
            sources.apply(time, &mut update_pass);
            update_pass.finish();

            observers.run(&instance, &state);

            //run_observers(&simulation);
            std::thread::sleep(Duration::from_millis(10));
        }
    });
}

struct SceneDomainDescription<'a, 'b> {
    scene: &'a Scene,
    materials: hecs::ViewBorrow<'a, &'b Material>,
}

impl<'a, 'b> SceneDomainDescription<'a, 'b> {
    pub fn new(scene: &'a Scene) -> Self {
        // access to the material properties
        let materials = scene.entities.view::<&Material>();

        Self { scene, materials }
    }
}

impl<'a, 'b> DomainDescription<Point3<usize>> for SceneDomainDescription<'a, 'b> {
    fn material(&self, point: &Point3<usize>) -> Material {
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

#[derive(Debug)]
struct Observer {
    texture_output: TextureWriter,
    gradient: TestGradient,
    field_component: FieldComponent,
}

impl Observer {
    pub fn run<I>(&mut self, instance: &I, state: &I::State)
    where
        I: Field<Point3<usize>>,
    {
        let view = instance.field(state, .., self.field_component);

        self.texture_output
            .write_colors(|point| {
                let point = Point3::new(point.x as usize, point.y as usize, 0);

                let value = view.at(&point).unwrap();
                let value = value.z.clamp(-1.0, 1.0) as f32;

                let color: Srgba = self.gradient.at(value).to_array().into();

                color
            })
            .unwrap();
    }
}

#[derive(Debug, Default)]
struct Observers {
    observers: Vec<Observer>,
}

impl Observers {
    pub fn from_scene(scene: &mut Scene, lattice_size: &Vector3<usize>) -> Self {
        let mut observers = vec![];

        for (entity, observer) in scene.entities.query_mut::<&super::observer::Observer>() {
            // todo: use observer extents

            if observer.display_as_texture {
                // todo: check if this already exists
                let texture_output = TextureWriter::new(lattice_size.xy().cast());
                scene
                    .command_buffer
                    .insert_one(entity, texture_output.clone());
                observers.push(Observer {
                    texture_output,
                    gradient: TestGradient,
                    field_component: observer.field_component,
                });
            }
        }

        // apply deferred commands
        scene.apply_deferred();

        Self { observers }
    }

    pub fn run<I>(&mut self, instance: &I, state: &I::State)
    where
        I: Field<Point3<usize>>,
    {
        for observer in &mut self.observers {
            observer.run(instance, state);
        }
    }
}

#[derive(Debug)]
struct Source {
    function: Box<dyn SourceFunction>,
    j_amplitude: Vector3<f64>,
    m_amplitude: Vector3<f64>,
    point: Point3<usize>,
}

#[derive(Debug, Default)]
struct Sources {
    sources: Vec<Source>,
}

impl Sources {
    pub fn add(
        &mut self,
        point: Point3<usize>,
        j_amplitude: Vector3<f64>,
        m_amplitude: Vector3<f64>,
        function: impl SourceFunction,
    ) {
        self.sources.push(Source {
            function: Box::new(function),
            j_amplitude,
            m_amplitude,
            point,
        });
    }

    pub fn apply<UpdatePass>(&self, time: f64, update_pass: &mut UpdatePass)
    where
        UpdatePass: UpdatePassForcing<Point3<usize>>,
    {
        for source in &self.sources {
            let value = source.function.evaluate(time);
            update_pass.set_forcing(
                &source.point,
                &SourceValues {
                    j_source: value * source.j_amplitude,
                    m_source: value * source.m_amplitude,
                },
            );
        }
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
