use nalgebra::{
    Point3,
    Vector3,
};

pub trait Source: Send + Sync + 'static {
    fn prepare(&mut self, time: f64);
    fn reset(&mut self);
    fn electric_current_density(&mut self, time: f64, point: &Point3<f64>) -> Vector3<f64>;
    fn magnetic_current_density(&mut self, time: f64, point: &Point3<f64>) -> Vector3<f64>;
}

pub struct GaussianPulse {
    pub electric_current_density_amplitude: Vector3<f64>,
    pub magnetic_current_density_amplitude: Vector3<f64>,
    pub time: f64,
    pub duration: f64,
}

impl Source for GaussianPulse {
    fn prepare(&mut self, _time: f64) {}

    fn reset(&mut self) {}

    fn electric_current_density(&mut self, time: f64, _point: &Point3<f64>) -> Vector3<f64> {
        self.electric_current_density_amplitude
            * (-((time - self.time) / self.duration).powi(2)).exp()
    }

    fn magnetic_current_density(&mut self, time: f64, _point: &Point3<f64>) -> Vector3<f64> {
        self.magnetic_current_density_amplitude
            * (-((time - self.time) / self.duration).powi(2)).exp()
    }
}
