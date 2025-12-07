use std::{
    convert::Infallible,
    ops::Index,
    path::PathBuf,
};

use bevy_ecs::component::Component;
use cem_probe::{
    PropertiesUi,
    TrackChanges,
    label_and_value,
    label_and_value_with_config,
};
use cem_render::texture::channel::{
    ImageSender,
    UndecidedTextureSender,
};
use cem_solver::{
    FieldComponent,
    fdtd::{
        cpu::{
            FdtdCpuSolverInstance,
            LatticeForEach,
            project::{
                FdtdCpuImageProjection,
                FdtdCpuProjectionPass,
            },
        },
        wgpu::{
            FdtdWgpuSolverInstance,
            FdtdWgpuSolverState,
            FdtdWgpuTextureProjection,
            project::FdtdWgpuProjectionPass,
        },
    },
    project::{
        CreateProjection,
        FdtdImageTarget,
        ProjectionParameters,
        ProjectionPassAdd,
    },
};
use cem_util::egui::FilePickerConfig;
use nalgebra::{
    Matrix4,
    UnitVector3,
    Vector2,
};

#[derive(Clone, Debug, Component)]
pub struct Observer {
    pub write_to_gif: Option<PathBuf>,
    pub display_as_texture: bool,
    pub field: FieldComponent,
    pub color_map: Matrix4<f32>,
    pub half_extents: Vector2<f32>,
}

impl PropertiesUi for Observer {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        let mut changes = TrackChanges::default();

        let response = egui::Frame::new()
            .show(ui, |ui| {
                egui::ComboBox::from_id_salt(ui.id().with("field"))
                    .selected_text(FieldNames[self.field])
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.field,
                            FieldComponent::E,
                            FieldNames[FieldComponent::E],
                        );
                        ui.selectable_value(
                            &mut self.field,
                            FieldComponent::H,
                            FieldNames[FieldComponent::H],
                        );
                    });

                label_and_value_with_config(
                    ui,
                    "File",
                    &mut changes,
                    &mut self.write_to_gif,
                    &FilePickerConfig::Save,
                );
                label_and_value(ui, "Live", &mut changes, &mut self.display_as_texture);
            })
            .response;

        changes.propagated(response)
    }
}

pub fn test_color_map(scale: f32, axis: UnitVector3<f32>) -> Matrix4<f32> {
    let mut m = Matrix4::zeros();

    // scale axis, add a 0 (affine coordinates), and turn into row-vector
    let x = scale * axis.into_inner().to_homogeneous().transpose();

    // red (row 0) will be positive
    m.set_row(0, &x);

    // blue (row 2) will be negative
    m.set_row(2, &(-x));

    // alpha will be 1
    m[(3, 3)] = 1.0;

    m
}

struct FieldNames;

impl Index<FieldComponent> for FieldNames {
    type Output = &'static str;

    fn index(&self, index: FieldComponent) -> &Self::Output {
        match index {
            FieldComponent::E => &"Electric Field",
            FieldComponent::H => &"Magnetic Field",
        }
    }
}

#[derive(derive_more::From, Debug)]
pub struct TextureSenderTarget {
    pub texture_sender: UndecidedTextureSender,
}

/// note: we could of course implement ImageTarget directly on the ImageSender,
/// but the wgpu impl derives a projection for that which would then copy stuff
/// from gpu to cpu and back (and have a conflicting implemetation compile
/// error).
#[derive(Debug)]
pub struct CopyToTextureImageTarget {
    pub image_sender: ImageSender,
}

impl FdtdImageTarget for CopyToTextureImageTarget {
    type Pixel = image::Rgba<u8>;
    type Container = Vec<u8>;
    type Error = Infallible;

    fn size(&self) -> Vector2<u32> {
        self.image_sender.size()
    }

    fn with_image_buffer(
        &mut self,
        f: impl FnOnce(&mut image::ImageBuffer<Self::Pixel, Self::Container>),
    ) -> Result<(), Infallible> {
        let mut image_buffer = self.image_sender.update_image();
        f(&mut image_buffer);
        Ok(())
    }
}

#[derive(Debug)]
pub struct FdtdCpuTextureSenderProjection {
    pub projection: FdtdCpuImageProjection<CopyToTextureImageTarget>,
}

impl<Threading> CreateProjection<TextureSenderTarget> for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    type Projection = FdtdCpuTextureSenderProjection;

    fn create_projection(
        &self,
        state: &Self::State,
        target: TextureSenderTarget,
        parameters: &ProjectionParameters,
    ) -> FdtdCpuTextureSenderProjection {
        let _ = state;

        let image_sender = target.texture_sender.send_images();
        tracing::debug!(size = ?image_sender.size(), "creating projection with image sender");
        let projection =
            self.create_projection(state, CopyToTextureImageTarget { image_sender }, parameters);
        FdtdCpuTextureSenderProjection { projection }
    }
}

impl<'a, Threading> ProjectionPassAdd<'a, FdtdCpuTextureSenderProjection>
    for FdtdCpuProjectionPass<'a, Threading>
{
    fn add_projection(&mut self, projection: &'a mut FdtdCpuTextureSenderProjection) {
        self.add_projection(&mut projection.projection);
    }
}

#[derive(Debug)]
pub struct FdtdWgpuTextureSenderProjection {
    pub projection: FdtdWgpuTextureProjection,
}

impl CreateProjection<TextureSenderTarget> for FdtdWgpuSolverInstance {
    type Projection = FdtdWgpuTextureSenderProjection;

    fn create_projection(
        &self,
        state: &FdtdWgpuSolverState,
        target: TextureSenderTarget,
        parameters: &ProjectionParameters,
    ) -> FdtdWgpuTextureSenderProjection {
        let texture_sender = target.texture_sender.send_texture();
        tracing::debug!(size = ?texture_sender.size, format = ?texture_sender.format, "creating projection with texture sender");
        let projection = self.create_projection(state, texture_sender.texture.clone(), parameters);
        FdtdWgpuTextureSenderProjection { projection }
    }
}

impl<'a> ProjectionPassAdd<'a, FdtdWgpuTextureSenderProjection> for FdtdWgpuProjectionPass<'a> {
    fn add_projection(&mut self, projection: &'a mut FdtdWgpuTextureSenderProjection) {
        self.add_projection(&mut projection.projection);
    }
}
