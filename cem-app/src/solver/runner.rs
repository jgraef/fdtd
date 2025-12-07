use std::{
    sync::Arc,
    thread::JoinHandle,
    time::{
        Duration,
        Instant,
    },
};

use bevy_ecs::{
    entity::Entity,
    system::{
        Commands,
        In,
        InMut,
        InRef,
        Query,
        SystemParam,
    },
    world::World,
};
use cem_render::{
    material,
    resource::RenderResourceManager,
    texture::Sampler,
};
use cem_scene::{
    Scene,
    spatial::{
        Collider,
        queries::{
            IntersectAabb,
            PointQuery,
        },
    },
    transform::GlobalTransform,
};
use cem_solver::{
    DomainDescription,
    SolverBackend,
    SolverInstance,
    Time,
    UpdatePass,
    UpdatePassForcing,
    fdtd::{
        self,
        FdtdSolverConfig,
        Resolution,
        cpu::FdtdCpuBackend,
        pml::{
            GradedPml,
            PmlCoefficients,
        },
        wgpu::FdtdWgpuBackend,
    },
    material::{
        Material,
        PhysicalConstants,
    },
    project::{
        BeginProjectionPass,
        CreateProjection,
        ProjectionParameters,
        ProjectionPass,
        ProjectionPassAdd,
    },
    source::Source,
};
use cem_util::{
    egui::{
        EguiUtilContextExt,
        RepaintTrigger,
    },
    format_size,
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
use parking_lot::{
    Condvar,
    Mutex,
    MutexGuard,
};
use parry3d::{
    bounding_volume::Aabb,
    query::Ray,
};

use crate::{
    Error,
    app::CreateAppContext,
    error::{
        ErrorHandler,
        UiErrorSink,
    },
    solver::{
        config::{
            Parallelization,
            SolverConfig,
            SolverConfigCommon,
            SolverConfigFdtd,
            SolverConfigSpecifics,
            StopCondition,
        },
        observer::{
            Observer,
            TextureSenderTarget,
        },
    },
    util::spawn_thread,
};

#[derive(Debug)]
pub struct SolverRunner {
    fdtd_wgpu: FdtdWgpuBackend,

    // todo: these 2 should probably be resources in the scene? (the repaint trigger kind of is in
    // form of a AsyncUpdateTrigger)
    repaint_trigger: RepaintTrigger,
    error_sink: UiErrorSink,

    active_solver: Option<Solver>,
}

impl SolverRunner {
    pub fn from_app_context(context: &CreateAppContext) -> Self {
        Self {
            fdtd_wgpu: FdtdWgpuBackend::new(
                context.wgpu_context.device.clone(),
                context.wgpu_context.queue.clone(),
                context.wgpu_context.staging_pool.clone(),
            ),
            repaint_trigger: context.egui_context.repaint_trigger(),
            error_sink: UiErrorSink::from(&context.egui_context),
            active_solver: None,
        }
    }

    /// TODO: We probably just want one parameter that impls some trait. That
    /// trait defines how a solver_config and scene is turned into the problem
    /// description for the runner (e.g. a `fdtd::Simulation`).
    pub fn run(&mut self, solver_config: &SolverConfig, scene: &mut Scene) -> Result<(), Error> {
        if self.active_solver.is_some() {
            bail!("Can't run more than one solver at once.");
        }

        match &solver_config.specifics {
            SolverConfigSpecifics::Fdtd(fdtd_config) => {
                self.run_fdtd(scene, &solver_config.common, fdtd_config)?;
            }
            SolverConfigSpecifics::Feec(_feec_config) => tracing::debug!("todo: feec solver"),
        }

        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(solver) = self.active_solver.take() {
            tracing::debug!("Requested closing of solver");

            let mut state = solver.shared.state.lock();
            state.finished = true;
            if state.paused {
                solver.shared.condition.notify_all();
            }
            drop(state);

            if let Err(panic) = solver.join_handle.join() {
                tracing::error!(?panic, "Solver thread panicked");
            }
        }
    }

    pub fn active_solver(&self) -> Option<&Solver> {
        self.active_solver.as_ref()
    }

    fn run_fdtd(
        &mut self,
        scene: &mut Scene,
        common_config: &SolverConfigCommon,
        fdtd_config: &SolverConfigFdtd,
    ) -> Result<(), Error> {
        let run_fdtd = RunFdtd {
            scene,
            common_config,
            fdtd_config,
            repaint_trigger: self.repaint_trigger.clone(),
            error_sink: self.error_sink.clone(),
        };

        let solver = match &common_config.parallelization {
            None => run_fdtd.run_fdtd_with_backend(&FdtdCpuBackend::single_threaded())?,
            Some(Parallelization::MultiThreaded { num_threads }) => {
                if num_threads.is_some_and(|num_threads| num_threads <= 1) {
                    tracing::debug!(
                        ?num_threads,
                        "switching to single-threaded backend, because num_threads <= 1"
                    );
                    run_fdtd.run_fdtd_with_backend(&FdtdCpuBackend::single_threaded())?
                }
                else {
                    #[cfg(not(feature = "multi-threading"))]
                    {
                        let _ = num_threads;
                        tracing::warn!(
                            "Compiled without rayon feature. Falling back to single-threaded"
                        );
                        run_fdtd.run_fdtd_with_backend(&FdtdCpuBackend::single_threaded())?
                    }

                    #[cfg(feature = "multi-threading")]
                    {
                        tracing::debug!(?num_threads, "using multi-threaded cpu backend");
                        run_fdtd
                            .run_fdtd_with_backend(&FdtdCpuBackend::multi_threaded(*num_threads)?)?
                    }
                }
            }
            Some(Parallelization::Wgpu) => {
                tracing::debug!("using wgpu backend");
                run_fdtd.run_fdtd_with_backend(&self.fdtd_wgpu)?
            }
        };

        self.active_solver = Some(solver);

        Ok(())
    }
}

struct RunFdtd<'a> {
    scene: &'a mut Scene,
    common_config: &'a SolverConfigCommon,
    fdtd_config: &'a SolverConfigFdtd,
    repaint_trigger: RepaintTrigger,
    error_sink: UiErrorSink,
}

