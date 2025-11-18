use std::f64::consts::TAU;

use nalgebra::{
    Point3,
    Vector3,
};

pub trait Source: Send + Sync + 'static {
    fn prepare(&mut self, time: f64) {
        let _ = time;
    }

    fn reset(&mut self) {}

    fn electric_current_density(&mut self, time: f64, point: &Point3<f64>) -> Vector3<f64>;
    fn magnetic_current_density(&mut self, time: f64, point: &Point3<f64>) -> Vector3<f64>;
}

#[derive(Clone, Copy, Debug)]
pub struct GaussianPulse {
    pub electric_current_density_amplitude: Vector3<f64>,
    pub magnetic_current_density_amplitude: Vector3<f64>,
    pub time: f64,
    pub duration: f64,
}

impl Source for GaussianPulse {
    fn electric_current_density(&mut self, time: f64, _point: &Point3<f64>) -> Vector3<f64> {
        self.electric_current_density_amplitude
            * (-((time - self.time) / self.duration).powi(2)).exp()
    }

    fn magnetic_current_density(&mut self, time: f64, _point: &Point3<f64>) -> Vector3<f64> {
        self.magnetic_current_density_amplitude
            * (-((time - self.time) / self.duration).powi(2)).exp()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ContinousWave {
    pub electric_current_density_amplitude: Vector3<f64>,
    pub magnetic_current_density_amplitude: Vector3<f64>,
    pub electric_current_density_phase: f64,
    pub magnetic_current_density_phase: f64,
    pub frequency: f64,
}

impl Source for ContinousWave {
    fn electric_current_density(&mut self, time: f64, _point: &Point3<f64>) -> Vector3<f64> {
        self.electric_current_density_amplitude
            * (TAU * self.frequency * time + self.electric_current_density_phase).cos()
    }

    fn magnetic_current_density(&mut self, time: f64, _point: &Point3<f64>) -> Vector3<f64> {
        self.magnetic_current_density_amplitude
            * (TAU * self.frequency * time + self.magnetic_current_density_phase).cos()
    }
}
