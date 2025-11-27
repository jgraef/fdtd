use crate::util::wgpu::buffer::WriteStagingBelt;

pub mod read_belt;
pub mod write_belt;

#[derive(Debug)]
pub struct WriteStaging<'a> {
    belt: Option<&'a mut WriteStagingBelt>,
    device: &'a wgpu::Device,
    command_encoder: &'a mut wgpu::CommandEncoder,
    unmap_buffers: Vec<wgpu::Buffer>,
}

impl<'a> WriteStaging<'a> {
    pub fn new(device: &'a wgpu::Device, command_encoder: &'a mut wgpu::CommandEncoder) -> Self {
        Self {
            belt: None,
            device,
            command_encoder,
            unmap_buffers: vec![],
        }
    }

    pub fn with_belt(mut self, belt: &'a mut WriteStagingBelt) -> Self {
        belt.recall();
        self.belt = Some(belt);
        self
    }

    fn view_mut_and_copy(
        &mut self,
        size: wgpu::BufferSize,
        alignment: wgpu::BufferSize,
        copy: impl FnOnce(&mut wgpu::CommandEncoder, wgpu::BufferSlice),
    ) -> wgpu::BufferViewMut {
        if let Some(belt) = &mut self.belt {
            let staging_buffer_slice = belt.allocate(self.device, size, alignment);

            copy(self.command_encoder, staging_buffer_slice);

            staging_buffer_slice.get_mapped_range_mut()
        }
        else {
            let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("one-time write staging"),
                size: size.get(),
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::MAP_WRITE,
                mapped_at_creation: true,
            });

            copy(self.command_encoder, staging_buffer.slice(..));

            let view = staging_buffer.get_mapped_range_mut(..);

            self.unmap_buffers.push(staging_buffer);

            view
        }
    }

    pub fn write_buffer(&mut self, destination: wgpu::BufferSlice) -> wgpu::BufferViewMut {
        let offset = destination.offset();
        let size = destination.size();

        assert!(
            size.get().is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            "allocation size {size} must be a multiple of `COPY_BUFFER_ALIGNMENT`"
        );
        assert!(
            offset.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            "WriteStaging offset {offset} must be a multiple of `COPY_BUFFER_ALIGNMENT`"
        );

        self.view_mut_and_copy(
            size,
            wgpu::BufferSize::new(wgpu::COPY_BUFFER_ALIGNMENT).unwrap(),
            |command_encoder, staging_buffer_slice| {
                command_encoder.copy_buffer_to_buffer(
                    staging_buffer_slice.buffer(),
                    staging_buffer_slice.offset(),
                    destination.buffer(),
                    offset,
                    size.get(),
                );
            },
        )
    }

    pub fn write_buffer_from_slice(&mut self, destination: wgpu::BufferSlice, data: &[u8]) {
        assert_eq!(destination.size().get(), data.len() as wgpu::BufferAddress);
        let mut view = self.write_buffer(destination);
        view.copy_from_slice(data);
    }

    pub fn write_texture(
        &mut self,
        source_layout: TextureSourceLayout,
        destination: wgpu::TexelCopyTextureInfo,
        size: wgpu::Extent3d,
    ) -> wgpu::BufferViewMut {
        let mut copy_size = wgpu::BufferAddress::from(size.height)
            * wgpu::BufferAddress::from(source_layout.bytes_per_row);
        if size.depth_or_array_layers > 1 {
            let rows_per_image = source_layout.rows_per_image.expect("`rows_per_image` must be specified when copying with a size that has `depth_or_array_layers` > 1");
            copy_size *= wgpu::BufferAddress::from(size.depth_or_array_layers)
                * wgpu::BufferAddress::from(rows_per_image);
        }
        let copy_size = wgpu::BufferSize::new(copy_size).expect("Texture size must not be zero");

        // todo: multiple of texture block size
        let alignment = wgpu::BufferSize::new(wgpu::COPY_BUFFER_ALIGNMENT).unwrap();

        self.view_mut_and_copy(
            copy_size,
            alignment,
            |command_encoder, staging_buffer_slice| {
                command_encoder.copy_buffer_to_texture(
                    source_layout.into_texel_copy_buffer_info(staging_buffer_slice),
                    destination,
                    size,
                );
            },
        )
    }

    pub fn finish(self) {
        // drop self, the drop impl will do its job
    }

    pub fn active_chunk_sizes(&self) -> impl Iterator<Item = wgpu::BufferAddress> {
        self.belt.iter().flat_map(|belt| belt.active_chunk_sizes())
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

impl<'a> Drop for WriteStaging<'a> {
    fn drop(&mut self) {
        if let Some(belt) = &mut self.belt {
            belt.finish();
        }

        for buffer in self.unmap_buffers.drain(..) {
            buffer.unmap();
        }
    }
}