impl<'a> RunFdtd<'a> {
    fn run_fdtd_with_backend<Backend>(self, backend: &Backend) -> Result<Solver, Error>
    where
        Backend: SolverBackend<FdtdSolverConfig, Point3<usize>> + 'static,
        Backend::Instance: CreateProjection<TextureSenderTarget> + Send + 'static,
        <Backend::Instance as SolverInstance>::State: Time + Send + 'static,
        for<'b> <Backend::Instance as SolverInstance>::UpdatePass<'b>:
            UpdatePassForcing<Point3<usize>>,
        for<'b> <Backend::Instance as BeginProjectionPass>::ProjectionPass<'b>: ProjectionPassAdd<
                'b,
                <Backend::Instance as CreateProjection<TextureSenderTarget>>::Projection,
            >,
        <Backend::Instance as CreateProjection<TextureSenderTarget>>::Projection: Send + 'static,
    {
        let Self {
            scene,
            common_config,
            fdtd_config,
            repaint_trigger,
            error_sink,
        } = self;

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
            &lattice_size,
            &common_config.volume.rotation(),
            &aabb,
        );

        let instance = scene
            .world
            .run_system_cached_with(
                create_solver_instance_system,
                (
                    backend,
                    &config,
                    coordinate_transformations,
                    common_config.default_material,
                ),
            )
            .unwrap()
            .expect("fdtd solver instance creation never fails");

        let mut state = instance.create_state();

        let sources = Sources::from_scene(&mut scene.world, &coordinate_transformations);

        // create observers
        let observers = Observers::from_scene(
            &instance,
            &mut state,
            &mut scene.world,
            &lattice_size,
            repaint_trigger,
        );

