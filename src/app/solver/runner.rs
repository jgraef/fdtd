use std::time::{
    Duration,
    Instant,
};

use color_eyre::eyre::bail;
use nalgebra::{
    Isometry3,
    Matrix4,
    Point3,
    Translation3,
    UnitQuaternion,
    Vector3,
};
use parry3d::bounding_volume::Aabb;

use crate::{
    Error,
    app::{
        composer::{
            renderer::{
                light::LoadMaterialTextures,
                resource::RenderResourceCreator,
                texture_channel::UndecidedTextureSender,
            },
            scene::{
                Scene,
                transform::Transform,
            },
        },
        solver::{
            DomainDescription,
            SolverBackend,
            SolverInstance,
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
                Resolution,
                cpu::FdtdCpuBackend,
                wgpu::FdtdWgpuBackend,
            },
            observer::Observer,
            project::{
                BeginProjectionPass,
                CreateProjection,
                ProjectionParameters,
                ProjectionPass,
                ProjectionPassAdd,
            },
            source::{
                GaussianPulse,
                ScalarSourceFunctionExt,
                Source,
            },
        },
        start::WgpuContext,
    },
    physics::material::Material,
    util::format_size,
};

#[derive(Debug)]
pub struct SolverRunner {
    fdtd_wgpu: FdtdWgpuBackend,
    render_resource_creator: RenderResourceCreator,
}

impl SolverRunner {
    pub fn new(
        wgpu_context: &WgpuContext,
        render_resource_creator: &RenderResourceCreator,
    ) -> Self {
        Self {
            fdtd_wgpu: FdtdWgpuBackend::new(&wgpu_context.device, &wgpu_context.queue),
            render_resource_creator: render_resource_creator.clone(),
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
                &self.render_resource_creator,
            )
        };

