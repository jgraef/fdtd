pub mod buffer;

use std::{
    ops::Deref,
    sync::Arc,
};

use nalgebra::Vector2;
use palette::Srgba;
use parking_lot::Mutex;
use wgpu::util::DeviceExt;

use crate::util::{
    ImageSizeExt,
    wgpu::buffer::{
        TextureSourceLayout,
        WriteStaging,
    },
};

#[derive(Clone, Debug, Default)]
struct AsyncResultBuf {
    buf: Arc<Mutex<Option<Result<(), wgpu::BufferAsyncError>>>>,
}

impl AsyncResultBuf {
    fn callback(&self) -> impl FnOnce(Result<(), wgpu::BufferAsyncError>) + 'static {
        let buf = self.buf.clone();
        move |result| {
            let mut buf = buf.lock();
            *buf = Some(result);
        }
    }

    fn unwrap(&self) -> Result<(), wgpu::BufferAsyncError> {
        let result = self.buf.lock().take();
        result.expect("map_buffer_on_submit hasn't finished yet")
    }
}

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
        // not sure, but it looks better without Srgb. The surface egui-wgpu uses is not srgba, but
        // wouldn't the conversion being taken care of?
        // format: wgpu::TextureFormat::Rgba8UnormSrgb,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage,
        view_formats: &[],
    }
}

pub trait WriteImageToTextureExt {
    fn write_to_texture(&self, texture: &wgpu::Texture, write_staging: &mut WriteStaging);
}

impl<Container> WriteImageToTextureExt for image::ImageBuffer<image::Rgba<u8>, Container>
where
    Container: AsRef<[u8]> + Deref<Target = [u8]>,
{
    fn write_to_texture(&self, texture: &wgpu::Texture, write_staging: &mut WriteStaging) {
        // todo: see https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#performance-considerations-2
        //
        // note: the 256 bytes per row alignment doesn't apply for Queue::write_texture:
        // https://docs.rs/wgpu/latest/wgpu/constant.COPY_BYTES_PER_ROW_ALIGNMENT.html

        let texture_size = Vector2::new(texture.width(), texture.height());

        let samples = self.as_flat_samples();

        let image_size = Vector2::new(samples.layout.width, samples.layout.height);
        assert_eq!(
            image_size, texture_size,
            "provided image size doesn't match texture"
        );

        let bytes_per_row: u32 = samples.layout.height_stride.try_into().unwrap();

        // this doesn't apply. of course I only read my comment above after implementing
        // it
        /*
        // declare outside of if, so it is still in scope after it
        let mut padded_buf;

        let (bytes_per_row_padded, data_padded) =
            if bytes_per_row_unpadded < wgpu::COPY_BYTES_PER_ROW_ALIGNMENT {
                // we need to pad the image

                let bytes_per_row_padded = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
                padded_buf = Vec::with_capacity(bytes_per_row_padded as usize * size.y as usize);

                for (y, row) in self.enumerate_rows() {
                    let row_offset = y as usize * bytes_per_row_padded as usize;
                    for (x, _y, pixel) in row {
                        let pixel_offset = row_offset + x as usize * BYTES_PER_PIXEL as usize;
                        padded_buf[pixel_offset..][..BYTES_PER_PIXEL as usize]
                            .copy_from_slice(&pixel.0);
                        //
                    }
                }

                (bytes_per_row_padded, &*padded_buf)
            }
            else {
                (bytes_per_row_unpadded, &**self.as_raw())
            };
        */

        let mut view = write_staging.write_texture(
            TextureSourceLayout {
                bytes_per_row,
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
        view.copy_from_slice(samples.samples);
    }
}