        tracing::debug!("time to create simulation: {:?}", time_start.elapsed());

        // run simulation
        let solver = Solver::spawn(
            instance,
            state,
            fdtd_config.stop_condition,
            sources,
            observers,
            error_sink,
        );

        Ok(solver)
    }
}

#[derive(Debug)]
struct Shared {
    state: Mutex<SolverState>,
    condition: Condvar,
}

#[derive(Clone, Copy, Debug)]
pub struct SolverState {
    pub finished: bool,
    pub paused: bool,
    pub sim_time: f64,
    pub sim_tick: usize,
    pub start_time: Instant,
    pub stop_time: Option<Instant>,
    pub total_running_time: Duration,
    pub last_step_time: Duration,
    pub step_delay: Option<Duration>,
    pub observation_delay: Option<Duration>,
}

#[derive(Debug)]
pub struct Solver {
    join_handle: JoinHandle<()>,
    shared: Arc<Shared>,
}

impl Solver {
    pub fn state(&self) -> SolverState {
        let state = self.shared.state.lock();
        *state
    }

    pub fn state_mut(&self) -> MutexGuard<'_, SolverState> {
        self.shared.state.lock()
    }

    pub fn stop(&self) {
        let mut state = self.shared.state.lock();
        state.finished = true;
        self.shared.condition.notify_all();
    }

    pub fn pause(&self) {
        let mut state = self.shared.state.lock();
        state.paused = true;
    }

    pub fn resume(&self) {
        let mut state = self.shared.state.lock();
        state.paused = false;
        self.shared.condition.notify_all();
    }

    fn spawn<Instance>(
        instance: Instance,
        mut state: Instance::State,
        stop_condition: StopCondition,
        sources: Sources,
        mut observers: Observers<<Instance as CreateProjection<TextureSenderTarget>>::Projection>,
        error_sink: UiErrorSink,
    ) -> Self
    where
        Instance: SolverInstance + CreateProjection<TextureSenderTarget> + Send + 'static,
        Instance::State: Time + Send + 'static,
        for<'a> Instance::UpdatePass<'a>: UpdatePassForcing<Point3<usize>>,
        for<'a> <Instance as BeginProjectionPass>::ProjectionPass<'a>:
            ProjectionPassAdd<'a, <Instance as CreateProjection<TextureSenderTarget>>::Projection>,
        <Instance as CreateProjection<TextureSenderTarget>>::Projection: Send + 'static,
    {
        let start_paused = true;

        let control_state = SolverState {
            finished: false,
            paused: start_paused,
            sim_time: 0.0,
            sim_tick: 0,
            start_time: Instant::now(),
            stop_time: None,
            total_running_time: Duration::ZERO,
            last_step_time: Duration::ZERO,
            step_delay: Some(Duration::from_millis(10)),
            observation_delay: Some(Duration::from_millis(1000 / 25)),
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(control_state),
            condition: Condvar::new(),
        });

        let join_handle = spawn_thread("solver", {
            let shared = shared.clone();

            move || {
                let mut time_last_observation: Option<Instant> = None;
                let mut stop_condition_reached = false;
                let mut time_pass = Duration::ZERO;
                let mut total_time = Duration::ZERO;

                // if we start out paused we want to run ob observers at least once
                if start_paused && let Err(error) = observers.run(&instance, &state) {
                    error_sink.handle_error(error);
                    return;
                }

                loop {
                    let mut control_state = shared.state.lock();

                    // update some data in the shared struct
                    control_state.sim_tick = state.tick();
                    control_state.sim_time = state.time();
                    control_state.last_step_time = time_pass;
                    control_state.total_running_time = total_time;

                    control_state.finished |= stop_condition_reached;
                    if control_state.finished {
                        control_state.stop_time = Some(Instant::now());
                        return;
                    }

                    if control_state.paused {
                        shared.condition.wait(&mut control_state);
                    }
                    else {
                        let observation_delay = control_state.observation_delay;
                        let step_delay = control_state.step_delay;

                        drop(control_state);

                        // check if stop condition reached. if so, set flag and continue to next
                        // (and last) iteration of loop
                        if evaluate_stop_condition(&stop_condition, total_time, &state) {
                            stop_condition_reached = true;
                            continue;
                        }

                        let time_pass_start = Instant::now();

                        // note: can't just put the method call into the argument because by then
                        // the state is borrowed. we should probably give some access to the state
                        // during an update pass.
                        let sim_time = state.time();

                        // do the update pass
                        let mut update_pass = instance.begin_update(&mut state);
                        sources.apply(sim_time, &mut update_pass);
                        update_pass.finish();

                        // do observations
                        let do_observations = observation_delay.is_some_and(|observation_delay| {
                            time_last_observation.is_none_or(|time_last_observation| {
                                time_last_observation.elapsed() > observation_delay
                            })
                        });
                        if do_observations {
                            if let Err(error) = observers.run(&instance, &state) {
                                error_sink.handle_error(error);
                                stop_condition_reached = true;
                                continue;
                            }
                            time_last_observation = Some(Instant::now());
                        }

                        time_pass = time_pass_start.elapsed();
                        total_time += time_pass;

                        // sleep if we're ups limited
                        if let Some(step_delay) = step_delay {
                            let sleep = step_delay.saturating_sub(time_pass);
                            if !sleep.is_zero() {
                                std::thread::sleep(sleep);
                            }
                        }
                    }
                }
            }
        });

        Self {
            join_handle,
            shared,
        }
    }
}