        match &common_config.parallelization {
            None => {
                run_single_threaded()?;
            }
            Some(Parallelization::MultiThreaded { num_threads }) => {
                if num_threads.is_some_and(|num_threads| num_threads <= 1) {
                    tracing::debug!(
                        ?num_threads,
                        "switching to single-threaded backend, because num_threads <= 1"
                    );
                    run_single_threaded()?;
                }
                else {
                    #[cfg(not(feature = "rayon"))]
                    {
                        let _ = num_threads;
                        tracing::warn!(
                            "Compiled without rayon feature. Falling back to single-threaded"
                        );
                        run_single_threaded()?;
                    }

                    tracing::debug!(?num_threads, "using multi-threaded cpu backend");
                    #[cfg(feature = "rayon")]
                    run_fdtd_with_backend(
                        scene,
                        common_config,
                        fdtd_config,
                        &FdtdCpuBackend::multi_threaded(*num_threads)?,
                        &self.render_resource_creator,
                    )?;
                }
            }
            Some(Parallelization::Wgpu) => {
                tracing::debug!("using wgpu backend");
                run_fdtd_with_backend(
                    scene,
                    common_config,
                    fdtd_config,
                    &self.fdtd_wgpu,
                    &self.render_resource_creator,
                )?;
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
    render_resource_creator: &RenderResourceCreator,
) -> Result<(), Error>
where
    Backend: SolverBackend<FdtdSolverConfig, Point3<usize>>,
    Backend::Instance:
        EvaluateStopCondition + CreateProjection<UndecidedTextureSender> + Send + 'static,
    <Backend::Instance as SolverInstance>::State: Time + Send + 'static,
    for<'a> <Backend::Instance as SolverInstance>::UpdatePass<'a>: UpdatePassForcing<Point3<usize>>,
    for<'a> <Backend::Instance as BeginProjectionPass>::ProjectionPass<'a>: ProjectionPassAdd<
            'a,
            <Backend::Instance as CreateProjection<UndecidedTextureSender>>::Projection,
        >,
    <Backend::Instance as CreateProjection<UndecidedTextureSender>>::Projection: Send + 'static,
{
    let time_start = Instant::now();

    let aabb = common_config.volume.aabb(scene);

    let size = aabb.extents();
    if !size.iter().all(|c| c.is_finite() && *c >= 0.0) {
        bail!("invalid aabb: {aabb:?}");
    }

    let config = FdtdSolverConfig {
        resolution: fdtd_config.resolution,
        physical_constants: common_config.physical_constants,
        size: size.cast(),
    };

    // check courant condition
    let temporal_resolution_satisfying_courant_condition =
        fdtd::estimate_temporal_from_spatial_resolution(
            common_config.physical_constants.speed_of_light(),
            &config.resolution.spatial,
        );
    if config.resolution.temporal > temporal_resolution_satisfying_courant_condition {
        tracing::warn!(resolution = ?config.resolution, "resolution doesn't satisfy courant condition");
    }

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

    // todo: we want an ui flow that prepares the solver-run anyway, so
    // we could display and warn about memory requirements there.
    // for now this is just a safe-guard that I don't crash my system xD
    if let (Some(memory_required), Some(memory_limit)) =
        (memory_required, common_config.memory_limit)
        && memory_required > memory_limit
    {
        bail!(
            "too much memory required: {memory_required_str} > {}",
            format_size(memory_limit)
        );
    }

    // coordinate transformations
    let coordinate_transformations = CoordinateTransformations::for_fdtd(
        &config.resolution,
        &common_config.volume.rotation(),
        &aabb,
    );

    let materials = SceneDomainDescription::new(scene, &coordinate_transformations);

    let instance = backend
        .create_instance(&config, materials)
        .expect("fdtd solver instance creation never fails");

    let mut state = instance.create_state();

    // create sources
    // todo: from scene. this is blocked by the fact that this specific source needs
    // config parameters.

    // let sources = Sources::from_scene(scene);
    let mut sources = Sources::default();
    /*let source = fdtd::source::ContinousWave {
        electric_current_density_amplitude: Vector3::z() / config.resolution.temporal,
        magnetic_current_density_amplitude: Vector3::zeros(),
        electric_current_density_phase: 0.0,
        magnetic_current_density_phase: 0.0,
        frequency: 2.0,
    };*/
    sources.push(
        Point3::from(lattice_size / 2),
        GaussianPulse::new(
            config.resolution.temporal * 50.0,
            config.resolution.temporal * 10.0,
        )
        .with_amplitudes(Vector3::z() / config.resolution.temporal, Vector3::zeros()),
    );

    // create observers
    let observers = Observers::from_scene(
        &instance,
        &mut state,
        scene,
        &lattice_size,
        render_resource_creator,
    );

    tracing::debug!("time to create simulation: {:?}", time_start.elapsed());

    // run simulation
    spawn_solver(
        instance,
        state,
        fdtd_config.stop_condition,
        sources,
        observers,
    );

    Ok(())
}

fn spawn_solver<Instance>(
    instance: Instance,
    mut state: Instance::State,
    stop_condition: StopCondition,
    sources: Sources,
    mut observers: Observers<<Instance as CreateProjection<UndecidedTextureSender>>::Projection>,
) where
    Instance: SolverInstance
        + EvaluateStopCondition
        + CreateProjection<UndecidedTextureSender>
        + Send
        + 'static,
    Instance::State: Time + Send + 'static,
    for<'a> Instance::UpdatePass<'a>: UpdatePassForcing<Point3<usize>>,
    for<'a> <Instance as BeginProjectionPass>::ProjectionPass<'a>:
        ProjectionPassAdd<'a, <Instance as CreateProjection<UndecidedTextureSender>>::Projection>,
    <Instance as CreateProjection<UndecidedTextureSender>>::Projection: Send + 'static,
{
    let _join_handle = std::thread::spawn(move || {
        let time_start = Instant::now();
        //let step_duration = Some(Duration::from_millis(10));
        let step_duration: Option<Duration> = None;
        let observation_duration = Some(Duration::from_millis(1000 / 25));
        let mut time_last_observation: Option<Instant> = None;

        loop {
            let time_elapsed = time_start.elapsed();

            if instance.evaluate_stop_condition(&state, &stop_condition, time_elapsed) {
                tracing::debug!("stop condition reached");
                break;
            }

            //tracing::debug!(tick = simulation.tick(), elapsed = ?time_elapsed);

            let time_pass_start = Instant::now();

            let time = state.time();
            let mut update_pass = instance.begin_update(&mut state);
            sources.apply(time, &mut update_pass);
            update_pass.finish();

            if observation_duration.is_some_and(|observation_duration| {
                time_last_observation.is_none_or(|time_last_observation| {
                    time_last_observation.elapsed() > observation_duration
                })
            }) {
                observers.run(&instance, &state);
                time_last_observation = Some(Instant::now());
            }

            let time_pass = time_pass_start.elapsed();

            if let Some(step_duration) = step_duration {
                let sleep = step_duration.saturating_sub(time_pass);
                if !sleep.is_zero() {
                    std::thread::sleep(sleep);
                }
            }
        }
    });
}

#[derive(derive_more::Debug)]
struct SceneDomainDescription<'a, 'b> {
    scene: &'a Scene,
    #[debug("hecs::ViewBorrow {{ ... }}")]
    materials: hecs::ViewBorrow<'a, &'b Material>,
    coordinate_transformations: &'a CoordinateTransformations,
}

impl<'a, 'b> SceneDomainDescription<'a, 'b> {
    pub fn new(
        scene: &'a Scene,
        coordinate_transformations: &'a CoordinateTransformations,
    ) -> Self {
        // access to the material properties
        let materials = scene.entities.view::<&Material>();

        Self {
            scene,
            materials,
            coordinate_transformations,
        }
    }
}

impl<'a, 'b> DomainDescription<Point3<usize>> for SceneDomainDescription<'a, 'b> {
    fn material(&self, point: &Point3<usize>) -> Material {
        let point = Point3::from_homogeneous(
            self.coordinate_transformations
                .transform_from_solver_to_world
                * point.cast::<f64>().to_homogeneous(),
        )
        .unwrap()
        .coords
        .try_cast::<f32>()
        .unwrap()
        .into();

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

#[derive(Debug, Default)]
struct Observers<P> {
    projections: Vec<P>,
}

impl<P> Observers<P> {
    pub fn from_scene<I>(
        instance: &I,
        state: &mut I::State,
        scene: &mut Scene,
        lattice_size: &Vector3<usize>,
        render_resource_creator: &RenderResourceCreator,
    ) -> Self
    where
        I: CreateProjection<UndecidedTextureSender, Projection = P>,
        for<'a> <I as BeginProjectionPass>::ProjectionPass<'a>: ProjectionPassAdd<'a, P>,
    {
        // todo:
        // - derive projection from observer and transform
        // - transform projection into simulation coordinate space

        // clippy, i want to chain other options into it later.
        #[allow(clippy::let_and_return)]
        let projections = scene
            .entities
            .query_mut::<&Observer>()
            .into_iter()
            .flat_map(|(entity, observer)| {
                tracing::debug!(?observer, "creating observer");

                let display_as_texture = observer.display_as_texture.then(|| {
                    let parameters = ProjectionParameters {
                        projection: Matrix4::identity(),
                        field: observer.field,
                        color_map: observer.color_map,
                    };

                    // create a texture channel. the sender is still undecided whether it will share
                    // a image buffer in host memory with the renderer, or request a gpu texture
                    // directly.
                    //
                    // todo: can we make so that the RENDER_ATTACHMENT usage is only applied if a
                    // texture for rendering is requested by the backend? and likewise for COPY_DST
                    let (sender, receiver) = render_resource_creator.create_texture_channel(
                        &lattice_size.xy().cast(),
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_DST,
                        "observer",
                    );

                    scene.command_buffer.insert_one(
                        entity,
                        LoadMaterialTextures::default().with_ambient_and_diffuse(receiver),
                    );

                    instance.create_projection(state, sender, &parameters)
                });

                display_as_texture
            })
            .collect();

        // apply deferred commands
        scene.apply_deferred();

        Self { projections }
    }

    pub fn run<I>(&mut self, instance: &I, state: &I::State)
    where
        I: BeginProjectionPass,
        for<'a> <I as BeginProjectionPass>::ProjectionPass<'a>: ProjectionPassAdd<'a, P>,
    {
        let mut pass = instance.begin_projection_pass(state);

        for projection in &mut self.projections {
            pass.add_projection(projection);
        }

        pass.finish();
    }
}

#[derive(Debug, Default)]
struct Sources {
    sources: Vec<(Point3<usize>, Source)>,
}

impl Sources {
    pub fn from_scene(scene: &mut Scene) -> Self {
        let sources = scene
            .entities
            .query_mut::<(&Transform, &Source)>()
            .into_iter()
            .map(|(_entity, (transform, source))| {
                let _world_point = transform.position();

                // todo: transform to simulation coordinate space

                let sim_point = Point3::origin();
                (sim_point, source.clone())
            })
            .collect();

        Self { sources }
    }

    pub fn push(&mut self, point: Point3<usize>, source: impl Into<Source>) {
        let source = source.into();
        tracing::debug!(?point, ?source, "creating source");
        self.sources.push((point, source));
    }

    pub fn apply<UpdatePass>(&self, time: f64, update_pass: &mut UpdatePass)
    where
        UpdatePass: UpdatePassForcing<Point3<usize>>,
    {
        for (point, source) in &self.sources {
            let values = source.0.evaluate(time);
            update_pass.set_forcing(point, &values);
        }
    }
}

/// TODO: This should be created by the backend and probably be a trait
#[derive(Clone, Copy, Debug)]
pub struct CoordinateTransformations {
    pub transform_from_solver_to_world: Matrix4<f64>,
    pub transform_from_world_to_solver: Matrix4<f64>,
}

impl CoordinateTransformations {
    pub fn for_fdtd(resolution: &Resolution, rotation: &UnitQuaternion<f32>, aabb: &Aabb) -> Self {
        let scaling_from_solver_to_world = Matrix4::new_nonuniform_scaling(&resolution.spatial);
        let scaling_from_world_to_solver =
            Matrix4::new_nonuniform_scaling(&resolution.spatial.map(|x| 1.0 / x));
        let rotation_from_solver_to_world = rotation.cast::<f64>();
        let translation_from_solver_to_world = Translation3::from(aabb.mins.coords.cast::<f64>());
        let isometry_from_solver_to_world = Isometry3::from_parts(
            translation_from_solver_to_world,
            rotation_from_solver_to_world,
        );
        let isometry_from_world_to_solver = isometry_from_solver_to_world.inverse();

        let transform_from_solver_to_world =
            isometry_from_solver_to_world.to_homogeneous() * scaling_from_solver_to_world;
        let transform_from_world_to_solver =
            scaling_from_world_to_solver * isometry_from_world_to_solver.to_homogeneous();

        Self {
            transform_from_solver_to_world,
            transform_from_world_to_solver,
        }
    }
}
