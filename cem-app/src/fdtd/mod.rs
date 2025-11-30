// disable clippy for this, since we will be refactoring this into the app later
#![allow(clippy::all)]

mod executor;

use std::{
    fmt::Debug,
    path::PathBuf,
    time::Duration,
};

use cem_solver::{
    DomainDescription,
    Field,
    FieldComponent,
    FieldView,
    SolverBackend,
    SolverInstance,
    Time,
    UpdatePass,
    UpdatePassForcing,
    fdtd::{
        FdtdSolverConfig,
        Resolution,
        cpu::{
            FdtdCpuBackend,
            FdtdCpuSolverInstance,
            MultiThreaded,
            SingleThreaded,
        },
        wgpu::{
            FdtdWgpuBackend,
            FdtdWgpuSolverInstance,
        },
    },
    material::{
        Material,
        PhysicalConstants,
    },
    source::{
        GaussianPulse,
        SourceFunction,
        SourceValues,
    },
};
use cem_util::format_size;
use chrono::Local;
use color_eyre::eyre::Error;
use egui::{
    ColorImage,
    Event,
    UserData,
    ViewportCommand,
};
use egui_plot::{
    Legend,
    Line,
    Plot,
    PlotPoint,
    PlotPoints,
};
use image::RgbaImage;
use nalgebra::{
    Point3,
    Vector3,
};

use crate::fdtd::executor::Executor;

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(long)]
    wgpu: bool,

    #[clap(long, short = 'j')]
    threads: Option<usize>,
}

impl Args {
    pub fn run(self) -> Result<(), Error> {
        eframe::run_native(
            "fdtd",
            Default::default(),
            Box::new(move |cc| {
                let render_state = cc.wgpu_render_state.as_ref().unwrap();

                Ok(Box::new(crate::fdtd::App::new(
                    self,
                    render_state.device.clone(),
                    render_state.queue.clone(),
                )?))
            }),
        )?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct App {
    ticks_per_second: u64,
    executor: Executor,
    screenshots_path: PathBuf,
}

impl App {
    pub fn new(args: Args, device: ::wgpu::Device, queue: ::wgpu::Queue) -> Result<Self, Error> {
        let config = FdtdSolverConfig {
            resolution: Resolution {
                spatial: Vector3::repeat(1.0),
                temporal: 0.25,
            },
            physical_constants: PhysicalConstants::REDUCED,
            size: Vector3::new(500.0, 0.0, 0.0),
        };

        let single_threaded =
            || Box::new(Simulation::cpu_single_threaded(&config)) as Box<dyn ErasedSimulation>;

        let simulation = if args.wgpu {
            Box::new(Simulation::wgpu(&config, device, queue)) as Box<dyn ErasedSimulation>
        }
        else if let Some(num_threads) = args.threads {
            #[cfg(not(feature = "multi-threading"))]
            {
                let _ = num_threads;
                tracing::warn!("Compiled without rayon feature. Falling back to single-threaded");
                single_threaded()
            }

            #[cfg(feature = "multi-threading")]
            {
                let num_threads = (num_threads != 0).then_some(num_threads);
                Box::new(Simulation::cpu_multi_threaded(&config, num_threads)?)
                    as Box<dyn ErasedSimulation>
            }
        }
        else {
            single_threaded()
        };

        let ticks_per_second = 100;
        let executor = Executor::new(simulation, Duration::from_millis(1000 / ticks_per_second));

        Ok(Self {
            ticks_per_second,
            executor,
            screenshots_path: PathBuf::from("screenshots"),
        })
    }

    fn save_screenshot(&self, image: &ColorImage) -> Result<(), Error> {
        if !self.screenshots_path.exists() {
            std::fs::create_dir_all(&self.screenshots_path)?;
        }
        let screenshot_path = self
            .screenshots_path
            .join(format!("{}.png", Local::now().format("%Y-%m-%d_%H:%M:%S")));
        let image = RgbaImage::from_raw(
            image.width() as u32,
            image.height() as u32,
            image.as_raw().to_owned(),
        )
        .expect("Invalid image data provided by egui");
        image.save(&screenshot_path)?;
        Ok(())
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.input(|input| {
            for event in &input.events {
                match event {
                    Event::Screenshot {
                        viewport_id: _,
                        user_data: _,
                        image,
                    } => {
                        self.save_screenshot(&image).unwrap();
                    }
                    _ => {}
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(".").clicked() {
                    self.executor.single_step();
                }
                if ui.button("▶").clicked() {
                    let ctx = ctx.clone();
                    self.executor.start(Some(Box::new(move |_simulation| {
                        ctx.request_repaint();
                    })));
                }
                if ui.button("⏹").clicked() {
                    self.executor.stop();
                }
                if ui.button("⏮").clicked() {
                    let mut guard = self.executor.write();
                    guard.simulation().reset();
                }
                if ui
                    .add(egui::Slider::new(&mut self.ticks_per_second, 1..=200))
                    .changed()
                {
                    self.executor
                        .set_step_interval(Duration::from_millis(1000 / self.ticks_per_second));
                }
                if ui.button("📷").clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::default()));
                }
            });

            let guard = self.executor.read();
            let step_time = guard.step_time().as_millis();
            let simulation = guard.simulation();

            ui.horizontal(|ui| {
                ui.label(simulation.label());
                ui.spacing();
                ui.label(format!("Tick: {}", simulation.tick()));
                ui.spacing();
                ui.label(format!("Time: {:?} s", simulation.time()));
                ui.spacing();
                ui.label(format!("Step time: {step_time} ms"));
            });

            let field_plot = Plot::new("E field").legend(Legend::default());
            field_plot.show(ui, |plot_ui| {
                plot_ui.set_plot_bounds_y(-2.0..=2.0);
                plot_ui.line(Line::new("E", simulation.field_values(FieldComponent::E)));
                plot_ui.line(Line::new("H", simulation.field_values(FieldComponent::H)));
            });
        });
    }
}