fn create_solver_instance_system<Backend>(
    (InRef(backend), InRef(config), In(coordinate_transformations), In(default_material)): (
        InRef<Backend>,
        InRef<FdtdSolverConfig>,
        In<CoordinateTransformations>,
        In<Material>,
    ),
    world_domain_description: WorldDomainDescriptionSystemParam,
) -> Result<Backend::Instance, Backend::Error>
where
    Backend: SolverBackend<FdtdSolverConfig, Point3<usize>>,
{
    backend.create_instance(
        config,
        WorldDomainDescription {
            system_param: world_domain_description,
            coordinate_transformations,
            default_material,
            resolution: config.resolution,
            physical_constants: config.physical_constants,
        },
    )
}

#[derive(Debug, SystemParam)]
struct WorldDomainDescriptionSystemParam<'w, 's> {
    point_query: PointQuery<'w, 's>,
    materials: Query<'w, 's, &'static Material>,
    intersect_aabb_query: IntersectAabb<'w>,
    pmls: Query<
        'w,
        's,
        (
            &'static GradedPml,
            &'static Collider,
            &'static GlobalTransform,
        ),
    >,
}

struct WorldDomainDescription<'w, 's> {
    system_param: WorldDomainDescriptionSystemParam<'w, 's>,
    coordinate_transformations: CoordinateTransformations,
    default_material: Material,
    // todo: the solver knows these two so the pml parameters it takes should not need them
    resolution: Resolution,
    physical_constants: PhysicalConstants,
}

impl<'w, 's> DomainDescription<Point3<usize>> for WorldDomainDescription<'w, 's> {
    fn material(&mut self, point: &Point3<usize>) -> Material {
        let point = self
            .coordinate_transformations
            .transform_point_from_solver_to_world(point);

        let mut materials = self
            .system_param
            .point_query
            .point_query(point)
            .filter_map(|entity| self.system_param.materials.get(entity).ok())
            .cloned();

        // for now we'll just use the first material we find.
        // if nothing is found, use the default
        materials.next().unwrap_or(self.default_material)
    }

