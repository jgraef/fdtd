use std::ops::Range;

use wgpu::util::DeviceExt;

use crate::composer::scene::{
    Shape,
    SurfaceMesh,
};

#[derive(Clone, Debug)]
pub struct Mesh {
    pub index_buffer: wgpu::Buffer,
    pub vertex_buffer: wgpu::Buffer,
    pub indices: Range<u32>,
    pub base_vertex: i32,
}

impl Mesh {
    pub fn from_surface_mesh(surface_mesh: &SurfaceMesh, device: &wgpu::Device) -> Self {
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh index buffer"),
            contents: bytemuck::cast_slice(&surface_mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertex buffer"),
            contents: bytemuck::cast_slice(&surface_mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // the indices array in surface_mesh is **not** flat (i.e. it consists of `[u32;
        // 3]`, one index per face), thus we need to multiply by 3.
        let num_indices = (surface_mesh.indices.len() * 3) as u32;

        Self {
            index_buffer,
            vertex_buffer,
            indices: 0..num_indices,
            base_vertex: 0,
        }
    }

    pub fn from_shape<S: Shape + ?Sized>(shape: &S, device: &wgpu::Device) -> Option<Self> {
        shape
            .to_surface_mesh()
            .map(|surface_mesh| Self::from_surface_mesh(&surface_mesh, device))
    }
}
