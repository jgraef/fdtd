use std::{
    io::Write,
    time::Duration,
};

use colorgrad::Gradient;
use image::{
    Delay,
    Frame,
    RgbaImage,
    codecs::gif::{
        GifEncoder,
        Repeat,
    },
};
use nalgebra::{
    Point2,
    Vector2,
};

#[derive(derive_more::Debug)]
pub struct GifOutput<W, G>
where
    W: Write,
{
    size: Vector2<usize>,
    #[debug("image::GifEncoder {{ ... }}")]
    gif_encoder: GifEncoder<W>,
    gradient: G,
    frame_delay: Delay,
}

impl<W, G> GifOutput<W, G>
where
    W: Write,
{
    pub fn new(
        writer: W,
        size: Vector2<usize>,
        frame_delay: Duration,
        gradient: G,
    ) -> Result<Self, image::ImageError> {
        let mut gif_encoder = GifEncoder::new(writer);
        gif_encoder.set_repeat(Repeat::Infinite)?;

        Ok(Self {
            size,
            gif_encoder,
            gradient,
            frame_delay: Delay::from_saturating_duration(frame_delay),
        })
    }
}

impl<W, G> GifOutput<W, G>
where
    W: Write,
    G: Gradient,
{
    pub fn write_frame(
        &mut self,
        // todo: should we make this a `Fn`? then we can use the parallel variant to construct the
        // image
        mut value_at: impl FnMut(&Point2<usize>) -> f32,
    ) -> Result<(), image::ImageError> {
        let frame = RgbaImage::from_fn(self.size.x as u32, self.size.y as u32, |x, y| {
            let color = self
                .gradient
                .at(value_at(&Point2::new(x as usize, y as usize)));
            color.to_rgba8().into()
        });

        self.gif_encoder
            .encode_frame(Frame::from_parts(frame, 0, 0, self.frame_delay))?;
        Ok(())
    }
}
