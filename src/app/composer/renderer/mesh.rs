use std::{
    fmt::Debug,
    ops::{
        Deref,
        Range,
    },
    sync::Arc,
};

use bytemuck::Pod;
use nalgebra::{
    Point2,
    Point3,
    Vector2,
    Vector3,
};
use parry3d::shape::{
    Ball,
    Cuboid,
    Cylinder,
    TriMesh,
};
use wgpu::util::DeviceExt;

use crate::{
    Error,
    app::composer::{
        loader::{
            LoadAsset,
            LoaderContext,
            LoadingProgress,
            LoadingState,
        },
        renderer::{
            Fallbacks,
            light::MaterialTextures,
        },
        scene::{
            Label,
            Scene,
        },
    },
};

#[derive(Clone, Debug)]
pub struct Mesh {
    pub index_buffer: wgpu::Buffer,
    pub vertex_buffer: wgpu::Buffer,
    pub normal_buffer: Option<wgpu::Buffer>,
    pub uv_buffer: Option<wgpu::Buffer>,
    pub indices: Range<u32>,
    pub base_vertex: u32,
    pub winding_order: WindingOrder,
}

impl Mesh {
    pub fn from_surface_mesh(surface_mesh: &SurfaceMesh, device: &wgpu::Device) -> Self {
        // todo: we could just fix the winding order here when we write the indices into
        // the buffer, and not bother doing that in the shader.

        fn buffer<T>(device: &wgpu::Device, label: &str, data: &[T]) -> wgpu::Buffer
        where
            T: Pod,
        {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::STORAGE,
            })
        }

        fn buffer_opt<T>(device: &wgpu::Device, label: &str, data: &[T]) -> Option<wgpu::Buffer>
        where
            T: Pod,
        {
            (!data.is_empty()).then(|| buffer(device, label, data))
        }

        let num_indices = surface_mesh.indices.len();
        let num_vertices = surface_mesh.vertices.len();
        let num_normals = surface_mesh.normals.len();
        let num_uvs = surface_mesh.uvs.len();

        assert_ne!(num_indices, 0, "Mesh with no indices");
        assert_ne!(num_vertices, 0, "Mesh with no vertices");

        assert!(
            num_normals == 0 || num_normals == num_vertices,
            "Surface mesh has {num_vertices} vertices, but {num_normals} normals."
        );
        assert!(
            num_uvs == 0 || num_uvs == num_vertices,
            "Surface mesh has {num_vertices} vertices, but {num_uvs} UVs."
        );

        #[cfg(debug_assertions)]
        {
            for index in surface_mesh.indices.iter().flatten() {
                assert!(
                    (*index as usize) < num_vertices,
                    "Vertex index out of bounds"
                );
            }
        }

        let index_buffer = buffer(device, "mesh index buffer", &surface_mesh.indices);
        let vertex_buffer = buffer(device, "mesh vertex buffer", &surface_mesh.vertices);
        let normal_buffer = buffer_opt(device, "mesh normal buffer", &surface_mesh.normals);
        let uv_buffer = buffer_opt(device, "mesh uv buffer", &surface_mesh.uvs);

        // the indices array in surface_mesh is **not** flat (i.e. it consists of `[u32;
        // 3]`, one index per face), thus we need to multiply by 3.
        let num_indices = (surface_mesh.indices.len() * 3) as u32;

        Self {
            index_buffer,
            vertex_buffer,
            normal_buffer,
            uv_buffer,
            indices: 0..num_indices,
            base_vertex: 0,
            winding_order: surface_mesh.winding_order,
        }
    }

    pub fn from_shape<S: ToSurfaceMesh + ?Sized>(shape: &S, device: &wgpu::Device) -> Self {
        let surface_mesh = shape.to_surface_mesh();
        Self::from_surface_mesh(&surface_mesh, device)
    }
}

#[derive(Debug)]
pub(super) struct MeshBindGroup {
    pub bind_group: wgpu::BindGroup,
}

