pub mod executor;
pub mod grid;
pub mod simulation;

use std::{
    f64::consts::TAU,
    sync::Arc,
    time::Duration,
};

use clap::Parser;
use color_eyre::eyre::{
    Error,
    eyre,
};
use dotenvy::dotenv;
use egui_plot::{
    Legend,
    Line,
    Plot,
    PlotPoints,
};
use nalgebra::{
    Point3,
    Vector3,
};

use crate::{
    executor::Executor,
    simulation::{
        ElectricPointForcingFunction,
        NullForcing,
        PhysicalConstants,
        Resolution,
        Simulation,
    },
};

fn main() -> Result<(), Error> {
    let _ = dotenv();
    tracing_subscriber::fmt::init();
    color_eyre::install()?;

    let _args = Args::parse();

    eframe::run_native(
        "FDTD",
        Default::default(),
        Box::new(|_cc| Ok(Box::new(FdtdApp::new()))),
    )
    .map_err(|e| eyre!("{e}"))?;

    Ok(())
}

#[derive(Debug, Parser)]
struct Args {}

#[derive(Debug)]
struct FdtdApp {
    ticks_per_second: u64,
    executor: Executor,
}

impl FdtdApp {
    pub fn new() -> Self {
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

        /*let forcing = {
            let amplitude = 1.0;
            let frequency = physical_constants.wavelength_to_frequency(500e-9);
            let duration = 20.0 / frequency;
            let peak = duration;
            println!("peak={peak:?}, duration={duration:?}");

            ElectricPointForcingFunction {
                point: Point3::new(250, 0, 0),
                f: move |time: f64| {
                    let time = time - peak;
                    let gaussian = (-(time / duration).powi(2)).exp();
                    //let sine = (TAU * time * frequency).sin();
                    //let signal = amplitude * gaussian * sine;
                    //println!("{gaussian} * {sine}");
                    //signal
                    gaussian
                },
            }
        };*/
        let forcing = NullForcing;

        let simulation = Simulation::new(
            Vector3::new(500, 1, 1),
            physical_constants,
            resolution,
            Arc::new(forcing),
        );

        let ticks_per_second = 100;
        let executor = Executor::new(simulation, Duration::from_millis(1000 / ticks_per_second));

        Self {
            ticks_per_second,
            executor,
        }
    }
}

impl eframe::App for FdtdApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                plot_ui.line(Line::new("E", PlotPoints::from(simulation.e_field())));
                plot_ui.line(Line::new("H", PlotPoints::from(simulation.h_field())));
                plot_ui.line(Line::new("ε_r", PlotPoints::from(simulation.epsilon())))
            });
        });
    }
}
