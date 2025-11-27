use std::{
    borrow::Cow,
    sync::Arc,
};

use parking_lot::RwLock;

#[derive(Debug)]
pub struct WriteStagingTransaction<'a, P>
where
    P: StagingBufferProvider,
{
    provider: P,
    device: &'a wgpu::Device,
    command_encoder: &'a mut wgpu::CommandEncoder,
    total_staged: wgpu::BufferAddress,
}

impl<'a, P> WriteStagingTransaction<'a, P>
where
    P: StagingBufferProvider,
{
    pub fn new(
        provider: P,
        device: &'a wgpu::Device,
        command_encoder: &'a mut wgpu::CommandEncoder,
    ) -> Self {
        Self {
            provider,
            device,
            command_encoder,
            total_staged: 0,
        }
    }

    pub fn total_staged(&self) -> wgpu::BufferAddress {
        self.total_staged
    }

    fn view_mut_and_copy(
        &mut self,
        size: wgpu::BufferSize,
        alignment: wgpu::BufferSize,
        copy: impl FnOnce(&mut wgpu::CommandEncoder, wgpu::BufferSlice),
    ) -> wgpu::BufferViewMut {
        assert!(
            size.get().is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            "WriteStagingBelt allocation size {size} must be a multiple of `COPY_BUFFER_ALIGNMENT`"
        );
        assert!(
            alignment.get().is_power_of_two(),
            "alignment must be a power of two, not {alignment}"
        );

        // At minimum, we must have alignment sufficient to map the buffer.
        let alignment = alignment.max(wgpu::BufferSize::new(wgpu::MAP_ALIGNMENT).unwrap());

        self.total_staged += size.get();
        self.provider
            .allocate(self.device, size, alignment, |staging_buffer_slice| {
                copy(self.command_encoder, staging_buffer_slice);
                staging_buffer_slice.get_mapped_range_mut()
            })
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
}

impl<'a, P> Drop for WriteStagingTransaction<'a, P>
where
    P: StagingBufferProvider,
{
    fn drop(&mut self) {
        self.provider.finish(self.command_encoder);
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

pub trait StagingBufferProvider {
    fn allocate<R>(
        &mut self,
        device: &wgpu::Device,
        size: wgpu::BufferSize,
        alignment: wgpu::BufferSize,
        f: impl FnOnce(wgpu::BufferSlice<'_>) -> R,
    ) -> R;
    fn finish(&mut self, command_encoder: &mut wgpu::CommandEncoder);
}

impl<T> StagingBufferProvider for &mut T
where
    T: StagingBufferProvider,
{
    fn allocate<R>(
        &mut self,
        device: &wgpu::Device,
        size: wgpu::BufferSize,
        alignment: wgpu::BufferSize,
        f: impl FnOnce(wgpu::BufferSlice<'_>) -> R,
    ) -> R {
        T::allocate(self, device, size, alignment, f)
    }

    fn finish(&mut self, command_encoder: &mut wgpu::CommandEncoder) {
        T::finish(*self, command_encoder);
    }
}

#[derive(Clone, Debug, Default)]
pub struct OneShotStaging {
    unmap_buffers: Vec<wgpu::Buffer>,
}

impl StagingBufferProvider for OneShotStaging {
    fn allocate<R>(
        &mut self,
        device: &wgpu::Device,
        size: wgpu::BufferSize,
        alignment: wgpu::BufferSize,
        f: impl FnOnce(wgpu::BufferSlice<'_>) -> R,
    ) -> R {
        let _ = alignment;
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("one-time write staging"),
            size: size.get(),
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: true,
        });

        let output = f(staging_buffer.slice(..));

        self.unmap_buffers.push(staging_buffer);

        output
    }

    fn finish(&mut self, command_encoder: &mut wgpu::CommandEncoder) {
        let _ = command_encoder;
        for buffer in self.unmap_buffers.drain(..) {
            buffer.unmap();
        }
    }
}

///////////////////////////////////////////////

#[derive(Clone, Debug)]
pub struct StagingPool {
    inner: Arc<ChunkPoolInner>,
}

#[derive(Debug)]
struct ChunkPoolInner {
    /// Minimum size of an individual chunk
    chunk_size: wgpu::BufferSize,
    chunk_label: Cow<'static, str>,
    state: RwLock<ChunkPoolState>,
}

#[derive(Debug, Default)]
struct ChunkPoolState {
    /// Chunks that are back from the GPU and ready to be mapped for write and
    /// put into `active_chunks`.
    free_chunks: Vec<Chunk>,
    in_flight_count: usize,
    total_allocated_count: usize,
    total_allocated_bytes: u64,
}

impl Default for StagingPool {
    fn default() -> Self {
        Self::new(wgpu::BufferSize::new(0x1000).unwrap(), "staging pool")
    }
}

impl StagingPool {
    pub fn new(chunk_size: wgpu::BufferSize, chunk_label: impl Into<Cow<'static, str>>) -> Self {
        Self {
            inner: Arc::new(ChunkPoolInner {
                chunk_size,
                chunk_label: chunk_label.into(),
                state: RwLock::new(Default::default()),
            }),
        }
    }

    pub fn start_write(&self) -> WriteStagingBelt {
        WriteStagingBelt::from_pool(self.clone())
    }

    pub fn info(&self) -> StagingPoolInfo {
        let state = self.inner.state.read();
        StagingPoolInfo {
            in_flight_count: state.in_flight_count,
            free_count: state.free_chunks.len(),
            total_allocation_count: state.total_allocated_count,
            total_allocation_bytes: state.total_allocated_bytes,
        }
    }
}

#[derive(Debug)]
pub struct WriteStagingBelt {
    pool: StagingPool,

    /// Chunks into which we are accumulating data to be transferred.
    active_chunks: Vec<Chunk>,
}

impl WriteStagingBelt {
    pub fn new(chunk_size: wgpu::BufferSize, chunk_label: impl Into<Cow<'static, str>>) -> Self {
        Self::from_pool(StagingPool::new(chunk_size, chunk_label))
    }

    pub fn from_pool(pool: StagingPool) -> Self {
        Self {
            pool,
            active_chunks: vec![],
        }
    }
}

impl StagingBufferProvider for WriteStagingBelt {
    fn allocate<R>(
        &mut self,
        device: &wgpu::Device,
        size: wgpu::BufferSize,
        alignment: wgpu::BufferSize,
        f: impl FnOnce(wgpu::BufferSlice<'_>) -> R,
    ) -> R {
        let chunk_index = self
            .active_chunks
            .iter()
            .position(|chunk| chunk.can_allocate(size, alignment.get()))
            .unwrap_or_else(|| {
                let mut state = self.pool.inner.state.write();
                state.in_flight_count += 1;

                let chunk = if let Some(index) = state
                    .free_chunks
                    .iter()
                    .position(|chunk| chunk.can_allocate(size, alignment.get()))
                {
                    state.free_chunks.swap_remove(index)
                }
                else {
                    let size = self.pool.inner.chunk_size.get().max(size.get());
                    state.total_allocated_count += 1;
                    state.total_allocated_bytes += size;
                    drop(state);

                    Chunk {
                        buffer: device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some(&self.pool.inner.chunk_label),
                            size,
                            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
                            mapped_at_creation: true,
                        }),
                        offset: 0,
                    }
                };

                let chunk_index = self.active_chunks.len();
                self.active_chunks.push(chunk);
                chunk_index
            });

        let chunk = &mut self.active_chunks[chunk_index];
        let allocation_offset = chunk.allocate(size, alignment.get());

        let staging_buffer_slice = chunk
            .buffer
            .slice(allocation_offset..allocation_offset + size.get());

        f(staging_buffer_slice)
    }

    fn finish(&mut self, command_encoder: &mut wgpu::CommandEncoder) {
        let active_chunks = std::mem::take(&mut self.active_chunks);

        for chunk in &active_chunks {
            chunk.buffer.unmap();
        }

        struct InflightChunks {
            active_chunks: Vec<Chunk>,
            pool: StagingPool,
        }

        impl InflightChunks {
            fn recall(&mut self) {
                for mut chunk in self.active_chunks.drain(..) {
                    let pool = self.pool.clone();

                    chunk
                        .buffer
                        .clone()
                        .slice(..)
                        .map_async(wgpu::MapMode::Write, move |_| {
                            chunk.offset = 0;

                            let mut state = pool.inner.state.write();
                            state.in_flight_count -= 1;
                            state.free_chunks.push(chunk);
                        });
                }
            }
        }

        impl Drop for InflightChunks {
            fn drop(&mut self) {
                // this is to make sure active buffers are recalled even if the command encoder
                // is dropped and never submitted
                self.recall();
            }
        }

        let mut inflight = InflightChunks {
            active_chunks,
            pool: self.pool.clone(),
        };

        command_encoder.on_submitted_work_done(move || {
            inflight.recall();
        });
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StagingPoolInfo {
    pub in_flight_count: usize,
    pub free_count: usize,
    pub total_allocation_count: usize,
    pub total_allocation_bytes: u64,
}

#[derive(Debug)]
struct Chunk {
    buffer: wgpu::Buffer,
    offset: wgpu::BufferAddress,
}

impl Chunk {
    fn can_allocate(&self, size: wgpu::BufferSize, alignment: wgpu::BufferAddress) -> bool {
        let alloc_start = wgpu::util::align_to(self.offset, alignment);
        let alloc_end = alloc_start + size.get();

        alloc_end <= self.buffer.size()
    }

    fn allocate(
        &mut self,
        size: wgpu::BufferSize,
        alignment: wgpu::BufferAddress,
    ) -> wgpu::BufferAddress {
        let alloc_start = wgpu::util::align_to(self.offset, alignment);
        let alloc_end = alloc_start + size.get();

        assert!(alloc_end <= self.buffer.size());
        self.offset = alloc_end;
        alloc_start
    }
}