impl MeshBindGroup {
    pub fn new(
        device: &wgpu::Device,
        mesh_bind_group_layout: &wgpu::BindGroupLayout,
        mesh: &Mesh,
        material_textures: Option<&MaterialTextures>,
        fallbacks: &Fallbacks,
    ) -> Self {
        macro_rules! texture {
            ($binding:expr, $name:ident, $default:ident) => {
                wgpu::BindGroupEntry {
                    binding: $binding,
                    resource: wgpu::BindingResource::TextureView(
                        material_textures
                            .and_then(|material_textures| material_textures.$name.as_ref())
                            .map(|texture_and_view| &texture_and_view.view)
                            .unwrap_or(&fallbacks.$default),
                    ),
                }
            };
        }

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh bind group"),
            layout: mesh_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: mesh.index_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: mesh.vertex_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: mesh
                        .uv_buffer
                        .as_ref()
                        .unwrap_or(&fallbacks.uv_buffer)
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&fallbacks.sampler),
                },
                texture!(4, ambient, white),
                texture!(5, diffuse, white),
                texture!(6, specular, white),
                texture!(7, emissive, white),
            ],
        });

        Self { bind_group }
    }
}

#[derive(Clone, Debug)]
pub struct SurfaceMesh {
    pub indices: Vec<[u32; 3]>,
    pub vertices: Vec<Point3<f32>>,
    pub normals: Vec<Vector3<f32>>,
    pub uvs: Vec<Point2<f32>>,
    pub winding_order: WindingOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WindingOrder {
    Clockwise,
    CounterClockwise,
}

impl WindingOrder {
    pub const fn front_face(&self) -> wgpu::FrontFace {
        match self {
            Self::Clockwise => wgpu::FrontFace::Cw,
            Self::CounterClockwise => wgpu::FrontFace::Ccw,
        }
    }

    pub const fn flipped(self) -> Self {
        match self {
            Self::Clockwise => Self::CounterClockwise,
            Self::CounterClockwise => Self::Clockwise,
        }
    }
}

#[derive(Clone, Debug)]
pub enum LoadMesh {
    Shape(MeshFromShape),
}

impl From<MeshFromShape> for LoadMesh {
    fn from(value: MeshFromShape) -> Self {
        Self::Shape(value)
    }
}

impl LoadAsset for LoadMesh {
    type State = LoadMeshState;

    fn start_loading(&self, context: &mut LoaderContext) -> Result<LoadMeshState, Error> {
        let _ = context;
        match self {
            LoadMesh::Shape(mesh_from_shape) => Ok(LoadMeshState::Shape(mesh_from_shape.clone())),
        }
    }
}

#[derive(Debug)]
pub enum LoadMeshState {
    Shape(MeshFromShape),
}

impl LoadingState for LoadMeshState {
    type Output = (Mesh,);

