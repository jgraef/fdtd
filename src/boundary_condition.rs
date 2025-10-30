use nalgebra::Vector3;

// todo: document. this basically calculates the central difference derivate
// given some of the two field values.
pub trait BoundaryCondition {
    fn apply_df(&self, f0: Option<Vector3<f64>>, f1: Option<Vector3<f64>>) -> Vector3<f64>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DirichletBoundaryCondition {
    pub alpha: Vector3<f64>,
    pub beta: Vector3<f64>,
}

impl BoundaryCondition for DirichletBoundaryCondition {
    fn apply_df(&self, f0: Option<Vector3<f64>>, f1: Option<Vector3<f64>>) -> Vector3<f64> {
        let f0 = f0.unwrap_or(self.alpha);
        let f1 = f1.unwrap_or(self.beta);
        f1 - f0
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NeumannBoundaryCondition {
    pub alpha: Vector3<f64>,
    pub beta: Vector3<f64>,
}

impl BoundaryCondition for NeumannBoundaryCondition {
    fn apply_df(&self, f0: Option<Vector3<f64>>, f1: Option<Vector3<f64>>) -> Vector3<f64> {
        match (f0, f1) {
            (Some(f0), Some(f1)) => f1 - f0,
            (None, _) => self.alpha,
            (_, None) => self.beta,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum AnyBoundaryCondition {
    Dirichlet(DirichletBoundaryCondition),
    Neumann(NeumannBoundaryCondition),
}

impl BoundaryCondition for AnyBoundaryCondition {
    fn apply_df(&self, f0: Option<Vector3<f64>>, f1: Option<Vector3<f64>>) -> Vector3<f64> {
        match self {
            AnyBoundaryCondition::Dirichlet(dirichlet_boundary_condition) => {
                dirichlet_boundary_condition.apply_df(f0, f1)
            }
            AnyBoundaryCondition::Neumann(neumann_boundary_condition) => {
                neumann_boundary_condition.apply_df(f0, f1)
            }
        }
    }
}

impl From<DirichletBoundaryCondition> for AnyBoundaryCondition {
    fn from(value: DirichletBoundaryCondition) -> Self {
        Self::Dirichlet(value)
    }
}

impl From<NeumannBoundaryCondition> for AnyBoundaryCondition {
    fn from(value: NeumannBoundaryCondition) -> Self {
        Self::Neumann(value)
    }
}

pub fn default_boundary_conditions(_dimensions: &Vector3<usize>) -> [AnyBoundaryCondition; 3] {
    /*dimensions.data.0[0].map(|n| {
        if n > 1 {
            AnyBoundaryCondition::from(DirichletBoundaryCondition::default())
        }
        else {
            AnyBoundaryCondition::from(NeumannBoundaryCondition::default())
        }
    })*/
    [AnyBoundaryCondition::from(NeumannBoundaryCondition::default()); 3]
}
