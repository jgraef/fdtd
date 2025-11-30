use nalgebra::{
    Matrix3,
    Point2,
    Point3,
    RowVector3,
    Vector2,
};

use crate::feec::simplex::half_edge::{
    Boundary,
    Coboundary,
    HalfEdgeMesh,
};

#[derive(Clone, Debug)]
pub struct Vertex {
    pub position: Point3<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Edge {
    pub electric_field: RowVector3<f64>,
    pub epsilon_inv: Matrix3<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Face {
    pub magnetic_field: RowVector3<f64>,
    pub mu_inv: Matrix3<f64>,
}

pub struct Simulation {
    mesh: HalfEdgeMesh<Vertex, Edge, Face>,
    dt: f64,
}

impl Simulation {
    pub fn from_tri_mesh(
        indices: impl IntoIterator<Item = [u32; 3]>,
        vertex: impl Fn(u32) -> Point3<f64>,
    ) -> Self {
        let mesh = HalfEdgeMesh::from_trimesh(
            indices,
            |index| {
                Vertex {
                    position: vertex(index),
                }
            },
            |_| Edge::default(),
            |_| Face::default(),
        );

        Self { mesh, dt: 0.0 }
    }

    pub fn step(&mut self) {
        // update magnetic field
        for face in self.mesh.faces() {
            let de = self
                .mesh
                .boundary(face)
                .map(|edge| self.mesh[edge].electric_field)
                .sum::<RowVector3<f64>>();
            let face = &mut self.mesh[face];
            face.magnetic_field += -self.dt * de * face.mu_inv;
        }

        // update electric field
        for edge in self.mesh.edges() {
            let he = self
                .mesh
                .coboundary(edge)
                .map(|face| self.mesh[face].magnetic_field)
                .sum::<RowVector3<f64>>();
            let edge = &mut self.mesh[edge];

            // todo: this is sigma * e plus some source current density
            let j = RowVector3::zeros();
            edge.electric_field += self.dt * (he - j) * edge.epsilon_inv;
        }
    }
}

fn generate_test_mesh() -> HalfEdgeMesh<Vertex, Edge, Face> {
    let size = Vector2::new(100, 100);

    let xy_to_index = |x, y| y * size.x + x;
    let index_to_xy = |i| Point2::new(i % size.x, i / size.x);

    let faces = (0..size.x).flat_map(|x| {
        (0..size.y).flat_map(move |y| {
            let v = [
                xy_to_index(x, y),
                xy_to_index(x + 1, y),
                xy_to_index(x, y + 1),
                xy_to_index(x + 1, y + 1),
            ];

            [[v[0], v[1], v[2]], [v[1], v[3], v[2]]]
        })
    });

    let epsilon_inv = Matrix3::identity();
    let mu_inv = Matrix3::identity();

    HalfEdgeMesh::from_trimesh(
        faces,
        |i| {
            let r = index_to_xy(i);
            Vertex {
                position: Point3::new(r.x, r.y, 0).cast(),
            }
        },
        |i| {
            let _r = i.map(index_to_xy);

            Edge {
                electric_field: Default::default(),
                epsilon_inv,
            }
        },
        |i| {
            let _r = i.map(index_to_xy);
            Face {
                magnetic_field: Default::default(),
                mu_inv,
            }
        },
    )
}

/*
fn load_obj_file(path: impl AsRef<Path>) -> Result<Simulation, tobj::Error> {
    let (models, _) = tobj::load_obj(path.as_ref(), &Default::default())?;
    let model = models.into_iter().next().ok_or_eyre("no models")?;
    tracing::debug!("loading mesh: {}", model.name);
    assert!(model.mesh.face_arities.is_empty());

    Ok(Simulation::from_tri_mesh(
        model
            .mesh
            .indices
            .chunks_exact(3)
            .map(|indices| indices.try_into().unwrap()),
        |index| {
            Point3::new(
                model.mesh.positions[index as usize * 3],
                model.mesh.positions[index as usize * 3 + 1],
                model.mesh.positions[index as usize * 3 + 2],
            )
            .cast()
        },
    ))
}
*/
