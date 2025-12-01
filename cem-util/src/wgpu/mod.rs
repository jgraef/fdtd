pub mod buffer;

use nalgebra::Vector2;
use palette::LinSrgba;

#[cfg(feature = "image")]
pub use self::image::*;
use crate::wgpu::buffer::WriteStaging;

pub fn create_texture(
    size: &Vector2<u32>,
    usage: wgpu::TextureUsages,
    format: wgpu::TextureFormat,
    label: &str,
    device: &wgpu::Device,
) -> wgpu::Texture {
    device.create_texture(&texture_descriptor(size, usage, format, label))
}

pub fn create_texture_view_from_texture(texture: &wgpu::Texture, label: &str) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(label),
        ..Default::default()
    })
}

/// Creates a 1 by 1 pixel texture from the given color
pub fn create_texture_from_linsrgba<S>(
    color: LinSrgba<u8>,
    usage: wgpu::TextureUsages,
    label: &str,
    device: &wgpu::Device,
    mut write_staging: S,
) -> wgpu::Texture
where
    S: WriteStaging,
{
    let size = Vector2::repeat(1);

    let texture = create_texture(
        &size,
        usage | wgpu::TextureUsages::COPY_DST,
        wgpu::TextureFormat::Rgba8Unorm,
        label,
        device,
    );

    let mut view = write_staging.write_texture(
        TextureSourceLayout {
            // this must be padded
            bytes_per_row: wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
            rows_per_image: None,
        },
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: Default::default(),
            aspect: Default::default(),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );

    let color: [u8; 4] = color.into();
    view[..4].copy_from_slice(&color);

    texture
}

pub fn texture_descriptor<'a>(
    size: &Vector2<u32>,
    usage: wgpu::TextureUsages,
    format: wgpu::TextureFormat,
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
        format,
        usage,
        view_formats: &[],
    }
}

/// Layout of a texture in a buffer's memory.
///
/// This is [`TexelCopyBufferLayout`](wgpu::TexelCopyBufferLayout), but without
/// offset
#[derive(Clone, Copy, Debug)]
pub struct TextureSourceLayout {
    pub bytes_per_row: u32,
    pub rows_per_image: Option<u32>,
}

impl TextureSourceLayout {
    pub fn into_texel_copy_buffer_info<'buffer>(
        self,
        buffer_slice: wgpu::BufferSlice<'buffer>,
    ) -> wgpu::TexelCopyBufferInfo<'buffer> {
        wgpu::TexelCopyBufferInfo {
            buffer: buffer_slice.buffer(),
            layout: wgpu::TexelCopyBufferLayout {
                offset: buffer_slice.offset(),
                bytes_per_row: Some(self.bytes_per_row),
                rows_per_image: self.rows_per_image,
            },
        }
    }
}

#[cfg(feature = "image")]
mod image {
    use nalgebra::Vector2;

    use crate::{
        image::ImageSizeExt as _,
        wgpu::{
            TextureSourceLayout,
            buffer::WriteStaging,
            texture_descriptor,
        },
    };

    pub trait ImageTextureExt {
        fn texture_format(&self) -> Result<wgpu::TextureFormat, UnsupportedColorSpace>;

        fn texture_descriptor<'a>(
            &self,
            usage: wgpu::TextureUsages,
            label: &'a str,
        ) -> Result<wgpu::TextureDescriptor<'a>, UnsupportedColorSpace>;

        fn create_texture<S>(
            &self,
            usage: wgpu::TextureUsages,
            label: &str,
            device: &wgpu::Device,
            write_staging: S,
        ) -> Result<wgpu::Texture, UnsupportedColorSpace>
        where
            S: WriteStaging;

        fn write_to_texture<S>(&self, texture: &wgpu::Texture, write_staging: S)
        where
            S: WriteStaging;
    }

    impl ImageTextureExt for image::RgbaImage {
        fn texture_format(&self) -> Result<wgpu::TextureFormat, UnsupportedColorSpace> {
            let cicp = self.color_space();

            if cicp.primaries == image::metadata::CicpColorPrimaries::SRgb {
                match cicp.transfer {
                    image::metadata::CicpTransferCharacteristics::Linear => {
                        Ok(wgpu::TextureFormat::Rgba8Unorm)
                    }
                    image::metadata::CicpTransferCharacteristics::SRgb => {
                        Ok(wgpu::TextureFormat::Rgba8UnormSrgb)
                    }
                    _ => Err(UnsupportedColorSpace { cicp }),
                }
            }
            else {
                Err(UnsupportedColorSpace { cicp })
            }
        }

        fn texture_descriptor<'a>(
            &self,
            usage: wgpu::TextureUsages,
            label: &'a str,
        ) -> Result<wgpu::TextureDescriptor<'a>, UnsupportedColorSpace> {
            Ok(texture_descriptor(
                &self.size(),
                usage,
                self.texture_format()?,
                label,
            ))
        }

        fn create_texture<S>(
            &self,
            usage: wgpu::TextureUsages,
            label: &str,
            device: &wgpu::Device,
            write_staging: S,
        ) -> Result<wgpu::Texture, UnsupportedColorSpace>
        where
            S: WriteStaging,
        {
            let texture = device.create_texture(
                &self.texture_descriptor(usage | wgpu::TextureUsages::COPY_DST, label)?,
            );
            self.write_to_texture(&texture, write_staging);
            Ok(texture)
        }

        fn write_to_texture<S>(&self, texture: &wgpu::Texture, mut write_staging: S)
        where
            S: WriteStaging,
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

    #[derive(Clone, Copy, Debug, thiserror::Error)]
    #[error("Unsupported color space: primaries={:?}, transfer={:?}", .cicp.primaries, .cicp.transfer)]
    pub struct UnsupportedColorSpace {
        cicp: image::metadata::Cicp,
    }
}
