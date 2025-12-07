pub mod buffer;

#[cfg(feature = "wgpu-image")]
pub mod image;

use std::num::NonZero;

use nalgebra::Vector2;
use palette::LinSrgba;

use crate::wgpu::buffer::WriteStaging;

pub fn create_texture(
    label: &str,
    size: &Vector2<u32>,
    usage: wgpu::TextureUsages,
    format: wgpu::TextureFormat,
    mip_level_count: NonZero<u32>,
    device: &wgpu::Device,
) -> wgpu::Texture {
    device.create_texture(&texture_descriptor(
        label,
        size,
        usage,
        format,
        mip_level_count,
    ))
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
        label,
        &size,
        usage | wgpu::TextureUsages::COPY_DST,
        wgpu::TextureFormat::Rgba8Unorm,
        const { NonZero::new(1).unwrap() },
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
    label: &'a str,
    size: &Vector2<u32>,
    usage: wgpu::TextureUsages,
    format: wgpu::TextureFormat,
    mip_level_count: NonZero<u32>,
) -> wgpu::TextureDescriptor<'a> {
    wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: mip_level_count.get(),
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
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
