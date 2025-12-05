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

use crate::{
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
        FdtdImageTarget,
        ProjectionParameters,
        ProjectionPass,
        ProjectionPassAdd,
    },
};

#[derive(Debug)]
pub struct FdtdCpuImageProjection<Target>
where
    Target: FdtdImageTarget,
{
    target: Target,
    parameters: ProjectionParameters,
}

impl<Threading, Target> CreateProjection<Target> for FdtdCpuSolverInstance<Threading>
where
    Threading: LatticeForEach,
    Target: FdtdImageTarget<Pixel = image::Rgba<u8>>,
{
    type Projection = FdtdCpuImageProjection<Target>;

    fn create_projection(
        &self,
        state: &FdtdCpuSolverState,
        target: Target,
        parameters: &ProjectionParameters,
    ) -> FdtdCpuImageProjection<Target> {
        let _ = state;
        FdtdCpuImageProjection {
            target,
            parameters: parameters.clone(),
        }
    }
}

impl<'a, Threading, Target> ProjectionPassAdd<'a, FdtdCpuImageProjection<Target>>
    for FdtdCpuProjectionPass<'a, Threading>
where
    Target: FdtdImageTarget<Pixel = image::Rgba<u8>>,
{
    fn add_projection(&mut self, projection: &'a mut FdtdCpuImageProjection<Target>) {
        if let Err(error) = projection.target.with_image_buffer(|image| {
            self.project_to_image(image, &projection.parameters);
        }) {
            self.errors.push(Box::new(error));
        }
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
    errors: Vec<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl<'a, Threading> FdtdCpuProjectionPass<'a, Threading> {
    fn new(instance: &'a FdtdCpuSolverInstance<Threading>, state: &'a FdtdCpuSolverState) -> Self {
        let swap_buffer_index = SwapBufferIndex::from_tick(state.tick + 1);

        Self {
            instance,
            state,
            swap_buffer_index,
            errors: vec![],
        }
    }

    pub fn project_to_image<Container>(
        &self,
        image: &mut image::ImageBuffer<image::Rgba<u8>, Container>,
        parameters: &ProjectionParameters,
    ) where
        Container: Deref<Target = [u8]> + DerefMut,
    {
        let image_size_scaling = (image.size() + Vector2::repeat(1)).cast::<f32>();

        // todo: par_iter depending on `Threading`
        image.enumerate_pixels_mut().for_each(|(x, y, pixel)| {
            // map image pixel to [0, 1]^2
            let mut uv = Vector2::new(x, y)
                .cast::<f32>()
                .component_div(&image_size_scaling);

            // images have y-axis flipped relative to our coordinate system
            uv.y = 1.0 - uv.y;

            // project point
            let projected_point = parameters.projection * Vector4::new(uv.x, uv.y, 0.0, 1.0);

            // map point to lattice coordinates
            let lattice_point = Point3::from(
                projected_point
                    .xyz()
                    .zip_map(self.instance.strider.size(), |c, s| {
                        ((c * (s as f32 - 1.0)).round().max(0.0) as usize).min(s - 1)
                    }),
            );

            let field = &self.state.field(parameters.field)[self.swap_buffer_index];
            if let Some(value) = field.get_point(&self.instance.strider, &lattice_point) {
                let color =
                    parameters.color_map * Point3::from(value.cast::<f32>()).to_homogeneous();

                // convert to srgba
                let color: Srgba = LinSrgba::from(color.data.0[0]).clamp().into_encoding();

                // convert to u8
                let color: Srgba<u8> = color.into_format();

                pixel.0 = color.into();
            }
            else {
                pixel.0 = [255, 0, 255, 255];
            }
        });
    }
}

impl<'a, Threading> ProjectionPass for FdtdCpuProjectionPass<'a, Threading> {
    type Error = FdtdCpuProjectionPassError;

    fn finish(self) -> Result<(), FdtdCpuProjectionPassError> {
        // we do projections immediately, so there's nothing to do here

        if self.errors.is_empty() {
            Ok(())
        }
        else {
            Err(FdtdCpuProjectionPassError {
                errors: self.errors,
            })
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("fdtd-cpu projection pass error")]
pub struct FdtdCpuProjectionPassError {
    pub errors: Vec<Box<dyn std::error::Error + Send + Sync + 'static>>,
}
