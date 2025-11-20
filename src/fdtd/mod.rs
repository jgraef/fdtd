// disable clippy for this, since we will be refactoring this into the app later
#![allow(clippy::all)]

mod executor;

use std::{
    path::PathBuf,
    time::Duration,
};

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
    Isometry3,
    Point3,
    UnitQuaternion,
    Vector3,
};

use crate::{
    app::solver::{
        fdtd::{
            self,
            AccessFieldRegion,
            FdtdSolverConfig,
            FieldComponent,
            Resolution,
            SourceValues,
            legacy::{
                Simulation,
                estimate_memory_usage,
                geometry::Block,
                source::{
                    GaussianPulse,
                    Source,
                },
            },
        },
        traits::{
            DomainDescription,
            SolverBackend,
            SolverInstance,
        },
    },
    fdtd::executor::Executor,
    physics::{
        PhysicalConstants,
        material::Material,
    },
};

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[clap(long)]
    wgpu: bool,
    #[clap(long)]
    cpu_legacy: bool,
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
                )))
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
    backend_label: &'static str,
}

impl App {
    pub fn new(args: Args, device: ::wgpu::Device, queue: ::wgpu::Queue) -> Self {
        let config = FdtdSolverConfig {
            resolution: Resolution {
                spatial: Vector3::repeat(1.0),
                temporal: 0.25,
            },
            physical_constants: PhysicalConstants::REDUCED,
            size: Vector3::new(500.0, 0.0, 0.0),
        };
        tracing::debug!(?config, memory_usage = estimate_memory_usage(&config));

        let (simulation, backend_label) = if args.wgpu {
            (CpuOrGpu::new_gpu(&config, &device, &queue), "wgpu")
        }
        else if args.cpu_legacy {
            (CpuOrGpu::new_cpu_legacy(&config), "cpu-legacy")
        }
        else {
            (CpuOrGpu::new_cpu(&config), "cpu")
        };

        let ticks_per_second = 100;
        let executor = Executor::new(simulation, Duration::from_millis(1000 / ticks_per_second));

        Self {
            ticks_per_second,
            executor,
            screenshots_path: PathBuf::from("screenshots"),
            backend_label,
        }
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

                ui.label(self.backend_label);
            });

            let guard = self.executor.read();
            let step_time = guard.step_time().as_millis();
            let simulation = guard.simulation();

            ui.horizontal(|ui| {
                ui.label(format!("Tick: {}", simulation.tick()));
                ui.spacing();
                ui.label(format!("Time: {:?} s", simulation.time()));
                ui.spacing();
                ui.label(format!("Total energy: {}", simulation.total_energy()));
                ui.spacing();
                ui.label(format!("Step time: {step_time} ms"));
            });

            let field_plot = Plot::new("E field").legend(Legend::default());
            field_plot.show(ui, |plot_ui| {
                plot_ui.set_plot_bounds_y(-2.0..=2.0);
                plot_ui.line(Line::new(
                    "E",
                    simulation.field_values(WhichFieldValue::Electric),
                ));
                plot_ui.line(Line::new(
                    "H",
                    simulation.field_values(WhichFieldValue::Magnetic),
                ));
                plot_ui.line(Line::new(
                    "ε_r",
                    simulation.field_values(WhichFieldValue::Epsilon),
                ))
            });
        });
    }
}

fn gaussian_pulse() -> GaussianPulse {
    GaussianPulse {
        electric_current_density_amplitude: Vector3::y(),
        magnetic_current_density_amplitude: Vector3::z(),
        time: 20.0,
        duration: 10.0,
    }
}

fn create_sources(time: f64) -> [(Point3<usize>, SourceValues); 1] {
    let point = Point3::new(50, 0, 0);

    let mut gaussian_pulse = gaussian_pulse();
    let j_source = gaussian_pulse.electric_current_density(time, &point.cast());
    let m_source = gaussian_pulse.magnetic_current_density(time, &point.cast());

    [(point, SourceValues { j_source, m_source })]
}

#[derive(Debug)]
enum CpuOrGpu {
    CpuLegacy {
        simulation: Simulation,
    },
    Cpu {
        instance: fdtd::cpu::FdtdCpuSolverInstance,
        state: fdtd::cpu::FdtdCpuSolverState,
    },
    Gpu {
        instance: fdtd::wgpu::FdtdWgpuSolverInstance,
        state: fdtd::wgpu::FdtdWgpuSolverState,
    },
}

impl CpuOrGpu {
    pub fn new_cpu_legacy(config: &FdtdSolverConfig) -> Self {
        let mut simulation = Simulation::new(&config);

        simulation.add_material(
            Block {
                transform: Isometry3::from_parts(
                    Vector3::new(450.0, 0.0, 0.0).into(),
                    UnitQuaternion::identity(),
                ),
                dimensions: Vector3::new(20.0, 0.0, 0.0),
            },
            Material {
                relative_permittivity: 3.9,
                ..Default::default()
            },
        );

        simulation.add_source(Point3::new(50.0, 0.0, 0.0), gaussian_pulse());

        Self::CpuLegacy { simulation }
    }

