#[cfg(feature = "parry-mesh")]
pub mod parry;

use std::{
    fmt::Debug,
    ops::Range,
    sync::Arc,
};

use bevy_ecs::{
    component::Component,
    lifecycle::HookContext,
    world::DeferredWorld,
};
use bitflags::bitflags;
use bytemuck::{
    Pod,
    Zeroable,
};
use cem_scene::assets::{
    AssetError,
    LoadAsset,
    LoadingProgress,
    LoadingState,
};
use cem_util::format_size;
use nalgebra::{
    Point2,
    Point3,
    Vector3,
};
use wgpu::util::DeviceExt;

use crate::{
    material::{
        AlbedoTexture,
        MaterialTexture,
    },
    renderer::{
        Fallbacks,
        Renderer,
    },
    resource::RenderResourceManager,
    systems::UpdateMeshBindGroupMessage,
};

#[derive(Debug, Component)]
#[component(on_add = mesh_added, on_insert = mesh_added, on_remove = mesh_removed)]
pub struct Mesh {
    pub index_buffer: wgpu::Buffer,
    pub vertex_buffer: wgpu::Buffer,
    pub indices: Range<u32>,
    pub base_vertex: u32,
    pub winding_order: WindingOrder,
    pub flags: MeshFlags,
}

fn mesh_added(mut world: DeferredWorld, context: HookContext) {
    world.write_message(UpdateMeshBindGroupMessage::MeshAdded {
        entity: context.entity,
    });
}

fn mesh_removed(mut world: DeferredWorld, context: HookContext) {
    world.write_message(UpdateMeshBindGroupMessage::MeshRemoved {
        entity: context.entity,
    });
}

bitflags! {
    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    #[repr(C)]
    pub struct MeshFlags: u32 {
        const UVS       = 0x0000_0001;
        const NORMALS   = 0x0000_0002;
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

#[derive(Debug, Component)]
pub struct MeshBindGroup {
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

#[derive(Debug)]
pub struct MeshBufferBuilder {
    flags: MeshFlags,
    preferred_winding_order: Option<WindingOrder>,
    index_buffer: Vec<[u32; 3]>,
    vertex_buffer: Vec<Vertex>,
}

impl MeshBufferBuilder {
    pub fn new(preferred_winding_order: Option<WindingOrder>) -> Self {
        Self {
            flags: MeshFlags::empty(),
            preferred_winding_order,
            index_buffer: vec![],
            vertex_buffer: vec![],
        }
    }

    pub fn finish(self, device: &wgpu::Device, label: &str) -> Mesh {
        let num_indices = self.index_buffer.len();
        let num_vertices = self.vertex_buffer.len();

        assert_ne!(num_indices, 0, "Mesh with no indices");
        assert_ne!(num_vertices, 0, "Mesh with no vertices");
        let winding_order = self
            .preferred_winding_order
            .expect("once we get a face, we must have a winding order");

        #[cfg(debug_assertions)]
        {
            for index in self.index_buffer.iter().flatten() {
                assert!(
                    (*index as usize) < num_vertices,
                    "Vertex index out of bounds: {index} < {num_vertices}"
                );
            }
        }

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label}/mesh/index")),
            contents: bytemuck::cast_slice(&self.index_buffer),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let vertex_data = bytemuck::cast_slice(&self.vertex_buffer);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label}/mesh/vertex")),
            contents: vertex_data,
            usage: wgpu::BufferUsages::STORAGE,
        });

        // the indices array in surface_mesh is **not** flat (i.e. it consists of `[u32;
        // 3]`, one index per face), thus we need to multiply by 3.
        let num_indices = (self.index_buffer.len() * 3) as u32;

        tracing::debug!(
            ?label,
            ?num_indices,
            flags = ?self.flags,
            index_buffer_size = %format_size(num_indices * 3),
            vertex_buffer_size = %format_size(vertex_data.len()),
            "created mesh"
        );

        Mesh {
            index_buffer,
            vertex_buffer,
            indices: 0..num_indices,
            base_vertex: 0,
            winding_order,
            flags: self.flags,
        }
    }
}

