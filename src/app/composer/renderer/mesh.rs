use std::ops::Range;

use nalgebra::Point3;
use wgpu::util::DeviceExt;

use crate::app::composer::{
    renderer::Render,
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
    pub indices: Range<u32>,
    pub base_vertex: u32,
    pub bind_group: wgpu::BindGroup,
    pub winding_order: WindingOrder,
}

impl Mesh {
    pub fn from_surface_mesh(
        surface_mesh: &SurfaceMesh,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
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

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh bind group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: index_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: vertex_buffer.as_entire_binding(),
                },
            ],
        });

        // the indices array in surface_mesh is **not** flat (i.e. it consists of `[u32;
        // 3]`, one index per face), thus we need to multiply by 3.
        let num_indices = (surface_mesh.indices.len() * 3) as u32;

        Self {
            index_buffer,
            vertex_buffer,
            indices: 0..num_indices,
            base_vertex: 0,
            bind_group,
            winding_order: surface_mesh.winding_order,
        }
    }

    pub fn from_shape<S: Shape + ?Sized>(
        shape: &S,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Option<Self> {
        shape
            .to_surface_mesh()
            .map(|surface_mesh| Self::from_surface_mesh(&surface_mesh, device, bind_group_layout))
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
) {
    for (entity, (shape, label)) in scene
        .entities
        .query_mut::<(&SharedShape, Option<&Label>)>()
        .with::<&Render>()
        .without::<&Mesh>()
    {
        if let Some(mesh) = Mesh::from_shape(&*shape.0, device, mesh_bind_group_layout) {
            scene.command_buffer.insert_one(entity, mesh);
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