trait ErasedSimulation: Time + Debug + Send + Sync + 'static {
    fn update(&mut self);
    fn field_values(&self, field_component: FieldComponent) -> PlotPoints<'static>;
    fn reset(&mut self);
    fn label(&self) -> &str;
}

#[derive(Debug)]
struct Simulation<Instance>
where
    Instance: SolverInstance,
{
    instance: Instance,
    state: Instance::State,
    source: GaussianPulse,
    label: String,
}

impl<Instance> Simulation<Instance>
where
    Instance: SolverInstance,
    Instance::State: Time,
{
    pub fn new<Backend, Config>(backend: &Backend, config: &Config, label: impl ToString) -> Self
    where
        Backend: SolverBackend<Config, Point3<usize>, Instance = Instance>,
        Config: Debug,
    {
        let memory_required = backend
            .memory_required(config)
            .map(|size| format_size(size).to_string())
            .unwrap_or_else(|| "unknown".to_owned());
        tracing::debug!(?config, memory_required,);

        let instance = backend
            .create_instance(config, TestDomainDescription)
            .unwrap();

        let state = instance.create_state();

        let source = GaussianPulse {
            time: 20.0,
            duration: 10.0,
        };

        Self {
            instance,
            state,
            source,
            label: label.to_string(),
        }
    }
}

impl Simulation<FdtdWgpuSolverInstance> {
    pub fn wgpu(config: &FdtdSolverConfig, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let backend = FdtdWgpuBackend::new(device, queue, Default::default());
        Self::new(&backend, config, "wgpu")
    }
}

impl Simulation<FdtdCpuSolverInstance<SingleThreaded>> {
    pub fn cpu_single_threaded(config: &FdtdSolverConfig) -> Self {
        Self::new(
            &FdtdCpuBackend::single_threaded(),
            config,
            "cpu (single-threaded)",
        )
    }
}

#[cfg(feature = "multi-threading")]
impl Simulation<FdtdCpuSolverInstance<MultiThreaded>> {
    pub fn cpu_multi_threaded(
        config: &FdtdSolverConfig,
        num_threads: Option<usize>,
    ) -> Result<Self, cem_solver::rayon::ThreadPoolBuildError> {
        let backend = FdtdCpuBackend::multi_threaded(num_threads)?;
        let label = format!("cpu ({} threads)", backend.num_threads());

        Ok(Self::new(&backend, config, label))
    }
}

impl<Instance> ErasedSimulation for Simulation<Instance>
where
    Instance: SolverInstance + Field<Point3<usize>> + Send + Sync + Debug + 'static,
    Instance::State: Time + Send + Sync + Debug + 'static,
    for<'a> Instance::UpdatePass<'a>: UpdatePassForcing<Point3<usize>>,
{
    fn update(&mut self) {
        let value = self.source.evaluate(self.state.time());

        let mut update_pass = self.instance.begin_update(&mut self.state);
        update_pass.set_forcing(
            &Point3::new(50, 0, 0),
            &SourceValues {
                j: Vector3::y() * value,
                m: Vector3::z() * value,
            },
        );
        update_pass.finish();
    }

    fn field_values(&self, field_component: FieldComponent) -> PlotPoints<'static> {
        let component_index = match field_component {
            FieldComponent::E => 1,
            FieldComponent::H => 2,
        };

        let field = self.instance.field(&self.state, .., field_component);

        PlotPoints::Owned(
            field
                .iter()
                .map(|(x, y)| {
                    // note: casting x like this doesn't account for resolution and offset
                    PlotPoint::new(x.x as f64, y[component_index] as f64)
                })
                .collect::<Vec<_>>(),
        )
    }

    fn reset(&mut self) {
        self.state = self.instance.create_state();
    }

    fn label(&self) -> &str {
        &self.label
    }
}

impl<Instance> Time for Simulation<Instance>
where
    Instance: SolverInstance,
    Instance::State: Time,
{
    fn time(&self) -> f64 {
        self.state.time()
    }

    fn tick(&self) -> usize {
        self.state.tick()
    }
}

struct TestDomainDescription;

impl DomainDescription<Point3<usize>> for TestDomainDescription {
    fn material(&self, point: &Point3<usize>) -> Material {
        let point = point.cast::<f32>();
        let mut material = Material::VACUUM;
        if point.x >= 440.0 && point.x <= 460.0 {
            material.relative_permittivity = 3.9;
        }
        material
    }
}
