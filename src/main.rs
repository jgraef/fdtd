pub mod executor;
pub mod grid;
pub mod simulation;

use std::time::Duration;

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
use nalgebra::Vector3;

use crate::{
    executor::Executor,
    simulation::{
        PhysicalConstants,
        Simulation,
    },
};

fn main() -> Result<(), Error> {
    let _ = dotenv();
    tracing_subscriber::fmt::init();
    color_eyre::install()?;

    let _args = Args::parse();

    let physical_constants = PhysicalConstants::SI;

    let min_wavelength = 100e-9;
    let resolution = physical_constants.estimate_resolution_from_min_wavelength(min_wavelength);

    println!("{physical_constants:#?}");
    println!("{resolution:#?}");

    let simulation = Simulation::new(Vector3::new(500, 1, 1), physical_constants, resolution);

    let executor = Executor::new(simulation);

    eframe::run_native(
        "FDTD",
        Default::default(),
        Box::new(|_cc| Ok(Box::new(FdtdApp { executor }))),
    )
    .map_err(|e| eyre!("{e}"))?;

    Ok(())
}

#[derive(Debug, Parser)]
struct Args {}

#[derive(Debug)]
struct FdtdApp {
    executor: Executor,
}

impl eframe::App for FdtdApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let guard = self.executor.read();
        let simulation = guard.simulation();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(".").clicked() {
                    self.executor.single_step();
                }
                if ui.button("▶").clicked() {
                    let ctx = ctx.clone();
                    self.executor.start(
                        Duration::from_millis(100),
                        Some(Box::new(move |_simulation| {
                            ctx.request_repaint();
                        })),
                    );
                }
                if ui.button("⏹").clicked() {
                    self.executor.stop();
                }
            });
            ui.label(format!("Time: {:?} s", simulation.time()));

            let e_field_plot = Plot::new("E field").legend(Legend::default());
            let e_field = simulation.e_field();
            e_field_plot.show(ui, |plot_ui| {
                plot_ui.set_plot_bounds_y(-1.0..=1.0);
                plot_ui.line(Line::new("E", PlotPoints::from(e_field)));
            });
        });
    }
}
