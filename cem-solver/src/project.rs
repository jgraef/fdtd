//! Projections from the simulation domain
//!
//! For now this handles only projections from the 3D volume of a simulation
//! domain into a 2D planar surface. The projection is mainly defined by
//! [`ProjectionParameters`], but additional machinery is required to allow
//! projections into different targets (such as [`wgpu::Texture`] and
//! [`image::ImageBuffer`]) using different backends (e.g. a wgpu backend
//! requires a lot of preparation).
//!
//! A rough outline on how to project an image:
//!
//! 1. Choose a target:
//!  - [`wgpu::Texture`] wrapped in a [`TextureTarget`].
//!  - [`ImageTarget`]: trait implemented for anything that provide a
//!    [`image::ImageBuffer`] to render to.
//! 2. Create projection with one of the implementations of
//!    [`CreateProjection::create_projection`] on a [`SolverInstance`].
//! 3. In the simulation loop, start a projection pass with
//!    [`BeginProjectionPass::begin_projection_pass`] (implemented on
//!    [`SolverInstance`]s).
//! 4. Put all your projections into the pass using
//!    [`ProjectionPassAdd::add_projection`]. This is implemented on the
//!    projection pass you've created, if the backend supports the target.
//! 5. Finish projections using [`ProjectionPassFinish::finish`]

use std::{
    io::Write,
    ops::DerefMut,
};

use nalgebra::{
    Matrix4,
    Vector2,
};

use crate::{
    FieldComponent,
    SolverInstance,
};

/// Parameters for a projection
#[derive(Clone, Debug)]
pub struct ProjectionParameters {
    /// An image plane (z=0) will be transformed by this homogeneous matrix. The
    /// resulting plane defines the points sampled from the simulation
    /// domain.
    pub projection: Matrix4<f32>,

    /// Which field component to sample
    ///
    /// # TODO
    ///
    /// - We should probably allow sampling both. Then we'd need a bigger
    ///   `color_map` matrix.
    pub field: FieldComponent,

    /// Linear map from field vector to color.
    ///
    /// The field vector is first expanded to `(x, y, z, 1)`, so translations
    /// can be achieved. The output vector will be interpreted as linear
    /// `(r, g, b, a)`.
    pub color_map: Matrix4<f32>,

    // at the moment this is just wgsl source code
    // todo: this should be some proper type
    pub color_map_code: Option<String>,
}

/// Trait for [`SolverInstance`]s that can create projections to a specific
/// target.
///
/// The overly convoluted where clause forces that implementors of this trait,
/// also can begin a projection pass which accepts the returned projections.
pub trait CreateProjection<Target>: SolverInstance + BeginProjectionPass
where
    for<'a> <Self as BeginProjectionPass>::ProjectionPass<'a>:
        ProjectionPassAdd<'a, Self::Projection>,
{
    /// A projection handle that can then be passed to the
    /// [`ProjectionPassAdd::add_projection`].
    type Projection;

    /// Create the projection.
    ///
    /// todo: can we remove the state parameter? wgpu needs it to create the
    /// bind group, but we could create the bind group when needed.
    fn create_projection(
        &self,
        state: &Self::State,
        target: Target,
        parameters: &ProjectionParameters,
    ) -> Self::Projection;
}

/// Trait for [`SolverInstance`]s that can run projection passes.
pub trait BeginProjectionPass: SolverInstance {
    /// The projection pass which can be used to add projections to it.
    ///
    /// After all projections have been added, call
    /// [`ProjectionPassFinish::finish`].
    type ProjectionPass<'a>: ProjectionPass
    where
        Self: 'a;

    /// Begin a new projection pass.
    ///
    /// # FIXME
    ///
    /// This signature allows multiple passes to be run concurrently. This might
    /// break on wgpu.
    fn begin_projection_pass<'a>(&'a self, state: &'a Self::State) -> Self::ProjectionPass<'a>;
}

/// Trait required for projection passes.
///
/// This only contains the [`finish`][Self::finish] method, because the
/// [`add_projection`][ProjectionPassAdd::add_projection] are implemented in a
/// separate trait.
pub trait ProjectionPass {
    /// Finish the projection pass.
    fn finish(self);
}

/// Trait for projection passes that can accept `Projection`s to be added.
///
/// If a [`SolverInstance`] allows for a projection to be created, it must also
/// accept them to be added to its projection pass.
pub trait ProjectionPassAdd<'a, Projection>: 'a {
    fn add_projection(&mut self, projection: &'a mut Projection);
}

/// A generic image target.
///
/// This only requires that it can provide a [`image::ImageBuffer`] when asked,
/// so it can be used to sample into a single image, or into an encoder that
/// creates an animation.
pub trait ImageTarget {
    type Pixel: image::Pixel;
    type Container: DerefMut<Target = [<Self::Pixel as image::Pixel>::Subpixel]>;

    fn size(&self) -> Vector2<u32>;

    fn with_image_buffer(
        &mut self,
        f: impl FnOnce(&mut image::ImageBuffer<Self::Pixel, Self::Container>),
    );
}

impl<T> ImageTarget for &mut T
where
    T: ImageTarget,
{
    type Pixel = T::Pixel;
    type Container = T::Container;

    fn size(&self) -> Vector2<u32> {
        T::size(*self)
    }

    fn with_image_buffer(
        &mut self,
        f: impl FnOnce(&mut image::ImageBuffer<Self::Pixel, Self::Container>),
    ) {
        T::with_image_buffer(*self, f);
    }
}

impl<Pixel, Container> ImageTarget for image::ImageBuffer<Pixel, Container>
where
    Pixel: image::Pixel,
    Container: DerefMut<Target = [<Pixel as image::Pixel>::Subpixel]>,
{
    type Pixel = Pixel;
    type Container = Container;

    fn size(&self) -> Vector2<u32> {
        Vector2::new(self.width(), self.height())
    }

    fn with_image_buffer(&mut self, f: impl FnOnce(&mut image::ImageBuffer<Pixel, Container>)) {
        f(self)
    }
}

/// A target that renders the sampled images as frames into a GIF.
#[derive(derive_more::Debug)]
pub struct GifEncoder<Writer>
where
    Writer: Write,
{
    #[debug("image::GifEncoder {{ ... }}")]
    pub gif_encoder: image::codecs::gif::GifEncoder<Writer>,
    pub frame_size: Vector2<u32>,
    pub frame_delay: image::Delay,
}

impl<Writer> ImageTarget for GifEncoder<Writer>
where
    Writer: Write,
{
    type Pixel = image::Rgba<u8>;
    type Container = Vec<u8>;

    fn size(&self) -> Vector2<u32> {
        self.frame_size
    }

    fn with_image_buffer(
        &mut self,
        f: impl FnOnce(&mut image::ImageBuffer<image::Rgba<u8>, Vec<u8>>),
    ) {
        let mut buffer = image::RgbaImage::new(self.frame_size.x, self.frame_size.y);

        f(&mut buffer);

        let frame = image::Frame::from_parts(buffer, 0, 0, self.frame_delay);
        self.gif_encoder
            .encode_frame(frame)
            .unwrap_or_else(|error| todo!("handle error: {error}"));
    }
}
