use std::{
    fmt::Debug,
    ops::{
        Deref,
        Range,
    },
    sync::Arc,
};

use bitflags::bitflags;
use bytemuck::{
    Pod,
    Zeroable,
};
use nalgebra::{
    Isometry3,
    Point2,
    Point3,
    Vector2,
    Vector3,
};
use parry3d::{
    bounding_volume::Aabb,
    query::{
        Ray,
        RayCast as _,
    },
    shape::{
        Ball,
        Cuboid,
        Cylinder,
        TriMesh,
    },
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
            material::{
                AlbedoTexture,
                MaterialTexture,
            },
        },
        scene::{
            Changed,
            Label,
            Scene,
            spatial::{
                ComputeAabb,
                PointQuery,
                RayCast,
            },
        },
    },
    util::format_size,
};

#[derive(Clone, Debug)]
pub struct Mesh {
    pub index_buffer: wgpu::Buffer,
    pub vertex_buffer: wgpu::Buffer,
    pub indices: Range<u32>,
    pub base_vertex: u32,
    pub winding_order: WindingOrder,
    pub flags: MeshFlags,
}

impl Mesh {
    pub fn from_surface_mesh(surface_mesh: &SurfaceMesh, device: &wgpu::Device) -> Self {
        // todo: we could just fix the winding order here when we write the indices into
        // the buffer, and not bother doing that in the shader.

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

        let mut flags = MeshFlags::empty();
        if !surface_mesh.normals.is_empty() {
            flags.insert(MeshFlags::NORMALS);
        }
        if !surface_mesh.uvs.is_empty() {
            flags.insert(MeshFlags::UVS);
        }

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh/index"),
            contents: bytemuck::cast_slice(&surface_mesh.indices),
            usage: wgpu::BufferUsages::STORAGE,
        });

        // todo: fix winding order in this step
        let vertex_data = surface_mesh
            .vertices
            .iter()
            .copied()
            .zip(
                surface_mesh
                    .normals
                    .iter()
                    .copied()
                    .chain(std::iter::repeat(Vector3::zeros())),
            )
            .zip(
                surface_mesh
                    .uvs
                    .iter()
                    .copied()
                    .chain(std::iter::repeat(Point2::origin())),
            )
            .map(|((position, normal), uv)| Vertex::new(position, normal, uv))
            .collect::<Vec<_>>();

        let vertex_data = bytemuck::cast_slice(&vertex_data);

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh/vertex"),
            contents: vertex_data,
            usage: wgpu::BufferUsages::STORAGE,
        });

        // the indices array in surface_mesh is **not** flat (i.e. it consists of `[u32;
        // 3]`, one index per face), thus we need to multiply by 3.
        let num_indices = (surface_mesh.indices.len() * 3) as u32;

        tracing::debug!(
            ?num_indices,
            ?num_vertices,
            ?num_normals,
            ?num_uvs,
            ?flags,
            index_buffer_size = %format_size(num_indices * 3),
            vertex_buffer_size = %format_size(vertex_data.len()),
            "created mesh"
        );

        Self {
            index_buffer,
            vertex_buffer,
            indices: 0..num_indices,
            base_vertex: 0,
            winding_order: surface_mesh.winding_order,
            flags,
        }
    }

    pub fn from_shape<S: ToSurfaceMesh + ?Sized>(shape: &S, device: &wgpu::Device) -> Self {
        let surface_mesh = shape.to_surface_mesh();
        Self::from_surface_mesh(&surface_mesh, device)
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    #[repr(C)]
    pub struct MeshFlags: u32 {
        const UVS     = 0x0000_0001;
        const NORMALS = 0x0000_0002;
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct Vertex([f32; 8]);

impl Vertex {
    pub fn new(position: Point3<f32>, normal: Vector3<f32>, uv: Point2<f32>) -> Self {
        Self([
            position.x, position.y, position.z, uv.x, normal.x, normal.y, normal.z, uv.y,
        ])
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
        albedo_texture: Option<&AlbedoTexture>,
        material_texture: Option<&MaterialTexture>,
        fallbacks: &Fallbacks,
    ) -> Self {
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
                    resource: wgpu::BindingResource::Sampler(&fallbacks.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        albedo_texture.map_or(&fallbacks.white, |texture| &texture.texture.view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        material_texture.map_or(&fallbacks.white, |texture| &texture.texture.view),
                    ),
                },
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

impl<T> From<T> for LoadMesh
where
    T: MeshFromShapeTraits,
{
    fn from(value: T) -> Self {
        MeshFromShape::from(value).into()
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
    type Output = (Mesh, Changed<Mesh>);

    fn poll(
        &mut self,
        context: &mut LoaderContext,
    ) -> Result<LoadingProgress<(Mesh, Changed<Mesh>)>, Error> {
        let mesh = match self {
            LoadMeshState::Shape(shape) => {
                tracing::debug!(shape = ?shape.0, "loading mesh from shape");
                Mesh::from_shape(&*shape.0, context.render_resource_creator.device())
            }
        };

        Ok(LoadingProgress::Ready((mesh, Changed::default())))
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
        let mut mesh = parry_surface_mesh(vertices, indices);

        mesh.normals = Vec::with_capacity(mesh.vertices.len());

        for vertex in &mesh.vertices {
            mesh.normals.push(vertex.coords.normalize());
        }

        mesh
    }
}

impl ToSurfaceMesh for Cuboid {
    fn to_surface_mesh(&self) -> SurfaceMesh {
        let (vertices, indices) = self.to_trimesh();

        parry_surface_mesh(vertices, indices)
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

// todo: move this
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
            normals: std::iter::repeat_n(-Vector3::z(), 4).collect(),
            uvs: VERTICES
                .iter()
                .map(|(x, y)| Point2::new(*x, 1.0 - *y))
                .collect(),
            winding_order: WindingOrder::CounterClockwise,
        }
    }
}

impl ComputeAabb for Quad {
    fn compute_aabb(&self, transform: &nalgebra::Isometry3<f32>) -> Aabb {
        Aabb::from_half_extents(
            Point3::origin(),
            Vector3::new(self.half_extents.x, self.half_extents.y, 0.0),
        )
        .transform_by(transform)
    }
}

impl RayCast for Quad {
    fn cast_ray(
        &self,
        transform: &Isometry3<f32>,
        ray: &Ray,
        max_time_of_impact: f32,
        solid: bool,
    ) -> Option<f32> {
        self.compute_aabb(transform)
            .cast_local_ray(ray, max_time_of_impact, solid)
    }
}

impl PointQuery for Quad {
    fn supported(&self) -> bool {
        false
    }

    fn contains_point(&self, transform: &Isometry3<f32>, point: &Point3<f32>) -> bool {
        let _ = (transform, point);
        false
    }
}

pub(super) fn update_mesh_bind_groups(
    scene: &mut Scene,
    device: &wgpu::Device,
    mesh_bind_group_layout: &wgpu::BindGroupLayout,
    texture_defaults: &Fallbacks,
) {
    let mut update_mesh_bind_group = |entity: hecs::Entity,
                                      mesh: &Mesh,
                                      albedo_texture: Option<&AlbedoTexture>,
                                      material_texture: Option<&MaterialTexture>,
                                      label: Option<&Label>| {
        if !mesh.flags.contains(MeshFlags::UVS)
            && (albedo_texture.is_some() || material_texture.is_some())
        {
            tracing::warn!(?label, "Mesh with textures, but no UV buffer");
        }

        let mesh_bind_group = MeshBindGroup::new(
            device,
            mesh_bind_group_layout,
            mesh,
            albedo_texture,
            material_texture,
            texture_defaults,
        );

        scene.command_buffer.remove_one::<Changed<Mesh>>(entity);
        scene
            .command_buffer
            .remove_one::<Changed<AlbedoTexture>>(entity);
        scene
            .command_buffer
            .remove_one::<Changed<MaterialTexture>>(entity);
        scene.command_buffer.insert_one(entity, mesh_bind_group);
    };

    for (entity, (mesh, albedo_texture, material_texture, label)) in scene
        .entities
        .query_mut::<(
            &Mesh,
            Option<&AlbedoTexture>,
            Option<&MaterialTexture>,
            Option<&Label>,
        )>()
        .without::<&MeshBindGroup>()
    {
        tracing::debug!(?label, "creating mesh bind group");
        update_mesh_bind_group(entity, mesh, albedo_texture, material_texture, label);
    }

    for (entity, (mesh, albedo_texture, material_texture, label)) in scene
        .entities
        .query_mut::<(&Mesh, Option<&AlbedoTexture>, Option<&MaterialTexture>, Option<&Label>)>()
        .with::<&MeshBindGroup>()
        .with::<hecs::Or<&Changed<Mesh>, hecs::Or<&Changed<AlbedoTexture>, &Changed<MaterialTexture>>>>()
    {
        tracing::debug!(?label, "updating mesh bind group");
        update_mesh_bind_group(entity, mesh, albedo_texture, material_texture, label);
    }

    scene.apply_deferred();
}
