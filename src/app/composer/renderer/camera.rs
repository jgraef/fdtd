use std::f32::consts::FRAC_PI_4;

use bytemuck::{
    Pod,
    Zeroable,
};
use nalgebra::{
    Matrix4,
    Perspective3,
    Point2,
    Point3,
    Vector2,
};
use palette::{
    LinSrgba,
    WithAlpha,
};
use parry3d::{
    bounding_volume::Aabb,
    query::Ray,
};
use wgpu::util::DeviceExt;

use crate::app::composer::{
    renderer::ClearColor,
    scene::Transform,
};

#[derive(Clone, Copy, Debug)]
pub struct CameraProjection {
    // note: not public because nalgebra seems to have the z-axis inverted relative to our
    // coordinate systems
    projection: Perspective3<f32>,
}

impl CameraProjection {
    /// # Arguments
    ///
    /// - `fovy`: Field of view along (camera-local) Y-axis (vertical angle).
    pub fn new(fovy: f32) -> Self {
        let projection = Perspective3::new(1.0, fovy, 0.1, 100.0);
        tracing::debug!(?projection);
        Self { projection }
    }

    pub(super) fn set_viewport(&mut self, viewport: &Viewport) {
        self.set_aspect_ratio(viewport.aspect_ratio());
    }

    /// Set aspect ratio (width / height)
    pub fn set_aspect_ratio(&mut self, aspect_ratio: f32) {
        self.projection.set_aspect(aspect_ratio);
    }

    pub fn unproject(&self, point: &Point3<f32>) -> Point3<f32> {
        let mut point = self.projection.unproject_point(point);
        // nalgebra's projection uses a reversed z-axis
        point.z *= -1.0;
        point
    }

    /// Returns angles (horizontal, vertical) that a point makes with the focal
    /// point of the camera.
    pub fn unproject_screen(&self, point: &Point2<f32>) -> Vector2<f32> {
        let fovy = self.projection.fovy();
        let aspect_ratio = self.projection.aspect();
        Vector2::new(point.x * fovy / aspect_ratio, point.y * fovy)
    }

    /// Shoot ray out of camera through point on screen. pew pew!
    pub fn shoot_screen_ray(&self, point: &Point2<f32>) -> Ray {
        let target = self.unproject(&Point3::new(point.x, point.y, 1.0));
        Ray {
            origin: Point3::origin(),
            dir: target.coords.normalize(),
        }
    }

    pub fn fovy(&self) -> f32 {
        self.projection.fovy()
    }

    /// Aspect ration (width / height)
    pub fn aspect_ratio(&self) -> f32 {
        self.projection.aspect()
    }

    /// Distance needed to move back from center of AABB to fit the AABB into
    /// FOV, assuming the camera is looking straight onto its XY plane
    ///
    /// To fit other orientations, either rotate a given AABB, or for better
    /// results compute the AABB in the rotated reference frame.
    ///
    /// One can then for example calculate a new camera transform by centering
    /// on the center of the AABB, adding the choosen rotation, and translating
    /// by `-Vector3::z() * distance` locally.
    pub fn distance_to_fit_aabb_into_fov(&self, aabb: &Aabb, margin: &Vector2<f32>) -> f32 {
        let scene_aabb_half_extents = aabb.half_extents();

        // camera projection parameters
        let half_fovy = 0.5 * self.fovy();
        let aspect_ratio = self.aspect_ratio();
        let half_fovx = half_fovy / aspect_ratio;

        // how far back do we have to be from the face of the AABB to fit the vertical
        // FOV of the camera? simple geometry tells us that tan(fovy/2) = y/z,
        // where y is the half-extend of the AABB in y-direction.
        let dz_vertical = (scene_aabb_half_extents.y + margin.y) / half_fovy.tan();

        // same for horizontal fit
        let dz_horizontal = (scene_aabb_half_extents.x + margin.x) / half_fovx.tan();

        // we want to fit both, so we take the max. we also need to add the distance
        // from the center of the AABB to its face along the z-axis.
        scene_aabb_half_extents.z + dz_vertical.max(dz_horizontal)
    }
}

impl Default for CameraProjection {
    fn default() -> Self {
        // 45 degrees
        let fovy = FRAC_PI_4;

        Self::new(fovy)
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

        Self { buffer, bind_group }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, camera_data: &CameraData) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(camera_data));
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

        // nalgebra assumes we're using a right-handed world coordinate system and a
        // left-handed NDC and thus flips the z-axis. Undo this here.
        projection_matrix[(2, 2)] *= -1.0;
        projection_matrix[(3, 2)] = 1.0;

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

#[derive(Clone, Copy, Debug)]
pub struct CameraConfig {
    pub show_solid: bool,
    pub show_wireframe: bool,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            show_solid: true,
            show_wireframe: true,
        }
    }
}