    fn pml(&mut self, point: &Point3<usize>) -> Option<PmlCoefficients> {
        let point = self
            .coordinate_transformations
            .transform_point_from_solver_to_world(point);

        let mut pmls = self
            .system_param
            .intersect_aabb_query
            .intersect_aabb(Aabb {
                mins: point,
                maxs: point,
            })
            .filter_map(|(entity, aabb)| {
                self.system_param.pmls.get(entity).ok().and_then(
                    |(pml, collider, global_transform)| {
                        let max_depth = nalgebra::distance(&aabb.mins, &aabb.maxs);
                        let ray = Ray::new(point, *pml.normal);

                        let ray_intersection = collider.cast_ray(
                            global_transform.isometry(),
                            &ray,
                            max_depth,
                            false,
                        )?;

                        Some(PmlCoefficients::new_graded(
                            &self.resolution,
                            &self.physical_constants,
                            pml.m,
                            pml.m_a,
                            pml.sigma_max,
                            pml.kappa_max,
                            pml.a_max,
                            ray_intersection.time_of_impact as f64,
                            -pml.normal.cast(),
                        ))
                    },
                )
            });

        // todo: merge pmls present at this point
        pmls.next()
    }
}

#[derive(Debug, Default)]
struct Observers<P> {
    projections: Vec<P>,
    repaint_trigger: Option<RepaintTrigger>,
}

impl<P> Observers<P> {
    pub fn from_scene<I>(
        instance: &I,
        state: &mut I::State,
        world: &mut World,
        lattice_size: &Vector3<usize>,
        repaint_trigger: RepaintTrigger,
    ) -> Self
    where
        I: CreateProjection<TextureSenderTarget, Projection = P> + 'static,
        I::State: 'static,
        P: 'static,
        for<'a> <I as BeginProjectionPass>::ProjectionPass<'a>: ProjectionPassAdd<'a, P>,
    {
        // todo:
        // - derive projection from observer and transform
        // - transform projection into simulation coordinate space

        world
            .run_system_cached_with(
                setup_observers_system::<I, P>,
                (instance, state, *lattice_size, repaint_trigger),
            )
            .unwrap()
    }

    pub fn run<I>(&mut self, instance: &I, state: &I::State) -> Result<(), Error>
    where
        I: BeginProjectionPass,
        for<'a> <I as BeginProjectionPass>::ProjectionPass<'a>: ProjectionPassAdd<'a, P>,
    {
        let mut pass = instance.begin_projection_pass(state);

        for projection in &mut self.projections {
            pass.add_projection(projection);
        }

        let result = pass.finish();

        if let Some(repaint_trigger) = &self.repaint_trigger {
            repaint_trigger.repaint();
        }

        result.map_err(Into::into)
    }
}

#[allow(clippy::type_complexity)]
fn setup_observers_system<I, P>(
    (InRef(instance), InMut(state), In(lattice_size), In(repaint_trigger)): (
        InRef<I>,
        InMut<I::State>,
        In<Vector3<usize>>,
        In<RepaintTrigger>,
    ),
    mut render_resource_manager: RenderResourceManager,
    observers: Query<(Entity, &Observer)>,
    mut commands: Commands,
) -> Observers<P>
where
    I: CreateProjection<TextureSenderTarget, Projection = P>,
    for<'a> <I as BeginProjectionPass>::ProjectionPass<'a>: ProjectionPassAdd<'a, P>,
{
    let mut needs_repaint = false;

    let projections = observers
        .iter()
        .filter_map(|(entity, observer)| {
            tracing::debug!(?observer, "creating observer");

            observer.display_as_texture.then(|| {
                needs_repaint = true;

                let parameters = ProjectionParameters {
                    projection: Matrix4::identity(), // todo
                    field: observer.field,
                    color_map: observer.color_map,
                    color_map_code: Some(
                        r#"
                    // color and alpha scaling
                    const s_c: f32 = 10.0;
                    const s_a: f32 = 100.0;

                    var color: vec4f;
                    var x = value.z;
                    if x > 0.0 {
                        color.r = min(s_c * x, 1.0);
                        color.a = min(s_a * x, 1.0);
                    }
                    else {
                        color.g = min(-s_c * x, 1.0);
                        color.a = min(-s_a * x, 1.0);
                    }
                    return color;
                    "#
                        .to_owned(),
                    ),
                };

                // create a texture channel. the sender is still undecided whether it
                // will share a image buffer in host memory
                // with the renderer, or request a gpu texture
                // directly.
                //
                // todo: can we make so that the RENDER_ATTACHMENT usage is only applied
                // if a texture for rendering is requested
                // by the backend? and likewise for COPY_DST
                let (sender, receiver) = render_resource_manager.create_texture_channel(
                    &lattice_size.xy().cast(),
                    wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST,
                    "observer",
                );

                commands.entity(entity).insert((
                    material::LoadAlbedoTexture::new(receiver)
                        .with_transparency(false)
                        .with_sampler(Sampler::LinearClamp),
                    material::Material {
                        transparent: true,
                        ..Default::default()
                    },
                ));

                instance.create_projection(state, TextureSenderTarget::from(sender), &parameters)
            })
        })
        .collect();

    Observers {
        projections,
        repaint_trigger: needs_repaint.then_some(repaint_trigger),
    }
}