impl MeshBuilder for MeshBufferBuilder {
    fn reserve(&mut self, num_faces: usize, num_vertices: usize) {
        self.index_buffer.reserve(num_faces);
        self.vertex_buffer.reserve(num_vertices);
    }

    fn push_face(&mut self, mut face: [u32; 3], winding_order: WindingOrder) {
        let reverse_winding = if let Some(preferred_winding_order) = self.preferred_winding_order {
            winding_order != preferred_winding_order
        }
        else {
            self.preferred_winding_order = Some(winding_order);
            false
        };

        if reverse_winding {
            face.reverse();
        }

        self.index_buffer.push(face);
    }

    fn push_vertex(
        &mut self,
        vertex: Point3<f32>,
        normal: Option<Vector3<f32>>,
        uv: Option<Point2<f32>>,
    ) {
        if normal.is_some() {
            self.flags.insert(MeshFlags::NORMALS);
        }
        if uv.is_some() {
            self.flags.insert(MeshFlags::UVS);
        }

        self.vertex_buffer.push(Vertex::new(
            vertex,
            normal.unwrap_or_default(),
            uv.unwrap_or_default(),
        ));
    }
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

#[derive(Clone, Debug, Component)]
pub enum LoadMesh {
    Generator {
        generator: Arc<dyn LoadFromMeshGenerator>,
    },
    /*File {
        path: PathBuf,
    },*/
}

impl LoadMesh {
    pub fn from_generator<G>(generator: G) -> Self
    where
        G: GenerateMesh + Debug + Send + Sync + 'static,
    {
        Self::Generator {
            generator: Arc::new(generator),
        }
    }

    // fixme: we don't return a Result here because it's just too easy to
    // accidentally just stick the whole Result into an entity as component. You
    // won't even get a warning because, well, the Result is used, but the mesh will
    // not work.
    // It's probably a better idea to defer the error until we start loading where
    // we can emit it anyway.
    pub fn from_shape<S>(shape: S, config: S::Config) -> Self
    where
        S: IntoGenerateMesh,
        S::GenerateMesh: Debug + Send + Sync + 'static,
    {
        Self::from_generator(shape.into_generate_mesh(config).unwrap())
    }
}

impl LoadAsset for LoadMesh {
    type State = Self;

    fn start_loading(&self) -> Result<Self, AssetError> {
        Ok(self.clone())
    }
}

impl LoadingState for LoadMesh {
    type Output = Mesh;
    type Context = RenderResourceManager<'static>;

    fn poll(
        &mut self,
        context: &mut RenderResourceManager,
    ) -> Result<LoadingProgress<Mesh>, AssetError> {
        let mesh = match self {
            LoadMesh::Generator { generator } => {
                let mut mesh_builder = MeshBufferBuilder::new(Some(Renderer::WINDING_ORDER));
                generator.generate(&mut mesh_builder, true, true);
                mesh_builder.finish(context.device(), &format!("{generator:?}"))
            } //LoadMesh::File { path: _ } => todo!("load mesh from file"),
        };

        Ok(LoadingProgress::Ready(mesh))
    }
}

pub trait LoadFromMeshGenerator: GenerateMesh + Debug + Send + Sync + 'static {}

impl<T> LoadFromMeshGenerator for T where T: GenerateMesh + Debug + Send + Sync + 'static {}

pub trait GenerateMesh {
    fn generate(&self, mesh_builder: &mut dyn MeshBuilder, normals: bool, uvs: bool);
}

pub trait MeshBuilder {
    fn reserve(&mut self, num_faces: usize, num_vertices: usize);
    fn push_face(&mut self, face: [u32; 3], winding_order: WindingOrder);
    fn push_vertex(
        &mut self,
        vertex: Point3<f32>,
        normal: Option<Vector3<f32>>,
        uv: Option<Point2<f32>>,
    );
}

pub trait IntoGenerateMesh {
    type Config;
    type GenerateMesh: GenerateMesh;
    type Error: std::error::Error;

    fn into_generate_mesh(self, config: Self::Config) -> Result<Self::GenerateMesh, Self::Error>;
}
