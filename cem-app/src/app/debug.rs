use std::{
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use cem_util::format_size;

use crate::app::start::WgpuContext;

pub trait DebugUi {
    fn show_debug(&self, ui: &mut egui::Ui);
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
