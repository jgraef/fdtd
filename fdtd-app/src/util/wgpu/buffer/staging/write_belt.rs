use std::{
    borrow::Cow,
    sync::Arc,
};

use parking_lot::Mutex;

#[derive(Debug)]
pub struct WriteStagingBelt {
    /// Minimum size of an individual chunk
    chunk_size: wgpu::BufferSize,

    chunk_label: Cow<'static, str>,

    /// Chunks into which we are accumulating data to be transferred.
    active_chunks: Vec<Chunk>,

    /// Chunks that have scheduled transfers already; they are unmapped and some
    /// command encoder has one or more commands with them as source.
    closed_chunks: Vec<Chunk>,

    /// Chunks that are back from the GPU and ready to be mapped for write and
    /// put into `active_chunks`.
    free_chunks: Arc<Mutex<Vec<Chunk>>>,
}

impl WriteStagingBelt {
    pub fn new(chunk_size: wgpu::BufferSize, chunk_label: impl Into<Cow<'static, str>>) -> Self {
        Self {
            chunk_size,
            chunk_label: chunk_label.into(),
            active_chunks: vec![],
            closed_chunks: vec![],
            free_chunks: Arc::new(Mutex::new(vec![])),
        }
    }

    /// Allocate a staging belt slice of `size` to be copied into the
    /// `destination` buffer at the specified offset.
    ///
    /// `offset` and `size` must be multiples of [`COPY_BUFFER_ALIGNMENT`]
    /// (as is required by the underlying buffer operations).
    ///
    /// The upload will be placed into the provided command encoder. This
    /// encoder must be submitted after [`StagingBelt::finish()`] is called
    /// and before [`StagingBelt::recall()`] is called.
    ///
    /// If the `size` is greater than the size of any free internal buffer, a
    /// new buffer will be allocated for it. Therefore, the `chunk_size`
    /// passed to [`StagingBelt::new()`] should ideally be larger than every
    /// such size.
    #[track_caller]
    pub fn write_buffer(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Buffer,
        offset: wgpu::BufferAddress,
        size: wgpu::BufferSize,
    ) -> wgpu::BufferViewMut {
        // Asserting this explicitly gives a usefully more specific, and more prompt,
        // error than leaving it to regular API validation.
        // We check only `offset`, not `size`, because `self.allocate()` will check the
        // size.
        assert!(
            offset.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            "WriteStagingBelt::write_buffer() offset {offset} must be a multiple of `COPY_BUFFER_ALIGNMENT`"
        );

        let slice_of_belt = self.allocate(
            device,
            size,
            const { wgpu::BufferSize::new(wgpu::COPY_BUFFER_ALIGNMENT).unwrap() },
        );
        encoder.copy_buffer_to_buffer(
            slice_of_belt.buffer(),
            slice_of_belt.offset(),
            destination,
            offset,
            size.get(),
        );
        slice_of_belt.get_mapped_range_mut()
    }

    /// Allocate a staging belt slice with the given `size` and `alignment` and
    /// return it.
    ///
    /// `size` must be a multiple of [`COPY_BUFFER_ALIGNMENT`]
    /// (as is required by the underlying buffer operations).
    ///
    /// To use this slice, call [`BufferSlice::get_mapped_range_mut()`] and
    /// write your data into that [`BufferViewMut`].
    /// (The view must be dropped before [`StagingBelt::finish()`] is called.)
    ///
    /// You can then record your own GPU commands to perform with the slice,
    /// such as copying it to a texture (whereas
    /// [`StagingBelt::write_buffer()`] can only write to other buffers).
    /// All commands involving this slice must be submitted after
    /// [`StagingBelt::finish()`] is called and before [`StagingBelt::recall()`]
    /// is called.
    ///
    /// If the `size` is greater than the space available in any free internal
    /// buffer, a new buffer will be allocated for it. Therefore, the
    /// `chunk_size` passed to [`StagingBelt::new()`] should ideally be
    /// larger than every such size.
    ///
    /// The chosen slice will be positioned within the buffer at a multiple of
    /// `alignment`, which may be used to meet alignment requirements for
    /// the operation you wish to perform with the slice. This does not
    /// necessarily affect the alignment of the [`BufferViewMut`].
    #[track_caller]
    pub fn allocate(
        &mut self,
        device: &wgpu::Device,
        size: wgpu::BufferSize,
        alignment: wgpu::BufferSize,
    ) -> wgpu::BufferSlice<'_> {
        assert!(
            size.get().is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            "WriteStagingBelt allocation size {size} must be a multiple of `COPY_BUFFER_ALIGNMENT`"
        );
        assert!(
            alignment.get().is_power_of_two(),
            "alignment must be a power of two, not {alignment}"
        );
        // At minimum, we must have alignment sufficient to map the buffer.
        let alignment = alignment.get().max(wgpu::MAP_ALIGNMENT);

        let chunk_index = self
            .active_chunks
            .iter()
            .position(|chunk| chunk.can_allocate(size, alignment))
            .unwrap_or_else(|| {
                let mut free_chunks = self.free_chunks.lock();

                let chunk = if let Some(index) = free_chunks
                    .iter()
                    .position(|chunk| chunk.can_allocate(size, alignment))
                {
                    free_chunks.swap_remove(index)
                }
                else {
                    Chunk {
                        buffer: device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some(&self.chunk_label),
                            size: self.chunk_size.get().max(size.get()),
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
        let allocation_offset = chunk.allocate(size, alignment);

        chunk
            .buffer
            .slice(allocation_offset..allocation_offset + size.get())
    }

    /// Prepare currently mapped buffers for use in a submission.
    ///
    /// This must be called before the command encoder(s) provided to
    /// [`StagingBelt::write_buffer()`] are submitted.
    ///
    /// At this point, all the partially used staging buffers are closed (cannot
    /// be used for further writes) until after [`StagingBelt::recall()`] is
    /// called *and* the GPU is done copying the data from them.
    pub fn finish(&mut self) {
        for chunk in self.active_chunks.drain(..) {
            chunk.buffer.unmap();
            self.closed_chunks.push(chunk);
        }
    }

    /// Recall all of the closed buffers back to be reused.
    ///
    /// This must only be called after the command encoder(s) provided to
    /// [`StagingBelt::write_buffer()`] are submitted. Additional calls are
    /// harmless. Not calling this as soon as possible may result in
    /// increased buffer memory usage.
    pub fn recall(&mut self) {
        for mut chunk in self.closed_chunks.drain(..) {
            let free_chunks = self.free_chunks.clone();

            chunk
                .buffer
                .clone()
                .slice(..)
                .map_async(wgpu::MapMode::Write, move |_| {
                    let mut free_chunks = free_chunks.lock();
                    chunk.offset = 0;
                    free_chunks.push(chunk);
                });
        }
    }

    pub fn active_chunk_sizes(&self) -> impl Iterator<Item = wgpu::BufferAddress> {
        self.active_chunks.iter().map(|chunk| chunk.offset)
    }
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
