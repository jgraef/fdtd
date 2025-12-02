use std::{
    f32::consts::FRAC_PI_4,
    time::Duration,
};

use bitflags::bitflags;
use bytemuck::{
    Pod,
    Zeroable,
};
use cem_util::wgpu::buffer::WriteStaging;
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

use crate::{
    impl_register_component,
    renderer::{
        ClearColor,
        draw_commands::DrawCommandFlags,
        light::{
            AmbientLight,
            PointLight,
        },
    },
    scene::{
        Changed,
        transform::GlobalTransform,
    },
    util::egui::probe::{
        PropertiesUi,
        TrackChanges,
        label_and_value,
        label_and_value_with_config,
        std::NumericPropertyUiConfig,
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

    pub fn update<S>(
        &mut self,
        device: &wgpu::Device,
        mut write_staging: S,
        camera_data: &CameraData,
        updated_instance_buffer: Option<(&wgpu::BindGroupLayout, &wgpu::Buffer)>,
    ) where
        S: WriteStaging,
    {
        write_staging
            .write_buffer_from_slice(self.buffer.slice(..), bytemuck::bytes_of(camera_data));

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
    transform: Matrix4<f32>,
    projection: Matrix4<f32>,
    world_position: Vector4<f32>,
    clear_color: LinSrgba,
    ambient_light_color: LinSrgba,
    point_light_color: LinSrgba,
    flags: CameraFlags,
    gamma: f32,
    _padding: [u32; 2],
}

impl CameraData {
    pub fn new(
        camera_projection: &CameraProjection,
        camera_transform: &GlobalTransform,
        clear_color: Option<&ClearColor>,
        ambient_light: Option<&AmbientLight>,
        point_light: Option<&PointLight>,
        camera_config: Option<&CameraConfig>,
    ) -> Self {
        let mut data = Self {
            transform: camera_transform.isometry().inverse().to_homogeneous(),
            projection: {
                let mut projection = camera_projection.projection.to_homogeneous();
                // nalgebra assumes we're using a right-handed world coordinate system and a
                // left-handed NDC and thus flips the z-axis. Undo this here.
                projection[(2, 2)] *= -1.0;
                projection[(3, 2)] = 1.0;
                projection
            },
            world_position: camera_transform.position().to_homogeneous(),
            gamma: 1.0,
            ..Self::zeroed()
        };

        if let Some(ambient_light) = ambient_light {
            data.flags.insert(CameraFlags::AMBIENT_LIGHT);
            data.ambient_light_color = ambient_light.color.into_linear().with_alpha(1.0);
        }

        if let Some(point_light) = point_light {
            data.flags.insert(CameraFlags::POINT_LIGHT);
            data.point_light_color = point_light.color.into_linear().with_alpha(1.0);
        }

        if let Some(clear_color) = clear_color {
            data.clear_color = clear_color.clear_color.into_linear().with_alpha(1.0);
        }

        // clippy, i will probably nest other ifs using the camera config
        #[allow(clippy::collapsible_if)]
        if let Some(camera_config) = camera_config {
            if camera_config.tone_map {
                data.flags.insert(CameraFlags::TONE_MAP)
            }
            data.gamma = camera_config.gamma;
        }

        data
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Pod, Zeroable)]
    #[repr(C)]
    struct CameraFlags: u32 {
        const AMBIENT_LIGHT = 0b0000_0001;
        const POINT_LIGHT   = 0b0000_0010;
        const TONE_MAP      = 0b0000_0100;
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CameraConfig {
    // todo: should this just contain the DrawCommandPipelineEnableFlags?
    pub show_mesh_opaque: bool,
    pub show_mesh_transparent: bool,
    pub show_wireframe: bool,
    pub show_debug_wireframe: bool,
    pub show_outline: bool,
    pub tone_map: bool,
    pub gamma: f32,
}

impl CameraConfig {
    pub fn apply_to_draw_command_flags(&self, flags: &mut DrawCommandFlags) {
        flags.set(DrawCommandFlags::MESH_OPAQUE, self.show_mesh_opaque);
        flags.set(
            DrawCommandFlags::MESH_TRANSPARENT,
            self.show_mesh_transparent,
        );
        flags.set(DrawCommandFlags::WIREFRAME, self.show_wireframe);
        flags.set(DrawCommandFlags::DEBUG_WIREFRAME, self.show_debug_wireframe);
        flags.set(DrawCommandFlags::OUTLINE, self.show_outline);
    }
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            show_mesh_opaque: true,
            show_mesh_transparent: true,
            show_wireframe: true,
            show_debug_wireframe: false,
            show_outline: true,
            tone_map: true,
            gamma: 2.4,
        }
    }
}

impl PropertiesUi for CameraConfig {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, _config: &Self::Config) -> egui::Response {
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui: &mut egui::Ui| {
                label_and_value(
                    ui,
                    "Mesh (Opaque)",
                    &mut changes,
                    &mut self.show_mesh_opaque,
                );
                label_and_value(
                    ui,
                    "Mesh (Transparent)",
                    &mut changes,
                    &mut self.show_mesh_transparent,
                );
                label_and_value(ui, "Wireframe", &mut changes, &mut self.show_wireframe);
                label_and_value(
                    ui,
                    "Debug Wireframe",
                    &mut changes,
                    &mut self.show_debug_wireframe,
                );
                label_and_value(ui, "Outline", &mut changes, &mut self.show_outline);
                label_and_value(ui, "Tone Map", &mut changes, &mut self.tone_map);
                label_and_value_with_config(
                    ui,
                    "Gamma",
                    &mut changes,
                    &mut self.gamma,
                    &NumericPropertyUiConfig::Slider { range: 0.0..=4.0 },
                );
            })
            .response;

        changes.propagated(response)
    }
}

