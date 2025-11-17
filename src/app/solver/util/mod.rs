pub mod gif;

use colorgrad::Gradient;
use image::RgbaImage;
use nalgebra::{
    Point2,
    Vector2,
};
use palette::Srgba;

pub trait WriteImage {
    type Error;

    fn size(&self) -> Vector2<u32>;

    /// this is a bit inflexible, because e.g. the TextureOutput doesn't require
    /// a &mut borrow. also, we might want to require a Fn closure, so we
    /// can use image's rayon feature.
    fn write_image(&mut self, f: impl FnMut(&mut RgbaImage)) -> Result<(), Self::Error>;

    fn write_colors(
        &mut self,
        mut color_at: impl FnMut(&Point2<u32>) -> Srgba,
    ) -> Result<(), Self::Error> {
        self.write_image(|image| {
            image.enumerate_pixels_mut().for_each(|(x, y, pixel)| {
                let color: [u8; 4] = color_at(&Point2::new(x, y)).into_format::<u8, u8>().into();
                *pixel = color.into();
            });
        })
    }

    fn write_field_values(
        &mut self,
        mut value_at: impl FnMut(&Point2<u32>) -> f32,
        gradient: &impl Gradient,
    ) -> Result<(), Self::Error> {
        self.write_colors(|point| gradient.at(value_at(point)).to_array().into())
    }
}
