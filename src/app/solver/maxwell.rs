use std::ops::RangeBounds;

use nalgebra::Vector3;

use crate::app::solver::traits::SolverInstance;

pub trait Field: SolverInstance {
    type Iter<'a>: Iterator<Item = (Self::Point, Vector3<f64>)>
    where
        Self: 'a;

    fn field<'a, R>(
        &'a self,
        state: &'a Self::State,
        range: R,
        field_component: FieldComponent,
    ) -> Self::Iter<'a>
    where
        R: RangeBounds<Self::Point>;
}

// todo: remove. this is not good. we can't always guarantuee that we can hand
// out `&mut Vector3<f64>`s
pub trait FieldMut: SolverInstance {
    type IterMut<'a>: Iterator<Item = (Self::Point, &'a mut Vector3<f64>)>
    where
        Self: 'a;

    fn field_mut<'a, R>(
        &'a self,
        state: &'a mut Self::State,
        range: R,
        field_component: FieldComponent,
    ) -> Self::IterMut<'a>
    where
        R: RangeBounds<Self::Point>;
}

#[derive(Clone, Copy, Debug)]
pub enum FieldComponent {
    E,
    H,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SourceValues {
    pub j_source: Vector3<f64>,
    pub m_source: Vector3<f64>,
}