#[derive(Debug, Default)]
struct Sources {
    sources: Vec<(Point3<usize>, Source)>,
}

impl Sources {
    pub fn from_scene(
        world: &mut World,
        coordinate_transformations: &CoordinateTransformations,
    ) -> Self {
        world
            .run_system_cached_with(setup_sources_system, coordinate_transformations)
            .unwrap()
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

fn setup_sources_system(
    InRef(coordinate_transformations): InRef<CoordinateTransformations>,
    sources: Query<(&GlobalTransform, &Source)>,
) -> Sources {
    let sources = sources
        .iter()
        .filter_map(|(global_transform, source)| {
            let world_point = global_transform.position();
            let sim_point =
                coordinate_transformations.transform_point_from_world_to_solver(&world_point)?;
            tracing::debug!(?world_point, ?sim_point, ?source, "creating source");

            Some((sim_point, source.clone()))
        })
        .collect();

    Sources { sources }
}

/// TODO: This should be created by the backend and probably be a trait
#[derive(Clone, Copy, Debug)]
pub struct CoordinateTransformations {
    pub transform_from_solver_to_world: Matrix4<f64>,
    pub transform_from_world_to_solver: Matrix4<f64>,
    pub lattice_size: Vector3<usize>,
}

impl CoordinateTransformations {
    pub fn for_fdtd(
        resolution: &Resolution,
        lattice_size: &Vector3<usize>,
        rotation: &UnitQuaternion<f32>,
        aabb: &Aabb,
    ) -> Self {
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
            lattice_size: *lattice_size,
        }
    }

    pub fn transform_point_from_solver_to_world(&self, point: &Point3<usize>) -> Point3<f32> {
        Point3::from_homogeneous(
            self.transform_from_solver_to_world * point.cast::<f64>().to_homogeneous(),
        )
        .unwrap()
        .coords
        .try_cast::<f32>()
        .unwrap()
        .into()
    }

    pub fn transform_point_from_world_to_solver(
        &self,
        point: &Point3<f32>,
    ) -> Option<Point3<usize>> {
        let point = Point3::from_homogeneous(
            self.transform_from_world_to_solver * point.cast::<f64>().to_homogeneous(),
        )
        .unwrap();
        let point = Point3::from(point.coords.map(|c| c.round()).try_cast::<usize>()?);
        (point.coords < self.lattice_size).then_some(point)
    }
}

pub fn evaluate_stop_condition<S>(
    stop_condition: &StopCondition,
    time_elapsed: Duration,
    state: &S,
) -> bool
where
    S: Time,
{
    match stop_condition {
        StopCondition::Never => false,
        StopCondition::StepLimit { limit } => state.tick() >= *limit,
        StopCondition::SimulatedTimeLimit { limit } => state.time() as f32 >= *limit,
        StopCondition::RealtimeLimit { limit } => time_elapsed >= *limit,
    }
}
