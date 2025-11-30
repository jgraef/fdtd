use std::{
    collections::HashMap,
    fmt::Debug,
    marker::PhantomData,
    ops::{
        Index,
        IndexMut,
        Neg,
    },
};

use arrayvec::ArrayVec;
use num::One;

#[derive(Clone, Debug)]
pub struct HalfEdgeMesh<V, E, F> {
    half_edges: Vec<HalfEdge>,
    vertices: Vec<Record<V>>,
    edges: Vec<Record<E>>,
    faces: Vec<Record<F>>,
}

#[derive(Clone, Debug)]
struct Record<T> {
    half_edge: u32,
    data: T,
}

#[derive(Clone, Debug)]
struct HalfEdge {
    twin: u32,
    next: u32,
    previous: u32,
    vertex: u32,
    edge: u32,
    face: u32,
}

macro_rules! make_handle {
    ($name:ident) => {
        pub struct $name<T> {
            pub index: u32,
            _phantom: PhantomData<fn(&T)>,
        }

        impl<T> $name<T> {
            pub fn new(index: u32) -> Self {
                Self {
                    index,
                    _phantom: PhantomData,
                }
            }
        }

        impl<T> Clone for $name<T> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<T> Copy for $name<T> {}

        impl<T> Debug for $name<T> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("index", &self.index)
                    .finish()
            }
        }
    };
}

make_handle!(VertexHandle);
make_handle!(EdgeHandle);
make_handle!(FaceHandle);

impl<V, E, F> HalfEdgeMesh<V, E, F> {
    pub fn from_trimesh(
        triangles: impl IntoIterator<Item = [u32; 3]>,
        mut vertex: impl FnMut(u32) -> V,
        mut edge: impl FnMut([u32; 2]) -> E,
        mut face: impl FnMut([u32; 3]) -> F,
    ) -> Self {
        let mut half_edges: Vec<HalfEdge> = vec![];
        let mut vertices: Vec<Record<V>> = vec![];
        let mut edges: Vec<Record<E>> = vec![];
        let mut faces: Vec<Record<F>> = vec![];

        let mut vertex_map: HashMap<u32, u32> = HashMap::new();
        let mut edge_map: HashMap<[u32; 2], u32> = HashMap::new();

        for triangle in triangles {
            let first_half_edge_index = half_edges.len();

            let face_index = faces.len() as u32;
            faces.push(Record {
                half_edge: first_half_edge_index as u32,
                data: face(triangle),
            });

            for i in 0..3 {
                let vertex_index = *vertex_map.entry(triangle[i]).or_insert_with(|| {
                    let vertex_index = vertices.len();
                    vertices.push(Record {
                        half_edge: (first_half_edge_index + i) as u32,
                        data: vertex(triangle[i]),
                    });
                    vertex_index as u32
                });

                let j = (i + 1) % 3;
                let edge_vertex_indices = [triangle[i], triangle[j]];
                let twin_vertex_indices = [triangle[j], triangle[i]];

                let (edge_index, twin) =
                    if let Some(edge_index) = edge_map.get(&edge_vertex_indices) {
                        let twin = edges[*edge_index as usize].half_edge;
                        assert_eq!(half_edges[twin as usize].twin, u32::MAX);
                        half_edges[twin as usize].twin = *edge_index;
                        (*edge_index, twin)
                    }
                    else {
                        let edge_index = edges.len() as u32;
                        edges.push(Record {
                            half_edge: (first_half_edge_index + i) as u32,
                            data: edge(edge_vertex_indices),
                        });
                        edge_map.insert(edge_vertex_indices, edge_index);
                        edge_map.insert(twin_vertex_indices, edge_index);
                        (edge_index, u32::MAX)
                    };

                half_edges.push(HalfEdge {
                    twin,
                    next: (first_half_edge_index + j) as u32,
                    previous: (first_half_edge_index + (i + 2) % 3) as u32,
                    vertex: vertex_index,
                    edge: edge_index,
                    face: face_index,
                });
            }
        }

        Self {
            half_edges,
            vertices,
            edges,
            faces,
        }
    }

    pub fn vertices(&self) -> impl Iterator<Item = VertexHandle<V>> + 'static {
        (0..self.vertices.len()).map(|index| VertexHandle::new(index as u32))
    }

    pub fn edges(&self) -> impl Iterator<Item = EdgeHandle<E>> + 'static {
        (0..self.edges.len()).map(|index| EdgeHandle::new(index as u32))
    }

    pub fn faces(&self) -> impl Iterator<Item = FaceHandle<F>> + 'static {
        (0..self.faces.len()).map(|index| FaceHandle::new(index as u32))
    }
}