impl_register_component!(CameraConfig where ComponentUi, default);

#[derive(Clone, Copy, Debug)]
pub struct CameraRenderInfo {
    pub total: Duration,
    pub num_opaque: usize,
    pub num_transparent: usize,
    pub num_outlines: usize,
}

pub(super) fn update_cameras<S>(
    entities: &mut hecs::World,
    command_buffer: &mut hecs::CommandBuffer,
    device: &wgpu::Device,
    mut write_staging: S,
    camera_bind_group_layout: &wgpu::BindGroupLayout,
    instance_buffer: &wgpu::Buffer,
    instance_buffer_reallocated: bool,
) where
    S: WriteStaging,
{
    // update cameras whose viewports changed
    for (entity, (camera_projection, viewport)) in entities
        .query_mut::<(&mut CameraProjection, &Viewport)>()
        .with::<&Changed<Viewport>>()
    {
        camera_projection.set_viewport(viewport);
        command_buffer.remove_one::<Changed<Viewport>>(entity);
    }

    // create uniforms for cameras that don't have them yet
    for (
        entity,
        (
            camera_projection,
            camera_transform,
            clear_color,
            ambient_light,
            point_light,
            camera_config,
        ),
    ) in entities
        .query_mut::<(
            &CameraProjection,
            &GlobalTransform,
            Option<&ClearColor>,
            Option<&AmbientLight>,
            Option<&PointLight>,
            Option<&CameraConfig>,
        )>()
        .without::<&CameraResources>()
    {
        tracing::debug!(
            ?entity,
            ?camera_projection,
            ?camera_transform,
            ?clear_color,
            ?ambient_light,
            ?point_light,
            "creating camera"
        );
        let camera_data = CameraData::new(
            camera_projection,
            camera_transform,
            clear_color,
            ambient_light,
            point_light,
            camera_config,
        );
        let camera_resources = CameraResources::new(
            camera_bind_group_layout,
            device,
            &camera_data,
            instance_buffer,
        );
        command_buffer.insert_one(entity, camera_resources);
    }

    // remove camera resources for anything that isn't a valid camera anymore
    for (entity, ()) in entities
        .query_mut::<()>()
        .with::<&CameraResources>()
        .without::<hecs::Or<&GlobalTransform, &CameraProjection>>()
    {
        tracing::warn!(
            ?entity,
            "not a valid camera anymore. removing `CameraResources`"
        );
        command_buffer.remove_one::<CameraResources>(entity);
    }

    // update camera buffers
    let updated_instance_buffer =
        instance_buffer_reallocated.then_some((camera_bind_group_layout, instance_buffer));
    for (
        _,
        (
            camera_resources,
            camera_projection,
            camera_transform,
            clear_color,
            ambient_light,
            point_light,
            camera_config,
        ),
    ) in entities.query_mut::<(
        &mut CameraResources,
        &CameraProjection,
        &GlobalTransform,
        Option<&ClearColor>,
        Option<&AmbientLight>,
        Option<&PointLight>,
        Option<&CameraConfig>,
    )>() {
        let camera_data = CameraData::new(
            camera_projection,
            camera_transform,
            clear_color,
            ambient_light,
            point_light,
            camera_config,
        );
        camera_resources.update(
            device,
            &mut write_staging,
            &camera_data,
            updated_instance_buffer,
        );
    }
}
