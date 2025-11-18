use std::ops::{
    Index,
    IndexMut,
};

use crate::{
    app::solver::fdtd::Resolution,
    physics::{
        PhysicalConstants,
        material::Material,
    },
};

/// Buffer holding 2 values.
///
/// One value is the current value, the other one is the value from the previous
/// step. Which one is which depends on the [`SwapBufferIndex`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SwapBuffer<T> {
    buffer: [T; 2],
}

impl<T> From<[T; 2]> for SwapBuffer<T> {
    fn from(value: [T; 2]) -> Self {
        Self { buffer: value }
    }
}

impl<T> SwapBuffer<T> {
    pub fn from_fn(mut f: impl FnMut(SwapBufferIndex) -> T) -> Self {
        Self::from(std::array::from_fn::<T, 2, _>(|index| {
            f(SwapBufferIndex { index })
        }))
    }
}

impl<T> Index<SwapBufferIndex> for SwapBuffer<T> {
    type Output = T;

    fn index(&self, index: SwapBufferIndex) -> &Self::Output {
        &self.buffer[index.index]
    }
}

impl<T> IndexMut<SwapBufferIndex> for SwapBuffer<T> {
    fn index_mut(&mut self, index: SwapBufferIndex) -> &mut Self::Output {
        &mut self.buffer[index.index]
    }
}

/// Index into a [`SwapBuffer`].
///
/// This can be derived from the simulation tick.
#[derive(Clone, Copy, Debug)]
pub struct SwapBufferIndex {
    index: usize,
}

impl SwapBufferIndex {
    pub fn from_tick(tick: usize) -> Self {
        Self { index: tick % 2 }
    }

    pub fn other(&self) -> Self {
        Self {
            index: (self.index + 1) % 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateCoefficients {
    pub c_a: f64,
    pub c_b: f64,
    pub d_a: f64,
    pub d_b: f64,
}

impl UpdateCoefficients {
    pub fn new(
        resolution: &Resolution,
        physical_constants: &PhysicalConstants,
        material: &Material,
    ) -> Self {
        let c_or_d = |perm, sigma| {
            let half_sigmal_delta_t_over_perm = 0.5 * sigma * resolution.temporal / perm;

            let a: f64 =
                (1.0 - half_sigmal_delta_t_over_perm) / (1.0 + half_sigmal_delta_t_over_perm);
            let b: f64 = resolution.temporal / (perm * (1.0 + half_sigmal_delta_t_over_perm));

            assert!(!a.is_nan());
            assert!(!b.is_nan());

            (a, b)
        };

        let (c_a, c_b) = c_or_d(
            material.relative_permittivity * physical_constants.vacuum_permittivity,
            material.eletrical_conductivity,
        );
        let (d_a, d_b) = c_or_d(
            material.relative_permeability * physical_constants.vacuum_permeability,
            material.magnetic_conductivity,
        );

        Self { c_a, c_b, d_a, d_b }
    }
}
