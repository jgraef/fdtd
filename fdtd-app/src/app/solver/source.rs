use std::{
    f64::consts::TAU,
    fmt::Debug,
    sync::Arc,
};

use nalgebra::Vector3;

#[derive(Clone, Copy, Debug, Default)]
pub struct SourceValues {
    pub j: Vector3<f64>,
    pub m: Vector3<f64>,
}

pub trait SourceFunction: Debug + Send + Sync + 'static {
    type Output;

    fn evaluate(&self, time: f64) -> Self::Output;
}

pub trait ScalarSourceFunctionExt: SourceFunction<Output = f64> {
    fn with_amplitudes(self, j: Vector3<f64>, m: Vector3<f64>) -> WithAmplitudes<Self>
    where
        Self: Sized,
    {
        WithAmplitudes {
            amplitude: SourceValues { j, m },
            inner: self,
        }
    }
}

impl<T> ScalarSourceFunctionExt for T where T: SourceFunction<Output = f64> {}

//pub trait SourceFunctionExt: SourceFunction {}
//impl<T> SourceFunctionExt for T where T: SourceFunction {}

#[derive(Clone, Copy, Debug)]
pub struct GaussianPulse {
    pub time: f64,
    pub duration: f64,
}

impl GaussianPulse {
    pub fn new(time: f64, duration: f64) -> Self {
        Self { time, duration }
    }
}

impl SourceFunction for GaussianPulse {
    type Output = f64;

    fn evaluate(&self, time: f64) -> f64 {
        (-((time - self.time) / self.duration).powi(2)).exp()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ContinousWave {
    pub phase: f64,
    pub frequency: f64,
}

impl ContinousWave {
    pub fn new(phase: f64, frequency: f64) -> Self {
        Self { phase, frequency }
    }
}

impl SourceFunction for ContinousWave {
    type Output = f64;

    fn evaluate(&self, time: f64) -> f64 {
        (TAU * self.frequency * time + self.phase).cos()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WithAmplitudes<F> {
    pub amplitude: SourceValues,
    pub inner: F,
}

impl<F> SourceFunction for WithAmplitudes<F>
where
    F: SourceFunction<Output = f64>,
{
    type Output = SourceValues;

    fn evaluate(&self, time: f64) -> Self::Output {
        let value = self.inner.evaluate(time);
        SourceValues {
            j: self.amplitude.j * value,
            m: self.amplitude.m * value,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Source(pub Arc<dyn SourceFunction<Output = SourceValues>>);

impl<F> From<F> for Source
where
    F: SourceFunction<Output = SourceValues>,
{
    fn from(value: F) -> Self {
        Source(Arc::new(value))
    }
}
