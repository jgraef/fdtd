use std::ops::Range;

use nalgebra::Point3;
use wgpu::util::DeviceExt;

use crate::composer::scene::Shape;

#[derive(Clone, Debug)]
pub struct Mesh {
    pub index_buffer: wgpu::Buffer,
    pub vertex_buffer: wgpu::Buffer,
    pub indices: Range<u32>,
    pub base_vertex: i32,
    pub bind_group: wgpu::BindGroup,
    pub winding_order: WindingOrder,
}

impl Mesh {
    pub fn from_surface_mesh(
        surface_mesh: &SurfaceMesh,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
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
    pub fn front_face(&self) -> wgpu::FrontFace {
        match self {
            WindingOrder::Clockwise => wgpu::FrontFace::Cw,
            WindingOrder::CounterClockwise => wgpu::FrontFace::Ccw,
        }
    }
}
