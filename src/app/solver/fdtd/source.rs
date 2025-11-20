use std::{
    f64::consts::TAU,
    fmt::Debug,
};

#[derive(Clone, Copy, Debug)]
pub struct GaussianPulse {
    pub time: f64,
    pub duration: f64,
}

impl SourceFunction for GaussianPulse {
    fn evaluate(&self, time: f64) -> f64 {
        (-((time - self.time) / self.duration).powi(2)).exp()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ContinousWave {
    pub phase: f64,
    pub frequency: f64,
}

impl SourceFunction for ContinousWave {
    fn evaluate(&self, time: f64) -> f64 {
        (TAU * self.frequency * time + self.phase).cos()
    }
}

pub trait SourceFunction: Debug + Send + Sync + 'static {
    fn evaluate(&self, time: f64) -> f64;
}
