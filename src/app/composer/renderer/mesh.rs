use std::ops::Range;

use nalgebra::Point3;
use wgpu::util::DeviceExt;

use crate::app::composer::{
    renderer::{
        Fallbacks,
        Render,
        light::MaterialTextures,
    },
    scene::{
        Label,
        Scene,
        shape::{
            Shape,
            SharedShape,
        },
    },
};

#[derive(Clone, Debug)]
pub struct Mesh {
    pub index_buffer: wgpu::Buffer,
    pub vertex_buffer: wgpu::Buffer,
    pub uv_buffer: Option<wgpu::Buffer>,
    pub indices: Range<u32>,
    pub base_vertex: u32,
    pub winding_order: WindingOrder,
}

impl Mesh {
    pub fn from_surface_mesh(surface_mesh: &SurfaceMesh, device: &wgpu::Device) -> Self {
        // todo: we could just fix the winding order here when we write the indices into
        // the buffer, and not bother doing that in the shader.

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh index buffer"),
            contents: bytemuck::cast_slice(&surface_mesh.indices),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertex buffer"),
            contents: bytemuck::cast_slice(&surface_mesh.vertices),
            usage: wgpu::BufferUsages::STORAGE,
        });

        // the indices array in surface_mesh is **not** flat (i.e. it consists of `[u32;
        // 3]`, one index per face), thus we need to multiply by 3.
        let num_indices = (surface_mesh.indices.len() * 3) as u32;

        Self {
            index_buffer,
            vertex_buffer,
            uv_buffer: None,
            indices: 0..num_indices,
            base_vertex: 0,
            winding_order: surface_mesh.winding_order,
        }
    }

    pub fn from_shape<S: Shape + ?Sized>(shape: &S, device: &wgpu::Device) -> Option<Self> {
        shape
            .to_surface_mesh()
            .map(|surface_mesh| Self::from_surface_mesh(&surface_mesh, device))
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
    pub vertices: Vec<Point3<f32>>,
    pub indices: Vec<[u32; 3]>,
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

pub(super) fn generate_meshes_for_shapes(
    scene: &mut Scene,
    device: &wgpu::Device,
    mesh_bind_group_layout: &wgpu::BindGroupLayout,
    texture_defaults: &Fallbacks,
) {
    for (entity, (shape, material_textures, label)) in scene
        .entities
        .query_mut::<(&SharedShape, Option<&MaterialTextures>, Option<&Label>)>()
        .with::<&Render>()
        .without::<&Mesh>()
    {
        if let Some(mesh) = Mesh::from_shape(&*shape.0, device) {
            if mesh.uv_buffer.is_none() && material_textures.is_some() {
                tracing::warn!(?label, "Mesh with textures, but no UV buffer");
            }

            let mesh_bind_group = MeshBindGroup::new(
                device,
                mesh_bind_group_layout,
                &mesh,
                material_textures,
                texture_defaults,
            );

            scene.command_buffer.insert(entity, (mesh, mesh_bind_group));
        }
        else {
            tracing::warn!(
                "Entity {entity:?} (label {label:?}) was marked for rendering, but a mesh could not be constructed."
            );
            scene.command_buffer.remove::<(Render,)>(entity);
        }
    }

    scene.apply_deferred();
}
