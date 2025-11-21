use std::ops::{
    Deref,
    DerefMut,
};

use image::{
    ImageBuffer,
    Rgba,
    RgbaImage,
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

use crate::{
    app::solver::{
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
            TextureTarget,
        },
    },
    util::{
        image_size,
        wgpu::write_image_to_texture,
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
    Target: ImageTarget,
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

impl<'a, Target> ProjectionPassAdd<ImageProjection<Target>> for FdtdCpuProjectionPass<'a>
where
    Target: ImageTarget<Pixel = image::Rgba<u8>>,
{
    fn add_projection<'b>(&'b mut self, projection: &'b mut ImageProjection<Target>) {
        projection.target.with_image_buffer(|image| {
            self.project_to_image(image, &projection.parameters);
        });
    }
}

#[derive(Debug)]
pub struct TextureProjection {
    texture: wgpu::Texture,
    queue: wgpu::Queue,
    parameters: ProjectionParameters,
    frame_buffer: RgbaImage,
}

impl<'a, Threading> CreateProjection<TextureTarget<'a>> for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
{
    type Projection = TextureProjection;

    fn create_projection(
        &self,
        state: &Self::State,
        target: TextureTarget<'a>,
        parameters: &ProjectionParameters,
    ) -> TextureProjection {
        let _ = state;

        let frame_buffer = RgbaImage::new(target.texture.width(), target.texture.height());

        TextureProjection {
            texture: target.texture.clone(),
            queue: target.queue.clone(),
            parameters: *parameters,
            frame_buffer,
        }
    }
}

impl<'a> ProjectionPassAdd<TextureProjection> for FdtdCpuProjectionPass<'a> {
    fn add_projection<'b>(&'b mut self, projection: &'b mut TextureProjection) {
        self.project_to_image(&mut projection.frame_buffer, &projection.parameters);
        write_image_to_texture(
            &projection.queue,
            &projection.frame_buffer,
            &projection.texture,
        );
    }
}

impl BeginProjectionPass for FdtdCpuSolverInstance {
    type ProjectionPass<'a>
        = FdtdCpuProjectionPass<'a>
    where
        Self: 'a;

    fn begin_projection_pass<'a>(
        &'a self,
        state: &'a FdtdCpuSolverState,
    ) -> FdtdCpuProjectionPass<'a> {
        FdtdCpuProjectionPass::new(self, state)
    }
}

#[derive(Debug)]
pub struct FdtdCpuProjectionPass<'a> {
    instance: &'a FdtdCpuSolverInstance,
    state: &'a FdtdCpuSolverState,
    swap_buffer_index: SwapBufferIndex,
}

impl<'a> FdtdCpuProjectionPass<'a> {
    fn new(instance: &'a FdtdCpuSolverInstance, state: &'a FdtdCpuSolverState) -> Self {
        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick + 1);

        Self {
            instance,
            state,
            swap_buffer_index,
        }
    }

    fn project_to_image<Container>(
        &self,
        image: &mut ImageBuffer<Rgba<u8>, Container>,
        parameters: &ProjectionParameters,
    ) where
        Container: Deref<Target = [u8]> + DerefMut,
    {
        let image_size = (image_size(image) - Vector2::repeat(1)).cast::<f32>();

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
                *pixel = Rgba(Default::default());
            }
        });
    }
}

impl<'a> ProjectionPass for FdtdCpuProjectionPass<'a> {
    fn finish(self) {
        // we do projections immediately, so there's nothing to do here
    }
}
