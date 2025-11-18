use std::{
    marker::PhantomData,
    sync::Arc,
};

use bytemuck::Pod;
use parking_lot::Mutex;

pub fn unpadded_buffer_size<T>(num_elements: usize) -> u64 {
    (std::mem::size_of::<T>() * num_elements) as u64
}

pub fn pad_buffer_size_for_copy(unpadded_size: u64) -> u64 {
    // https://github.com/gfx-rs/wgpu/blob/836c97056fb2c32852d1d8f6f45fefba1d1d6d26/wgpu/src/util/device.rs#L52
    // Valid vulkan usage is
    // 1. buffer size must be a multiple of COPY_BUFFER_ALIGNMENT.
    // 2. buffer size must be greater than 0.
    // Therefore we round the value up to the nearest multiple, and ensure it's at
    // least COPY_BUFFER_ALIGNMENT.
    const ALIGN_MASK: u64 = wgpu::COPY_BUFFER_ALIGNMENT - 1;
    ((unpadded_size + ALIGN_MASK) & !ALIGN_MASK).max(wgpu::COPY_BUFFER_ALIGNMENT)
}

pub fn buffer_usage_needs_padding(usage: wgpu::BufferUsages) -> bool {
    // Not sure if MAP_READ or MAP_WRITE needs padding. copying definitely needs it,
    // since the documentation of copy_buffer_to_buffer states that copies need to
    // be multiples of COPT_BUFFER_ALIGNMENT.
    //
    // I checked [wgpu::util::DownladBuffer][1], but it doesn't even pad the size.
    //
    // [1]: https://github.com/gfx-rs/wgpu/blob/836c97056fb2c32852d1d8f6f45fefba1d1d6d26/wgpu/src/util/mod.rs#L166
    usage.intersects(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC)
}

#[derive(Clone, Debug)]
pub struct TypedArrayBuffer<T> {
    buffer: wgpu::Buffer,
    num_elements: usize,
    unpadded_buffer_size: u64,
    padded_buffer_size: u64,
    _phantom: PhantomData<[T]>,
}

impl<T> TypedArrayBuffer<T> {
    fn new_impl(
        device: &wgpu::Device,
        label: &str,
        num_elements: usize,
        usage: wgpu::BufferUsages,
        mapped_at_creation: bool,
    ) -> Self {
        assert_ne!(num_elements, 0);

        let unpadded_buffer_size = unpadded_buffer_size::<T>(num_elements);
        let padded_buffer_size = if mapped_at_creation || buffer_usage_needs_padding(usage) {
            pad_buffer_size_for_copy(unpadded_buffer_size)
        }
        else {
            unpadded_buffer_size
        };

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: padded_buffer_size,
            usage,
            mapped_at_creation,
        });

        Self {
            buffer,
            num_elements,
            unpadded_buffer_size,
            padded_buffer_size,
            _phantom: PhantomData,
        }
    }

    pub fn new(device: &wgpu::Device, label: &str, usage: wgpu::BufferUsages, size: usize) -> Self {
        Self::new_impl(device, label, size, usage, false)
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub fn len(&self) -> usize {
        self.num_elements
    }
}

impl<T> TypedArrayBuffer<T>
where
    T: Pod,
{
    pub fn from_data(
        device: &wgpu::Device,
        label: &str,
        usage: wgpu::BufferUsages,
        data: &[T],
    ) -> Self {
        Self::from_fn(device, label, data.len(), usage, |index| data[index])
    }

    pub fn from_fn(
        device: &wgpu::Device,
        label: &str,
        num_elements: usize,
        usage: wgpu::BufferUsages,
        mut fill: impl FnMut(usize) -> T,
    ) -> Self {
        let buffer = Self::new_impl(device, label, num_elements, usage, true);

        {
            let mut view = buffer
                .buffer
                .get_mapped_range_mut(..buffer.unpadded_buffer_size);
            let view: &mut [T] = bytemuck::cast_slice_mut(view.as_mut());
            view.iter_mut()
                .enumerate()
                .for_each(|(index, value)| *value = fill(index));
        }

        buffer.buffer.unmap();

        buffer
    }

    pub fn read<R>(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mut f: impl FnMut(&[T]) -> R,
    ) -> Result<R, wgpu::BufferAsyncError> {
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read_staging"),
            size: self.padded_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut command_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fdtd/read"),
        });

        // note: a general copy_from method on TypedArrayBuffer doesn't work, since the
        // source offset might not always be aligned.
        command_encoder.copy_buffer_to_buffer(
            &self.buffer,
            0,
            &staging,
            0,
            self.padded_buffer_size,
        );

        let result_buf = Arc::new(Mutex::new(None));

        command_encoder.map_buffer_on_submit(&staging, wgpu::MapMode::Read, .., {
            let result_buf = result_buf.clone();
            move |result| {
                let mut result_buf = result_buf.lock();
                *result_buf = Some(result);
            }
        });

        let submission_index = queue.submit([command_encoder.finish()]);

        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: None,
            })
            .expect("device poll failed");

        let result = result_buf.lock().take();
        result.expect("map_buffer_on_submit hasn't finished yet")?;

        let mapped = staging.get_mapped_range(..self.unpadded_buffer_size);
        let view: &[T] = bytemuck::cast_slice(mapped.as_ref());

        let output = f(view);

        drop(mapped);
        staging.unmap();

        Ok(output)
    }
}
