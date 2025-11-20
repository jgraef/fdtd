use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug)]
pub struct GaussianPulse {
    pub time: f64,
    pub duration: f64,
}

impl GaussianPulse {
    pub fn evaluate(&self, time: f64) -> f64 {
        (-((time - self.time) / self.duration).powi(2)).exp()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ContinousWave {
    pub phase: f64,
    pub frequency: f64,
}

impl ContinousWave {
    pub fn evaluate(&self, time: f64) -> f64 {
        (TAU * self.frequency * time + self.phase).cos()
    }
}