impl<V, E, F> Index<VertexHandle<V>> for HalfEdgeMesh<V, E, F> {
    type Output = V;

    fn index(&self, index: VertexHandle<V>) -> &Self::Output {
        &self.vertices[index.index as usize].data
    }
}

impl<V, E, F> IndexMut<VertexHandle<V>> for HalfEdgeMesh<V, E, F> {
    fn index_mut(&mut self, index: VertexHandle<V>) -> &mut Self::Output {
        &mut self.vertices[index.index as usize].data
    }
}

impl<V, E, F> Index<EdgeHandle<E>> for HalfEdgeMesh<V, E, F> {
    type Output = E;

    fn index(&self, index: EdgeHandle<E>) -> &Self::Output {
        &self.edges[index.index as usize].data
    }
}

impl<V, E, F> IndexMut<EdgeHandle<E>> for HalfEdgeMesh<V, E, F> {
    fn index_mut(&mut self, index: EdgeHandle<E>) -> &mut Self::Output {
        &mut self.edges[index.index as usize].data
    }
}

impl<V, E, F> Index<FaceHandle<F>> for HalfEdgeMesh<V, E, F> {
    type Output = F;

    fn index(&self, index: FaceHandle<F>) -> &Self::Output {
        &self.faces[index.index as usize].data
    }
}

impl<V, E, F> IndexMut<FaceHandle<F>> for HalfEdgeMesh<V, E, F> {
    fn index_mut(&mut self, index: FaceHandle<F>) -> &mut Self::Output {
        &mut self.faces[index.index as usize].data
    }
}

pub trait Coboundary<P> {
    type Q;

    fn coboundary(&self, simplex: P) -> impl Iterator<Item = Self::Q>;
}

pub trait Boundary<P> {
    type Q;

    fn boundary(&self, simplex: P) -> impl Iterator<Item = Self::Q>;
}

impl<V, E, F> Coboundary<EdgeHandle<E>> for HalfEdgeMesh<V, E, F> {
    type Q = FaceHandle<F>;

    fn coboundary(&self, simplex: EdgeHandle<E>) -> impl Iterator<Item = Self::Q> {
        let half_edge = self.edges[simplex.index as usize].half_edge;
        // can this index be u32::MAX?

        let mut faces: ArrayVec<_, 2> = ArrayVec::new();

        let half_edge = &self.half_edges[half_edge as usize];
        if half_edge.face != u32::MAX {
            faces.push(half_edge.face);
        }

        if half_edge.twin != u32::MAX {
            let twin = &self.half_edges[half_edge.twin as usize];
            if twin.face != u32::MAX {
                faces.push(twin.face);
            }
        }
        else {
            todo!("coboundary of a edge that does only have one half-edge?");
        }

        faces.into_iter().map(FaceHandle::new)
    }
}

impl<V, E, F> Boundary<FaceHandle<F>> for HalfEdgeMesh<V, E, F> {
    type Q = EdgeHandle<E>;

    fn boundary(&self, simplex: FaceHandle<F>) -> impl Iterator<Item = Self::Q> {
        let half_edge = self.faces[simplex.index as usize].half_edge;
        // can this index be u32::MAX?

        let half_edge = &self.half_edges[half_edge as usize];

        [
            self.half_edges[half_edge.previous as usize].edge,
            half_edge.edge,
            self.half_edges[half_edge.next as usize].edge,
        ]
        .into_iter()
        .map(EdgeHandle::new)
    }
}

impl<V, E, F> Oriented<EdgeHandle<E>> for HalfEdgeMesh<V, E, F> {
    fn orientation(&self, simplex: EdgeHandle<E>) -> Orientation {
        let half_edge_index = self.faces[simplex.index as usize].half_edge;
        // can this index be u32::MAX?

        let half_edge = &self.half_edges[half_edge_index as usize];

        let twin_index = half_edge.twin;
        if twin_index != u32::MAX {
            Orientation::from_bool_canonical(half_edge_index < twin_index)
        }
        else {
            todo!("orientation of a edge that only has one half-edge")
        }
    }
}

pub trait Oriented<S> {
    fn orientation(&self, simplex: S) -> Orientation;
}

#[derive(Clone, Copy, Debug)]
pub enum Orientation {
    Positive,
    Negative,
}

impl Orientation {
    pub fn from_bool_canonical(value: bool) -> Self {
        if value {
            Self::Positive
        }
        else {
            Self::Negative
        }
    }

    pub fn sign<T>(&self) -> T
    where
        T: One + Neg<Output = T>,
    {
        match self {
            Self::Positive => T::one(),
            Self::Negative => T::neg(T::one()),
        }
    }

    pub fn reversed(&self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
        }
    }
}
