pub mod boundary_condition;
pub mod executor;
pub mod geometry;
pub mod lattice;
pub mod material;
pub mod pml;
pub mod simulation;
pub mod source;
mod util;

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
    CreateAppContext,
    fdtd::{
        executor::Executor,
        geometry::Block,
        material::Material,
        simulation::{
            Axis,
            PhysicalConstants,
            Resolution,
            Simulation,
        },
        source::GaussianPulse,
    },
};

#[derive(Debug)]
pub struct FdtdApp {
    ticks_per_second: u64,
    executor: Executor,
    screenshots_path: PathBuf,
}

impl FdtdApp {
    pub fn new(_context: CreateAppContext) -> Self {
        //let physical_constants = PhysicalConstants::SI;
        let physical_constants = PhysicalConstants::REDUCED;

        //let min_wavelength = 400e-9;
        //let resolution =
        // physical_constants.estimate_resolution_from_min_wavelength(min_wavelength);
        let resolution = Resolution {
            spatial: Vector3::repeat(1.0),
            temporal: 0.25,
        };

        println!("{physical_constants:#?}");
        println!("{resolution:#?}");

        let mut simulation = Simulation::new(
            Vector3::new(500.0, 0.0, 0.0),
            physical_constants,
            resolution,
        );

        println!("Memory usage: {}", simulation.memory_usage_estimate());

        simulation.add_material(
            Block {
                transform: Isometry3::from_parts(
                    Vector3::new(200.0, 0.0, 0.0).into(),
                    UnitQuaternion::identity(),
                ),
                dimensions: Vector3::new(20.0, 0.0, 0.0),
            },
            Material {
                relative_permittivity: 3.9,
                ..Default::default()
            },
        );

        simulation.add_source(
            Point3::new(-200.0, 0.0, 0.0),
            GaussianPulse {
                electric_current_density_amplitude: Vector3::y(),
                magnetic_current_density_amplitude: Vector3::z(),
                time: 20.0,
                duration: 10.0,
            },
        );

        let ticks_per_second = 100;
        let executor = Executor::new(simulation, Duration::from_millis(1000 / ticks_per_second));

        Self {
            ticks_per_second,
            executor,
            screenshots_path: PathBuf::from("screenshots"),
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

impl eframe::App for FdtdApp {
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
            let simulation = guard.simulation();

            ui.horizontal(|ui| {
                ui.label(format!("Tick: {}", simulation.tick()));
                ui.spacing();
                ui.label(format!("Time: {:?} s", simulation.time()));
            });
            ui.label(format!("Total energy: {}", simulation.total_energy()));

            let field_plot = Plot::new("E field").legend(Legend::default());
            field_plot.show(ui, |plot_ui| {
                plot_ui.set_plot_bounds_y(-2.0..=2.0);
                plot_ui.line(Line::new(
                    "E",
                    PlotPoints::Owned(
                        simulation
                            .field_values(
                                Point3::origin(),
                                Axis::X,
                                0.5,
                                |cell, swap_buffer_index| cell.electric_field(swap_buffer_index).y,
                            )
                            .map(|(x, y)| PlotPoint::new(x, y))
                            .collect::<Vec<_>>(),
                    ),
                ));
                plot_ui.line(Line::new(
                    "H",
                    PlotPoints::Owned(
                        simulation
                            .field_values(
                                Point3::origin(),
                                Axis::X,
                                0.0,
                                |cell, swap_buffer_index| cell.magnetic_field(swap_buffer_index).z,
                            )
                            .map(|(x, y)| PlotPoint::new(x, y))
                            .collect::<Vec<_>>(),
                    ),
                ));
                plot_ui.line(Line::new(
                    "ε_r",
                    PlotPoints::Owned(
                        simulation
                            .field_values(Point3::origin(), Axis::X, 0.5, |cell, _| {
                                cell.material().relative_permittivity
                            })
                            .map(|(x, y)| PlotPoint::new(x, y))
                            .collect::<Vec<_>>(),
                    ),
                ))
            });
        });
    }
}
