use std::ops::{
    Deref,
    DerefMut,
};

use nalgebra::{
    Point3,
    Vector2,
    Vector4,
};
use palette::{
    Clamp,
    LinSrgba,
    Srgba,
};

use crate::app::{
    composer::renderer::texture_channel::{
        ImageSender,
        UndecidedTextureSender,
    },
    solver::{
        fdtd::{
            cpu::{
                FdtdCpuSolverInstance,
                FdtdCpuSolverState,
                LatticeForEach,
            },
            util::SwapBufferIndex,
        },
        project::{
            BeginProjectionPass,
            CreateProjection,
            ImageTarget,
            ProjectionParameters,
            ProjectionPass,
            ProjectionPassAdd,
        },
    },
};

#[derive(Debug)]
pub struct ImageProjection<Target>
where
    Target: ImageTarget,
{
    target: Target,
    parameters: ProjectionParameters,
}

impl<Threading, Target> CreateProjection<Target> for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
    Target: ImageTarget<Pixel = image::Rgba<u8>>,
{
    type Projection = ImageProjection<Target>;

    fn create_projection(
        &self,
        state: &FdtdCpuSolverState,
        target: Target,
        parameters: &ProjectionParameters,
    ) -> ImageProjection<Target> {
        let _ = state;
        ImageProjection {
            target,
            parameters: *parameters,
        }
    }
}

impl<'a, Threading, Target> ProjectionPassAdd<'a, ImageProjection<Target>>
    for FdtdCpuProjectionPass<'a, Threading>
where
    Target: ImageTarget<Pixel = image::Rgba<u8>>,
{
    fn add_projection(&mut self, projection: &'a mut ImageProjection<Target>) {
        projection.target.with_image_buffer(|image| {
            self.project_to_image(image, &projection.parameters);
        });
    }
}

// Todo: this is still kind of awkward, since it needs the Queue. Also we have a
// TextureWrite that shares an image buffer with the rendering engine and will
// write them to GPU before rendering.
#[derive(Debug)]
pub struct TextureSenderProjection {
    image_sender: ImageSender,
    parameters: ProjectionParameters,
}

impl<Threading> CreateProjection<UndecidedTextureSender> for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    type Projection = TextureSenderProjection;

    fn create_projection(
        &self,
        state: &Self::State,
        target: UndecidedTextureSender,
        parameters: &ProjectionParameters,
    ) -> TextureSenderProjection {
        let _ = state;

        let image_sender = target.send_images(&parameters.size, "fdtd-cpu/projection");

        TextureSenderProjection {
            image_sender,
            parameters: *parameters,
        }
    }
}

impl<'a, Threading> ProjectionPassAdd<'a, TextureSenderProjection>
    for FdtdCpuProjectionPass<'a, Threading>
{
    fn add_projection(&mut self, projection: &'a mut TextureSenderProjection) {
        let mut image_buffer = projection.image_sender.update_image();
        self.project_to_image(&mut image_buffer, &projection.parameters);
    }
}

impl<Threading> BeginProjectionPass for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    type ProjectionPass<'a>
        = FdtdCpuProjectionPass<'a, Threading>
    where
        Self: 'a;

    fn begin_projection_pass<'a>(
        &'a self,
        state: &'a FdtdCpuSolverState,
    ) -> FdtdCpuProjectionPass<'a, Threading> {
        FdtdCpuProjectionPass::new(self, state)
    }
}

#[derive(Debug)]
pub struct FdtdCpuProjectionPass<'a, Threading> {
    instance: &'a FdtdCpuSolverInstance<Threading>,
    state: &'a FdtdCpuSolverState,
    swap_buffer_index: SwapBufferIndex,
}

impl<'a, Threading> FdtdCpuProjectionPass<'a, Threading> {
    fn new(instance: &'a FdtdCpuSolverInstance<Threading>, state: &'a FdtdCpuSolverState) -> Self {
        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick + 1);

        Self {
            instance,
            state,
            swap_buffer_index,
        }
    }

    fn project_to_image<Container>(
        &self,
        image: &mut image::ImageBuffer<image::Rgba<u8>, Container>,
        parameters: &ProjectionParameters,
    ) where
        Container: Deref<Target = [u8]> + DerefMut,
    {
        let image_size = (image.size() - Vector2::repeat(1)).cast::<f32>();

        // todo: par_iter depending on `Threading`
        image.enumerate_pixels_mut().for_each(|(x, y, pixel)| {
            let uv = Vector2::new(x, y).cast::<f32>().component_div(&image_size);
            let point = Vector4::new(uv.x, uv.y, 0.0, 1.0);
            let projected_point =
                (parameters.projection * point).map(|c| c.round().max(0.0) as usize);

            let field = &self.state.field(parameters.field)[self.swap_buffer_index];
            if let Some(value) =
                field.get_point(&self.instance.strider, &projected_point.xyz().into())
            {
                let color =
                    parameters.color_map * Point3::from(value.cast::<f32>()).to_homogeneous();

                // convert to srgba
                let color: Srgba = LinSrgba::from(color.data.0[0]).clamp().into_encoding();
                // convert to u8
                let color: Srgba<u8> = color.into_format();

                pixel.0 = color.into();
            }
            else {
                *pixel = image::Rgba(Default::default());
            }
        });
    }
}

impl<'a, Threading> ProjectionPass for FdtdCpuProjectionPass<'a, Threading> {
    fn finish(self) {
        // we do projections immediately, so there's nothing to do here
    }
}
