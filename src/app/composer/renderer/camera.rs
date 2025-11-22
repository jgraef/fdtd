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
    Vector4,
};
use palette::{
    LinSrgba,
    WithAlpha,
};
use parry3d::{
    bounding_volume::Aabb,
    query::Ray,
};
use serde::{
    Deserialize,
    Serialize,
};
use wgpu::util::DeviceExt;

use crate::app::composer::{
    renderer::{
        ClearColor,
        draw_commands::DrawCommandEnablePipelineFlags,
        light::CameraLightFilter,
    },
    scene::{
        Changed,
        Scene,
        transform::Transform,
    },
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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
        if let Some(aspect_ratio) = viewport.aspect_ratio() {
            self.set_aspect_ratio(aspect_ratio);
        }
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
    /// Returns aspect ratio of viewport.
    ///
    /// Returns `None` if either width or height are non-positive.
    pub fn aspect_ratio(&self) -> Option<f32> {
        (self.viewport.width() > 0.0 && self.viewport.height() > 0.0)
            .then(|| self.viewport.aspect_ratio())
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
        instance_buffer: &wgpu::Buffer,
    ) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera uniform buffer"),
            contents: bytemuck::bytes_of(camera_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group =
            create_camera_bind_group(device, camera_bind_group_layout, &buffer, instance_buffer);

        Self { buffer, bind_group }
    }

    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera_data: &CameraData,
        updated_instance_buffer: Option<(&wgpu::BindGroupLayout, &wgpu::Buffer)>,
    ) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(camera_data));
        if let Some((camera_bind_group_layout, instance_buffer)) = updated_instance_buffer {
            self.bind_group = create_camera_bind_group(
                device,
                camera_bind_group_layout,
                &self.buffer,
                instance_buffer,
            );
        }
    }
}

fn create_camera_bind_group(
    device: &wgpu::Device,
    camera_bind_group_layout: &wgpu::BindGroupLayout,
    camera_buffer: &wgpu::Buffer,
    instance_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("camera uniform bind group"),
        layout: camera_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: instance_buffer.as_entire_binding(),
            },
        ],
    })
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub(super) struct CameraData {
    pub transform: Matrix4<f32>,
    pub projection: Matrix4<f32>,
    pub world_position: Vector4<f32>,
    pub clear_color: LinSrgba,
    pub light_filter: CameraLightFilter,
}

impl CameraData {
    pub fn new(
        camera_projection: &CameraProjection,
        camera_transform: &Transform,
        clear_color: Option<&ClearColor>,
        light_filter: Option<&CameraLightFilter>,
    ) -> Self {
        let transform = camera_transform.transform.inverse().to_homogeneous();

        let mut projection = camera_projection.projection.to_homogeneous();
        // nalgebra assumes we're using a right-handed world coordinate system and a
        // left-handed NDC and thus flips the z-axis. Undo this here.
        projection[(2, 2)] *= -1.0;
        projection[(3, 2)] = 1.0;

        let world_position =
            Point3::from(camera_transform.transform.translation.vector).to_homogeneous();

        Self {
            transform,
            projection,
            world_position,
            // note: shaders always work with linear colors.
            clear_color: clear_color
                .map(|clear_color| clear_color.clear_color.into_linear().with_alpha(1.0))
                .unwrap_or_default(),
            light_filter: light_filter.copied().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CameraConfig {
    // todo: should this just contain the DrawCommandPipelineEnableFlags?
    pub show_solid: bool,
    pub show_wireframe: bool,
    pub show_outline: bool,
}

impl CameraConfig {
    pub fn apply_to_pipeline_enable_flags(
        &self,
        pipeline_enable_flags: &mut DrawCommandEnablePipelineFlags,
    ) {
        pipeline_enable_flags.set(DrawCommandEnablePipelineFlags::SOLID, self.show_solid);
        pipeline_enable_flags.set(
            DrawCommandEnablePipelineFlags::WIREFRAME,
            self.show_wireframe,
        );
        pipeline_enable_flags.set(DrawCommandEnablePipelineFlags::OUTLINE, self.show_outline);
    }
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            show_solid: true,
            show_wireframe: false,
            show_outline: true,
        }
    }
}

pub(super) fn update_cameras(
    scene: &mut Scene,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_bind_group_layout: &wgpu::BindGroupLayout,
    instance_buffer: &wgpu::Buffer,
    instance_buffer_reallocated: bool,
) {
    // update cameras whose viewports changed
    for (entity, (camera_projection, viewport)) in scene
        .entities
        .query_mut::<(&mut CameraProjection, &Viewport)>()
        .with::<&Changed<Viewport>>()
    {
        camera_projection.set_viewport(viewport);
        scene.command_buffer.remove_one::<Changed<Viewport>>(entity);
    }

    // create uniforms for cameras that don't have them yet
    for (entity, (camera_projection, camera_transform, clear_color, camera_light_filter)) in scene
        .entities
        .query_mut::<(
            &CameraProjection,
            &Transform,
            Option<&ClearColor>,
            Option<&CameraLightFilter>,
        )>()
        .without::<&CameraResources>()
    {
        tracing::debug!(
            ?entity,
            ?camera_projection,
            ?camera_transform,
            ?clear_color,
            "creating camera"
        );
        let camera_data = CameraData::new(
            camera_projection,
            camera_transform,
            clear_color,
            camera_light_filter,
        );
        let camera_resources = CameraResources::new(
            camera_bind_group_layout,
            device,
            &camera_data,
            instance_buffer,
        );
        scene.command_buffer.insert_one(entity, camera_resources);
    }

    // remove camera resources for anything that isn't a valid camera anymore
    for (entity, ()) in scene
        .entities
        .query_mut::<()>()
        .with::<&CameraResources>()
        .without::<hecs::Or<&Transform, &CameraProjection>>()
    {
        tracing::warn!(
            ?entity,
            "not a valid camera anymore. removing `CameraResources`"
        );
        scene.command_buffer.remove_one::<CameraResources>(entity);
    }

    // apply commands
    scene.apply_deferred();

    // update camera buffers
    let updated_instance_buffer =
        instance_buffer_reallocated.then_some((camera_bind_group_layout, instance_buffer));
    for (
        _,
        (camera_resources, camera_projection, camera_transform, clear_color, camera_light_filter),
    ) in scene.entities.query_mut::<(
        &mut CameraResources,
        &CameraProjection,
        &Transform,
        Option<&ClearColor>,
        Option<&CameraLightFilter>,
    )>() {
        let camera_data = CameraData::new(
            camera_projection,
            camera_transform,
            clear_color,
            camera_light_filter,
        );
        camera_resources.update(device, queue, &camera_data, updated_instance_buffer);
    }
}
