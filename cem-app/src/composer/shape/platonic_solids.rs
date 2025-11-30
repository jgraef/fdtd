// todo: we should put a reference here from where we stole the code xD

/// Golden ratio
pub const PHI: f64 = 1.6180339887498948482045868343656;
pub const INV_PHI: f64 = 1.0 / PHI;

pub trait StaticMesh {
    const VERTICES: &'static [[f64; 3]];
    const FACES: &'static [[usize; 3]];
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tetrahedron;

impl StaticMesh for Tetrahedron {
    const VERTICES: &'static [[f64; 3]] =
        &[[-1., -1., -1.], [-1., 1., 1.], [1., -1., 1.], [1., 1., -1.]];

    const FACES: &'static [[usize; 3]] = &[[1, 0, 2], [0, 1, 3], [3, 1, 2], [0, 3, 2]];
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Hexahedron;

impl StaticMesh for Hexahedron {
    const VERTICES: &'static [[f64; 3]] = &[
        [1., 1., -1.],
        [1., -1., 1.],
        [1., -1., -1.],
        [1., 1., 1.],
        [-1., -1., -1.],
        [-1., 1., -1.],
        [-1., 1., 1.],
        [-1., -1., 1.],
    ];

    const FACES: &'static [[usize; 3]] = &[
        [0, 1, 2],
        [1, 0, 3],
        [0, 4, 5],
        [4, 0, 2],
        [6, 0, 5],
        [0, 6, 3],
        [1, 6, 7],
        [6, 1, 3],
        [1, 4, 2],
        [4, 1, 7],
        [6, 4, 7],
        [4, 6, 5],
    ];
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Octahedron;

impl StaticMesh for Octahedron {
    const VERTICES: &'static [[f64; 3]] = &[
        [0., 0., -1.],
        [-1., 0., 0.],
        [0., 1., 0.],
        [1., 0., 0.],
        [0., -1., 0.],
        [0., 0., 1.],
    ];

    const FACES: &'static [[usize; 3]] = &[
        [0, 1, 2],
        [3, 0, 2],
        [1, 0, 4],
        [2, 1, 5],
        [3, 2, 5],
        [4, 0, 3],
        [5, 1, 4],
        [5, 4, 3],
    ];
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Dodecahedron;

impl StaticMesh for Dodecahedron {
    const VERTICES: &'static [[f64; 3]] = &[
        [-1., 1., -1.],
        [-PHI, 0., INV_PHI],
        [-PHI, 0., -INV_PHI],
        [-1., 1., 1.],
        [-INV_PHI, PHI, 0.],
        [1., 1., 1.],
        [INV_PHI, PHI, 0.],
        [0., INV_PHI, PHI],
        [-1., -1., 1.],
        [0., -INV_PHI, PHI],
        [-1., -1., -1.],
        [-INV_PHI, -PHI, 0.],
        [0., -INV_PHI, -PHI],
        [0., INV_PHI, -PHI],
        [1., 1., -1.],
        [PHI, 0., -INV_PHI],
        [PHI, 0., INV_PHI],
        [1., -1., 1.],
        [INV_PHI, -PHI, 0.],
        [1., -1., -1.],
    ];

    const FACES: &'static [[usize; 3]] = &[
        [1, 0, 2],
        [0, 1, 3],
        [0, 3, 4],
        [4, 5, 6],
        [5, 4, 3],
        [5, 3, 7],
        [8, 3, 1],
        [3, 8, 7],
        [7, 8, 9],
        [8, 10, 11],
        [10, 8, 2],
        [2, 8, 1],
        [0, 10, 2],
        [10, 0, 12],
        [12, 0, 13],
        [0, 14, 13],
        [14, 0, 6],
        [6, 0, 4],
        [15, 5, 16],
        [5, 15, 14],
        [5, 14, 6],
        [9, 5, 7],
        [5, 9, 17],
        [5, 17, 16],
        [18, 8, 11],
        [8, 18, 17],
        [8, 17, 9],
        [19, 10, 12],
        [10, 19, 11],
        [11, 19, 18],
        [13, 19, 12],
        [19, 13, 14],
        [19, 14, 15],
        [19, 17, 18],
        [17, 19, 16],
        [16, 19, 15],
    ];
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Icosahedron;

impl StaticMesh for Icosahedron {
    const VERTICES: &'static [[f64; 3]] = &[
        [-1., 0., -PHI],
        [0., PHI, -1.],
        [1., 0., -PHI],
        [0., PHI, 1.],
        [PHI, 1., 0.],
        [1., 0., PHI],
        [-PHI, 1., 0.],
        [-PHI, -1., 0.],
        [-1., 0., PHI],
        [0., -PHI, 1.],
        [PHI, -1., 0.],
        [0., -PHI, -1.],
    ];

    const FACES: &'static [[usize; 3]] = &[
        [1, 6, 3],
        [0, 6, 1],
        [3, 4, 1],
        [3, 6, 8],
        [6, 0, 7],
        [2, 0, 1],
        [4, 3, 5],
        [4, 2, 1],
        [7, 8, 6],
        [5, 3, 8],
        [0, 11, 7],
        [11, 0, 2],
        [4, 5, 10],
        [10, 2, 4],
        [8, 7, 9],
        [8, 9, 5],
        [7, 11, 9],
        [11, 2, 10],
        [10, 5, 9],
        [11, 10, 9],
    ];
}
