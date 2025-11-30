pub mod buffer;

use nalgebra::Vector2;
use palette::Srgba;
use wgpu::util::DeviceExt;

#[cfg(feature = "image")]
pub use self::image::*;

pub fn create_texture(
    device: &wgpu::Device,
    size: &Vector2<u32>,
    usage: wgpu::TextureUsages,
    label: &str,
) -> wgpu::Texture {
    device.create_texture(&texture_descriptor(size, usage, label))
}

pub fn create_texture_view_from_texture(texture: &wgpu::Texture, label: &str) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(label),
        ..Default::default()
    })
}

pub fn create_texture_from_color(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color: &Srgba<u8>,
    usage: wgpu::TextureUsages,
    label: &str,
) -> wgpu::Texture {
    let color: [u8; 4] = (*color).into();
    device.create_texture_with_data(
        queue,
        &texture_descriptor(
            &Vector2::repeat(1),
            usage | wgpu::TextureUsages::COPY_DST,
            label,
        ),
        Default::default(),
        &color,
    )
}

pub fn texture_descriptor<'a>(
    size: &Vector2<u32>,
    usage: wgpu::TextureUsages,
    label: &'a str,
) -> wgpu::TextureDescriptor<'a> {
    wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // todo: need to be able to pick this. but usually we're working with srgba when
        // writing/reading a texture
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage,
        view_formats: &[],
    }
}

#[cfg(feature = "image")]
mod image {
    use std::ops::Deref;

    use nalgebra::Vector2;
    use wgpu::util::DeviceExt;

    use crate::{
        image::ImageSizeExt as _,
        wgpu::{
            buffer::{
                StagingBufferProvider,
                TextureSourceLayout,
                WriteStagingTransaction,
            },
            texture_descriptor,
        },
    };
    pub fn create_texture_from_image<Container>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &image::ImageBuffer<image::Rgba<u8>, Container>,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> wgpu::Texture
    where
        Container: Deref<Target = [u8]>,
    {
        let size = image.size();
        device.create_texture_with_data(
            queue,
            &texture_descriptor(&size, usage | wgpu::TextureUsages::COPY_DST, label),
            Default::default(),
            image.as_raw(),
        )
    }

    pub trait WriteImageToTextureExt {
        fn write_to_texture<P>(
            &self,
            texture: &wgpu::Texture,
            write_staging: &mut WriteStagingTransaction<P>,
        ) where
            P: StagingBufferProvider;
    }

    impl<Container> WriteImageToTextureExt for image::ImageBuffer<image::Rgba<u8>, Container>
    where
        Container: AsRef<[u8]> + Deref<Target = [u8]>,
    {
        fn write_to_texture<P>(
            &self,
            texture: &wgpu::Texture,
            write_staging: &mut WriteStagingTransaction<P>,
        ) where
            P: StagingBufferProvider,
        {
            // note: images with width < 256 need padding. we do this while copying the
            // image data into the staging buffer.
            //
            // https://docs.rs/wgpu/latest/wgpu/constant.COPY_BYTES_PER_ROW_ALIGNMENT.html

            let texture_size = Vector2::new(texture.width(), texture.height());

            let samples = self.as_flat_samples();

            let image_size = Vector2::new(samples.layout.width, samples.layout.height);
            assert_eq!(
                image_size, texture_size,
                "provided image size doesn't match texture"
            );
            assert_eq!(
                samples.layout.channel_stride, 1,
                "todo: channel stride not 4"
            );
            assert_eq!(samples.layout.width_stride, 4, "todo: width stride not 4");

            const BYTES_PER_PIXEL: usize = 4;
            let bytes_per_row_unpadded: u32 = samples.layout.width * BYTES_PER_PIXEL as u32;
            let bytes_per_row_padded =
                wgpu::util::align_to(bytes_per_row_unpadded, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

            let mut view = write_staging.write_texture(
                TextureSourceLayout {
                    bytes_per_row: bytes_per_row_padded,
                    rows_per_image: None,
                },
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: Default::default(),
                    aspect: Default::default(),
                },
                wgpu::Extent3d {
                    width: texture_size.x,
                    height: texture_size.y,
                    depth_or_array_layers: 1,
                },
            );

            let mut source_offset = 0;
            let mut destination_offset = 0;
            let n = bytes_per_row_unpadded as usize;

            for _ in 0..self.height() {
                view[destination_offset..][..n]
                    .copy_from_slice(&samples.samples[source_offset..][..n]);
                source_offset += samples.layout.height_stride;
                destination_offset += bytes_per_row_padded as usize;
            }
        }
    }
}