    fn poll(&mut self, context: &mut LoaderContext) -> Result<LoadingProgress<(Mesh,)>, Error> {
        match self {
            LoadMeshState::Shape(shape) => {
                tracing::debug!(shape = ?shape.0, "loading mesh from shape");
                let mesh = Mesh::from_shape(&*shape.0, context.render_resource_creator.device());
                Ok(LoadingProgress::Ready((mesh,)))
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MeshFromShape(pub Arc<dyn MeshFromShapeTraits>);

impl<S: MeshFromShapeTraits> From<S> for MeshFromShape {
    fn from(value: S) -> Self {
        Self(Arc::new(value))
    }
}

impl Deref for MeshFromShape {
    type Target = dyn MeshFromShapeTraits;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/*
impl Serialize for MeshFromShape {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.as_typed_shape().serialize(serializer)
    }
}*/

pub trait MeshFromShapeTraits: ToSurfaceMesh + Debug + Send + Sync + 'static {}

impl<T> MeshFromShapeTraits for T where T: ToSurfaceMesh + Debug + Send + Sync + 'static {}

pub trait ToSurfaceMesh {
    /// Generate surface mesh.
    ///
    /// At the moment this is only used for rendering. If an entity has the
    /// [`Render`] tag and a [`SharedShape`], the renderer will generate a mesh
    /// for it and send it to the GPU. If this method returns `None` though, the
    /// [`Render`] tag will be removed.
    fn to_surface_mesh(&self) -> SurfaceMesh;
}

/// according to the [documentation][1] the tri mesh should be wound
/// counter-clockwise.
///
/// [1]: https://docs.rs/parry3d/latest/parry3d/shape/struct.TriMesh.html#method.new
pub const PARRY_WINDING_ORDER: WindingOrder = WindingOrder::CounterClockwise;

fn parry_surface_mesh(vertices: Vec<Point3<f32>>, indices: Vec<[u32; 3]>) -> SurfaceMesh {
    SurfaceMesh {
        indices,
        vertices,
        normals: vec![],
        uvs: vec![],
        winding_order: PARRY_WINDING_ORDER,
    }
}

impl ToSurfaceMesh for Ball {
    fn to_surface_mesh(&self) -> SurfaceMesh {
        let (vertices, indices) = self.to_trimesh(20, 20);
        parry_surface_mesh(vertices, indices)
    }
}

impl ToSurfaceMesh for Cuboid {
    fn to_surface_mesh(&self) -> SurfaceMesh {
        let (vertices, indices) = self.to_trimesh();

        let mut mesh = parry_surface_mesh(vertices, indices);

        // fake uvs
        mesh.uvs = vec![
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
        ];

        mesh
    }
}

impl ToSurfaceMesh for Cylinder {
    fn to_surface_mesh(&self) -> SurfaceMesh {
        let (vertices, indices) = self.to_trimesh(20);
        parry_surface_mesh(vertices, indices)
    }
}

impl ToSurfaceMesh for TriMesh {
    fn to_surface_mesh(&self) -> SurfaceMesh {
        parry_surface_mesh(self.vertices().to_owned(), self.indices().to_owned())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub half_extents: Vector2<f32>,
}

impl Quad {
    pub fn new(half_extents: impl Into<Vector2<f32>>) -> Self {
        Self {
            half_extents: half_extents.into(),
        }
    }
}

impl ToSurfaceMesh for Quad {
    fn to_surface_mesh(&self) -> SurfaceMesh {
        const VERTICES: [(f32, f32); 4] = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
        const INDICES: [[u32; 3]; 2] = [[0, 1, 2], [0, 2, 3]];

        SurfaceMesh {
            indices: INDICES.into(),
            vertices: VERTICES
                .iter()
                .map(|(x, y)| {
                    Point3::new(
                        self.half_extents.x * (2.0 * *x - 1.0),
                        self.half_extents.y * (2.0 * *y - 1.0),
                        0.0,
                    )
                })
                .collect(),
            normals: std::iter::repeat_n(Vector3::z(), 4).collect(),
            uvs: VERTICES
                .iter()
                .map(|(x, y)| Point2::new(*x, 1.0 - *y))
                .collect(),
            winding_order: WindingOrder::CounterClockwise,
        }
    }
}

pub(super) fn update_mesh_bind_groups(
    scene: &mut Scene,
    device: &wgpu::Device,
    mesh_bind_group_layout: &wgpu::BindGroupLayout,
    texture_defaults: &Fallbacks,
) {
    // todo: changed tags?

    for (entity, (mesh, material_textures, label)) in scene
        .entities
        .query_mut::<(&Mesh, Option<&MaterialTextures>, Option<&Label>)>()
        .without::<&MeshBindGroup>()
    {
        if mesh.uv_buffer.is_none() && material_textures.is_some() {
            tracing::warn!(?label, "Mesh with textures, but no UV buffer");
        }

        let mesh_bind_group = MeshBindGroup::new(
            device,
            mesh_bind_group_layout,
            mesh,
            material_textures,
            texture_defaults,
        );

        scene.command_buffer.insert_one(entity, mesh_bind_group);
    }

    scene.apply_deferred();
}
