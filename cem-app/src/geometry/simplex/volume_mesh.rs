pub struct VolumeMesh<V, E, F, C> {
    vertices: Vec<Vertex<V>>,
    edges: Vec<Edge<E>>,
    faces: Vec<Face<F>>,
    cells: Vec<Cell<C>>,
}

struct Vertex<T> {
    data: T,
    incident_edge: usize,
}

struct Edge<T> {
    data: T,
    vertices: [usize; 2],
    incident_face: usize,
}

struct Face<T> {
    data: T,
    edges: [usize; 3],
    incident_cells: [usize; 2],
}

struct Cell<T> {
    data: T,
    faces: [usize; 4],
}

impl<V, E, F, C> Default for VolumeMesh<V, E, F, C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V, E, F, C> VolumeMesh<V, E, F, C> {
    pub const fn new() -> Self {
        Self {
            vertices: vec![],
            edges: vec![],
            faces: vec![],
            cells: vec![],
        }
    }
}