    pub fn new_cpu(config: &FdtdSolverConfig) -> Self {
        let instance = fdtd::cpu::FdtdCpuBackend
            .create_instance(&config, TestDomainDescription)
            .unwrap();
        let state = instance.create_state();
        Self::Cpu { instance, state }
    }

    pub fn new_gpu(
        config: &FdtdSolverConfig,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
    ) -> Self {
        let solver = fdtd::wgpu::FdtdWgpuBackend::new(device, queue);

        let instance = solver
            .create_instance(&config, TestDomainDescription)
            .unwrap();
        let state = instance.create_state();
        Self::Gpu { instance, state }
    }

    pub fn step(&mut self) {
        match self {
            CpuOrGpu::CpuLegacy { simulation } => simulation.step(),
            CpuOrGpu::Cpu { instance, state } => {
                instance.update(state, create_sources(state.time()))
            }
            CpuOrGpu::Gpu { instance, state } => {
                instance.update(state, create_sources(state.time()))
            }
        }
    }

    pub fn reset(&mut self) {
        match self {
            CpuOrGpu::CpuLegacy { simulation } => simulation.reset(),
            CpuOrGpu::Cpu {
                instance, state, ..
            } => {
                *state = instance.create_state();
            }
            CpuOrGpu::Gpu {
                instance, state, ..
            } => {
                *state = instance.create_state();
            }
        }
    }

    pub fn tick(&self) -> usize {
        match self {
            CpuOrGpu::CpuLegacy { simulation } => simulation.tick(),
            CpuOrGpu::Cpu { state, .. } => state.tick(),
            CpuOrGpu::Gpu { state, .. } => state.tick(),
        }
    }

    pub fn time(&self) -> f64 {
        match self {
            CpuOrGpu::CpuLegacy { simulation } => simulation.time(),
            CpuOrGpu::Cpu { state, .. } => state.time(),
            CpuOrGpu::Gpu { state, .. } => state.time(),
        }
    }

    pub fn total_energy(&self) -> f64 {
        match self {
            CpuOrGpu::CpuLegacy { simulation } => simulation.total_energy(),
            CpuOrGpu::Gpu { .. } | CpuOrGpu::Cpu { .. } => {
                // todo
                0.0
            }
        }
    }

    pub fn field_values(&self, which: WhichFieldValue) -> PlotPoints<'static> {
        let x_correction = match which {
            WhichFieldValue::Electric => 0.5,
            WhichFieldValue::Magnetic => 0.0,
            WhichFieldValue::Epsilon => 0.5,
        };

        match self {
            CpuOrGpu::CpuLegacy { simulation } => {
                let get_value = |cell: &fdtd::legacy::simulation::Cell, swap_buffer_index| {
                    match which {
                        WhichFieldValue::Electric => cell.electric_field(swap_buffer_index).y,
                        WhichFieldValue::Magnetic => cell.magnetic_field(swap_buffer_index).z,
                        WhichFieldValue::Epsilon => cell.material().relative_permittivity,
                    }
                };

                PlotPoints::Owned(
                    simulation
                        .field_values(
                            Point3::origin(),
                            fdtd::cpu::Axis::X,
                            x_correction,
                            get_value,
                        )
                        .map(|(x, y)| PlotPoint::new(x, y))
                        .collect::<Vec<_>>(),
                )
            }
            CpuOrGpu::Cpu { instance, state } => {
                let (field_component, component_index) = match which {
                    WhichFieldValue::Electric => (FieldComponent::E, 1),
                    WhichFieldValue::Magnetic => (FieldComponent::H, 2),
                    WhichFieldValue::Epsilon => return PlotPoints::Owned(vec![]),
                };

                PlotPoints::Owned(
                    instance
                        .read_state(state, &AccessFieldRegion::new(.., field_component))
                        .map(|(x, y)| {
                            // note: casting x like this doesn't account for resolution and offset
                            PlotPoint::new(x.x as f64, y[component_index] as f64)
                        })
                        .collect::<Vec<_>>(),
                )
            }
            CpuOrGpu::Gpu { instance, state } => {
                let (field_component, component_index) = match which {
                    WhichFieldValue::Electric => (FieldComponent::E, 1),
                    WhichFieldValue::Magnetic => (FieldComponent::H, 2),
                    WhichFieldValue::Epsilon => return PlotPoints::Owned(vec![]),
                };

                PlotPoints::Owned(
                    instance
                        .read_state(state, &AccessFieldRegion::new(.., field_component))
                        .map(|(x, y)| {
                            // note: casting x like this doesn't account for resolution and offset
                            PlotPoint::new(x.x as f64, y[component_index] as f64)
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WhichFieldValue {
    Electric,
    Magnetic,
    Epsilon,
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
