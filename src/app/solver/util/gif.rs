use std::{
    io::Write,
    time::Duration,
};

use image::{
    Delay,
    Frame,
    RgbaImage,
    codecs::gif::{
        GifEncoder,
        Repeat,
    },
};
use nalgebra::Vector2;

use crate::app::solver::util::WriteImage;

#[derive(derive_more::Debug)]
pub struct GifOutput<W>
where
    W: Write,
{
    size: Vector2<u32>,
    #[debug("image::GifEncoder {{ ... }}")]
    gif_encoder: GifEncoder<W>,
    frame_delay: Delay,
}

impl<W> GifOutput<W>
where
    W: Write,
{
    pub fn new(
        writer: W,
        size: Vector2<u32>,
        frame_delay: Duration,
    ) -> Result<Self, image::ImageError> {
        let mut gif_encoder = GifEncoder::new(writer);
        gif_encoder.set_repeat(Repeat::Infinite)?;

        Ok(Self {
            size,
            gif_encoder,
            frame_delay: Delay::from_saturating_duration(frame_delay),
        })
    }
}

impl<W> WriteImage for GifOutput<W>
where
    W: Write,
{
    type Error = image::ImageError;

    fn size(&self) -> Vector2<u32> {
        self.size
    }

    fn write_image(&mut self, mut f: impl FnMut(&mut RgbaImage)) -> Result<(), Self::Error> {
        // unfortunately we have to allocate a new image buffer for each frame
        let mut image = RgbaImage::new(self.size.x, self.size.y);
        f(&mut image);
        self.gif_encoder
            .encode_frame(Frame::from_parts(image, 0, 0, self.frame_delay))?;
        Ok(())
    }
}
