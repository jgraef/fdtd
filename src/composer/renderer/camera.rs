use std::f32::consts::FRAC_PI_4;

use bytemuck::{
    Pod,
    Zeroable,
};
use nalgebra::{
    Matrix4,
    Perspective3,
};
use palette::{
    LinSrgba,
    WithAlpha,
};
use wgpu::util::DeviceExt;

use crate::composer::{
    renderer::ClearColor,
    scene::Transform,
};

#[derive(Clone, Copy, Debug)]
pub struct CameraProjection {
    pub projection: Perspective3<f32>,
}

impl CameraProjection {
    pub fn new(fovy: f32) -> Self {
        let projection = Perspective3::new(1.0, fovy, 0.0, 100.0);
        tracing::debug!(?projection);
        Self { projection }
    }

    fn set_viewport(&mut self, viewport: &Viewport) {
        self.projection.set_aspect(viewport.aspect_ratio());
    }
}

impl Default for CameraProjection {
    fn default() -> Self {
        Self::new(FRAC_PI_4)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub viewport: egui::Rect,
}

impl Viewport {
    pub fn aspect_ratio(&self) -> f32 {
        self.viewport.aspect_ratio()
    }
}

#[derive(Clone, Debug)]
pub(super) struct CameraResources {
    pub dirty: bool,
    pub buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

impl CameraResources {
    pub fn new(
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        device: &wgpu::Device,
        camera_data: &CameraData,
    ) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera uniform buffer"),
            contents: bytemuck::bytes_of(camera_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera uniform bind group"),
            layout: camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(buffer.as_entire_buffer_binding()),
            }],
        });

        Self {
            dirty: false,
            buffer,
            bind_group,
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, camera_data: &CameraData) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(camera_data));

        self.dirty = false;
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub(super) struct CameraData {
    pub view_matrix: Matrix4<f32>,
    pub clear_color: LinSrgba,
}

impl CameraData {
    pub fn new(
        camera_projection: &CameraProjection,
        camera_transform: &Transform,
        clear_color: Option<&ClearColor>,
    ) -> Self {
        let mut projection_matrix = camera_projection.projection.to_homogeneous();
        // the projection matrix nalgebra produces has the z-axis inverted relative to
        // our coordinate system, so we fix this here.
        projection_matrix[(2, 2)] *= -1.0;
        projection_matrix[(3, 2)] *= -1.0;
        // also nalgebra doesn't set the iten 4,4 to 1 (why?)
        projection_matrix[(3, 3)] = 1.0;

        //projection_matrix = Matrix4::identity();

        Self {
            // apply inverse transform of camera, then projection
            view_matrix: projection_matrix * camera_transform.transform.inverse().to_homogeneous(),
            // note: shaders always work with linear colors.
            clear_color: clear_color
                .map(|clear_color| clear_color.clear_color.into_linear().with_alpha(1.0))
                .unwrap_or_default(),
        }
    }
}
