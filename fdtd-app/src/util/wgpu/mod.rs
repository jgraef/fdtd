pub mod buffer;

use std::{
    ops::Deref,
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use nalgebra::Vector2;
use palette::Srgba;
use wgpu::util::DeviceExt;

use crate::{
    app::composer::DebugUi,
    util::{
        ImageSizeExt,
        format_size,
        wgpu::buffer::{
            StagingBufferProvider,
            StagingPool,
            TextureSourceLayout,
            WriteStagingTransaction,
        },
    },
};

#[derive(Clone, Debug)]
pub struct WgpuContext {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: Arc<wgpu::AdapterInfo>,
    pub staging_pool: StagingPool,
}

impl WgpuContext {
    pub fn new(adapter: wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let adapter_info = Arc::new(adapter.get_info());
        Self {
            adapter,
            device,
            queue,
            adapter_info,
            staging_pool: StagingPool::new(wgpu::BufferSize::new(0x1000).unwrap(), "staging pool"),
        }
    }
}

impl DebugUi for WgpuContext {
    fn show_debug(&self, ui: &mut egui::Ui) {
        let device_info = get_wgpu_device_info(&self.device, ui.ctx());
        let staging_belt_info = self.staging_pool.info();

        ui.small("Adapter");
        ui.label(format!(
            "{} ({:04x}:{:04x})",
            self.adapter_info.name, self.adapter_info.vendor, self.adapter_info.device
        ));
        ui.small("Backend");
        ui.label(format!("{:?}", self.adapter_info.backend));
        ui.small("Driver");
        ui.label(format!(
            "{} ({})",
            self.adapter_info.driver, self.adapter_info.driver_info
        ));
        ui.small("Device type");
        ui.label(format!("{:?}", self.adapter_info.device_type));

        if let Some(report) = &device_info.allocator_report {
            ui.separator();

            ui.label("Allocator report:");
            ui.indent(egui::Id::NULL, |ui| {
                ui.label(format!(
                    "Total allocated: {}",
                    format_size(report.total_allocated_bytes)
                ));
                ui.label(format!(
                    "Total reserved: {}",
                    format_size(report.total_reserved_bytes)
                ));
                for allocation in &report.allocations {
                    ui.label(format!(
                        "{}: {}",
                        allocation.name,
                        format_size(allocation.size)
                    ));
                }
            });
        }

        ui.separator();

        ui.label("Staging belt:");
        ui.indent(egui::Id::NULL, |ui| {
            ui.label(format!(
                "In-flight chunks: {}",
                staging_belt_info.in_flight_count
            ));
            ui.label(format!("Free chunks: {}", staging_belt_info.free_count));
            ui.label(format!(
                "Total allocations: {} chunks, {}",
                staging_belt_info.total_allocation_count,
                format_size(staging_belt_info.total_allocation_bytes)
            ));
        });
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
            view[destination_offset..][..n].copy_from_slice(&samples.samples[source_offset..][..n]);
            source_offset += samples.layout.height_stride as usize;
            destination_offset += bytes_per_row_padded as usize;
        }
    }
}

pub fn get_wgpu_device_info(device: &wgpu::Device, ctx: &egui::Context) -> Arc<DeviceInfo> {
    #[derive(Clone)]
    struct Container {
        device_info: Arc<DeviceInfo>,
        expiry: Instant,
    }

    let mut report_buf = ctx.data(|data| data.get_temp::<Container>(egui::Id::NULL));
    let now = Instant::now();

    if let Some(report) = &report_buf
        && report.expiry < now
    {
        report_buf = None;
    }

    if let Some(report) = report_buf {
        report.device_info
    }
    else {
        let allocator_report = device.generate_allocator_report();
        //let internal_counters = device.get_internal_counters();

        let device_info = Arc::new(DeviceInfo {
            allocator_report,
            //internal_counters,
        });

        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::NULL,
                Container {
                    device_info: device_info.clone(),
                    expiry: now + Duration::from_secs(1),
                },
            );
        });

        device_info
    }
}

pub struct DeviceInfo {
    pub allocator_report: Option<wgpu::AllocatorReport>,
    //pub internal_counters: wgpu::InternalCounters,
}
