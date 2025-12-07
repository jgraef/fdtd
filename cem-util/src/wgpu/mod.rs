pub mod buffer;

use std::num::NonZero;

use nalgebra::Vector2;
use palette::LinSrgba;

#[cfg(feature = "image")]
pub use self::image::*;
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

#[cfg(feature = "image")]
mod image {
    use std::num::NonZero;

    use image::imageops::FilterType;
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
            label: &'a str,
            usage: wgpu::TextureUsages,
            mip_level_count: NonZero<u32>,
        ) -> Result<wgpu::TextureDescriptor<'a>, UnsupportedColorSpace>;

        fn create_texture<S>(
            &self,
            label: &str,
            usage: wgpu::TextureUsages,
            mip_levels: MipLevels,
            device: &wgpu::Device,
            write_staging: S,
        ) -> Result<wgpu::Texture, UnsupportedColorSpace>
        where
            S: WriteStaging;

        fn write_to_texture<S>(&self, texture: &wgpu::Texture, write_staging: S)
        where
            S: WriteStaging,
        {
            self.write_to_texture_mip_level(texture, MipLevel::Original, write_staging);
        }

        fn write_to_texture_mip_level<S>(
            &self,
            texture: &wgpu::Texture,
            mip_level: MipLevel,
            write_staging: S,
        ) where
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
            label: &'a str,
            usage: wgpu::TextureUsages,
            mip_level_count: NonZero<u32>,
        ) -> Result<wgpu::TextureDescriptor<'a>, UnsupportedColorSpace> {
            Ok(texture_descriptor(
                label,
                &self.size(),
                usage,
                self.texture_format()?,
                mip_level_count,
            ))
        }

        fn create_texture<S>(
            &self,
            label: &str,
            usage: wgpu::TextureUsages,
            mip_levels: MipLevels,
            device: &wgpu::Device,
            mut write_staging: S,
        ) -> Result<wgpu::Texture, UnsupportedColorSpace>
        where
            S: WriteStaging,
        {
            let (mip_level_count, mip_levels) = mip_levels.get(self.size());

            let texture = device.create_texture(&self.texture_descriptor(
                label,
                usage | wgpu::TextureUsages::COPY_DST,
                mip_level_count,
            )?);

            let mut image_buffer;
            let mut previous_level = self;

            for mip_level in mip_levels {
                let current_level = match mip_level {
                    MipLevel::Original => previous_level,
                    MipLevel::Downsampled {
                        level,
                        size,
                        filter,
                    } => {
                        tracing::debug!(?level, ?size, ?filter, "creating mipmap for image");
                        image_buffer =
                            image::imageops::resize(previous_level, size.x, size.y, filter);
                        &image_buffer
                    }
                };

                current_level.write_to_texture_mip_level(&texture, mip_level, &mut write_staging);
                previous_level = current_level;
            }

            Ok(texture)
        }

        fn write_to_texture_mip_level<S>(
            &self,
            texture: &wgpu::Texture,
            mip_level: MipLevel,
            mut write_staging: S,
        ) where
            S: WriteStaging,
        {
            // note: images with width < 256 need padding. we do this while copying the
            // image data into the staging buffer.
            //
            // https://docs.rs/wgpu/latest/wgpu/constant.COPY_BYTES_PER_ROW_ALIGNMENT.html

            let texture_size = Vector2::new(texture.width(), texture.height());

            // get actual mip level number and the texture size we have at that level
            let (mip_level, mip_level_size) = match mip_level {
                MipLevel::Original => (0, texture_size),
                MipLevel::Downsampled {
                    level,
                    size,
                    filter: _,
                } => (level, size),
            };

            let samples = self.as_flat_samples();

            let image_size = Vector2::new(samples.layout.width, samples.layout.height);
            assert_eq!(
                image_size, mip_level_size,
                "provided image size ({image_size:?}) doesn't match texture size at this mip level ({mip_level_size:?} @ {mip_level})"
            );
            assert_eq!(samples.layout.channel_stride, 1, "channel stride not 4");
            assert_eq!(samples.layout.width_stride, 4, "width stride not 4");

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
                    mip_level,
                    origin: Default::default(),
                    aspect: Default::default(),
                },
                wgpu::Extent3d {
                    width: mip_level_size.x,
                    height: mip_level_size.y,
                    depth_or_array_layers: 1,
                },
            );

            let mut source_offset = 0;
            let mut destination_offset = 0;
            let n = bytes_per_row_unpadded as usize;

            for _ in 0..image_size.y {
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

    #[derive(Clone, Copy, Debug, Default)]
    pub enum MipLevels {
        #[default]
        One,
        Fixed {
            mip_level_count: NonZero<u32>,
            filter: FilterType,
        },
        Auto {
            filter: FilterType,
        },
    }

    impl MipLevels {
        pub fn get(&self, size: Vector2<u32>) -> (NonZero<u32>, impl Iterator<Item = MipLevel>) {
            let (mip_level_count, filter) = match self {
                MipLevels::One => (const { NonZero::new(1).unwrap() }, None),
                MipLevels::Fixed {
                    mip_level_count,
                    filter,
                } => (*mip_level_count, Some(*filter)),
                MipLevels::Auto { filter } => (mip_level_count_for_size(&size), Some(*filter)),
            };

            let mut current_size = size;
            let mut level = 1;
            let downsampled = std::iter::from_fn(move || {
                (level < mip_level_count.get()).then(|| {
                    current_size = current_size.map(|c| 1.max(c / 2));
                    let mip_level = MipLevel::Downsampled {
                        level,
                        size: current_size,
                        filter: filter.unwrap(),
                    };
                    level += 1;
                    mip_level
                })
            });

            (
                mip_level_count,
                [MipLevel::Original].into_iter().chain(downsampled),
            )
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum MipLevel {
        #[default]
        Original,
        Downsampled {
            level: u32,
            size: Vector2<u32>,
            filter: FilterType,
        },
    }

    impl MipLevel {
        pub fn level(&self) -> u32 {
            match self {
                MipLevel::Original => 0,
                MipLevel::Downsampled {
                    level,
                    size: _,
                    filter: _,
                } => *level,
            }
        }
    }

    pub fn mip_level_count_for_size(size: &Vector2<u32>) -> NonZero<u32> {
        let size = size.x.max(size.y);
        NonZero::new(1 + size.checked_ilog2().unwrap_or_default()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use image::imageops::FilterType;
    use nalgebra::Vector2;

    use crate::wgpu::{
        MipLevel,
        MipLevels,
    };

    #[test]
    fn one_mip_level() {
        let (num_levels, levels) = MipLevels::One.get(Vector2::repeat(512));
        assert_eq!(num_levels.get(), 1);
        assert_eq!(levels.collect::<Vec<_>>(), vec![MipLevel::Original]);
    }

    #[test]
    fn multiple_fixed_mip_levels() {
        let levels = MipLevels::Fixed {
            mip_level_count: NonZero::new(5).unwrap(),
            filter: FilterType::Nearest,
        };
        let (num_levels, levels) = levels.get(Vector2::repeat(512));
        let levels = levels.collect::<Vec<_>>();
        assert_eq!(num_levels.get(), 5);
        assert_eq!(levels.len(), 5);
        assert_eq!(levels[0], MipLevel::Original);
        assert_eq!(
            levels[1],
            MipLevel::Downsampled {
                level: 1,
                size: Vector2::repeat(256),
                filter: FilterType::Nearest
            }
        );
        assert_eq!(
            levels[2],
            MipLevel::Downsampled {
                level: 2,
                size: Vector2::repeat(128),
                filter: FilterType::Nearest
            }
        );
        assert_eq!(
            levels[3],
            MipLevel::Downsampled {
                level: 3,
                size: Vector2::repeat(64),
                filter: FilterType::Nearest
            }
        );
        assert_eq!(
            levels[4],
            MipLevel::Downsampled {
                level: 4,
                size: Vector2::repeat(32),
                filter: FilterType::Nearest
            }
        );
    }

    #[test]
    fn auto_mip_levels() {
        let levels = MipLevels::Auto {
            filter: FilterType::Nearest,
        };
        let (num_levels, levels) = levels.get(Vector2::repeat(16));
        let levels = levels.collect::<Vec<_>>();
        assert_eq!(num_levels.get(), 5);
        assert_eq!(levels.len(), 5);
        assert_eq!(levels[0], MipLevel::Original);
        assert_eq!(
            levels[1],
            MipLevel::Downsampled {
                level: 1,
                size: Vector2::repeat(8),
                filter: FilterType::Nearest
            }
        );
        assert_eq!(
            levels[2],
            MipLevel::Downsampled {
                level: 2,
                size: Vector2::repeat(4),
                filter: FilterType::Nearest
            }
        );
        assert_eq!(
            levels[3],
            MipLevel::Downsampled {
                level: 3,
                size: Vector2::repeat(2),
                filter: FilterType::Nearest
            }
        );
        assert_eq!(
            levels[4],
            MipLevel::Downsampled {
                level: 4,
                size: Vector2::repeat(1),
                filter: FilterType::Nearest
            }
        );
    }
}
