use std::{
    marker::PhantomData,
    ops::{
        Deref,
        DerefMut,
        Range,
        RangeBounds,
    },
    sync::Arc,
};

use bytemuck::Pod;
use nalgebra::Vector2;
use palette::Srgba;
use parking_lot::Mutex;
use wgpu::util::DeviceExt;

use crate::util::{
    ImageSizeExt,
    normalize_index_bounds,
};

pub fn unpadded_buffer_size<T>(num_elements: usize) -> u64 {
    (std::mem::size_of::<T>() * num_elements) as u64
}

pub const BUFFER_COPY_ALIGN_MASK: u64 = wgpu::COPY_BUFFER_ALIGNMENT - 1;

pub fn align_copy_start_offset(offset: u64) -> u64 {
    offset & !BUFFER_COPY_ALIGN_MASK
}

pub fn pad_buffer_size_for_copy(unpadded_size: u64) -> u64 {
    // https://github.com/gfx-rs/wgpu/blob/836c97056fb2c32852d1d8f6f45fefba1d1d6d26/wgpu/src/util/device.rs#L52
    // Valid vulkan usage is
    // 1. buffer size must be a multiple of COPY_BUFFER_ALIGNMENT.
    // 2. buffer size must be greater than 0.
    // Therefore we round the value up to the nearest multiple, and ensure it's at
    // least COPY_BUFFER_ALIGNMENT.
    ((unpadded_size + BUFFER_COPY_ALIGN_MASK) & !BUFFER_COPY_ALIGN_MASK)
        .max(wgpu::COPY_BUFFER_ALIGNMENT)
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

pub fn is_buffer_copy_aligned(index: u64) -> bool {
    (index & BUFFER_COPY_ALIGN_MASK) == 0
}

// note: this is intentionally not Clone
#[derive(Debug)]
pub struct TypedArrayBuffer<T> {
    inner: Option<TypedArrayBufferInner>,
    device: wgpu::Device,
    label: String,
    usage: wgpu::BufferUsages,
    _phantom: PhantomData<[T]>,
}

impl<T> TypedArrayBuffer<T> {
    fn new_impl(
        device: &wgpu::Device,
        label: &str,
        capacity: usize,
        num_elements: usize,
        usage: wgpu::BufferUsages,
        mapped_at_creation: bool,
    ) -> Self {
        // todo: do we want to store if its mapped (and if it's read/write)?

        let mut buffer = Self {
            inner: None,
            device: device.clone(),
            label: label.to_owned(),
            usage,
            _phantom: PhantomData,
        };

        buffer.allocate_inner(capacity, num_elements, mapped_at_creation);

        buffer
    }

    fn allocate_inner(
        &mut self,
        capacity: usize,
        num_elements: usize,
        mapped_at_creation: bool,
    ) -> Option<TypedArrayBufferInner> {
        let old_inner = self.inner.take();

        assert!(capacity >= num_elements);

        if capacity != 0 {
            self.inner = Some(TypedArrayBufferInner::new::<T>(
                &self.device,
                &self.label,
                capacity,
                num_elements,
                self.usage,
                mapped_at_creation,
            ));
        }

        old_inner
    }

    pub fn new(device: &wgpu::Device, label: &str, usage: wgpu::BufferUsages) -> Self {
        Self::with_capacity(device, label, usage, 0)
    }

    pub fn with_capacity(
        device: &wgpu::Device,
        label: &str,
        usage: wgpu::BufferUsages,
        capacity: usize,
    ) -> Self {
        Self::new_impl(device, label, capacity, 0, usage, false)
    }

    pub fn buffer(&self) -> Option<&wgpu::Buffer> {
        self.inner.as_ref().map(|inner| &inner.buffer)
    }

    pub fn len(&self) -> usize {
        self.inner.as_ref().map_or(0, |inner| inner.num_elements)
    }

    pub fn capacity(&self) -> usize {
        self.inner.as_ref().map_or(0, |inner| inner.capacity)
    }

    pub fn is_empty(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(|inner| inner.num_elements == 0)
    }

    pub fn usage(&self) -> wgpu::BufferUsages {
        self.usage
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn is_allocated(&self) -> bool {
        self.inner.is_some()
    }
}

impl<T> TypedArrayBuffer<T>
where
    T: Pod,
{
    pub fn from_slice(
        device: &wgpu::Device,
        label: &str,
        usage: wgpu::BufferUsages,
        data: &[T],
    ) -> Self {
        Self::from_fn_with_view(device, label, data.len(), usage, |view| {
            view.copy_from_slice(data);
        })
    }

    pub fn from_value(
        device: &wgpu::Device,
        label: &str,
        num_elements: usize,
        usage: wgpu::BufferUsages,
        value: T,
    ) -> Self {
        Self::from_fn_with_view(device, label, num_elements, usage, |view| {
            view.fill(value);
        })
    }

    pub fn from_fn(
        device: &wgpu::Device,
        label: &str,
        num_elements: usize,
        usage: wgpu::BufferUsages,
        mut fill: impl FnMut(usize) -> T,
    ) -> Self {
        Self::from_fn_with_view(device, label, num_elements, usage, |view| {
            view.iter_mut()
                .enumerate()
                .for_each(|(index, value)| *value = fill(index));
        })
    }

    pub fn from_fn_with_view(
        device: &wgpu::Device,
        label: &str,
        num_elements: usize,
        usage: wgpu::BufferUsages,
        mut fill: impl FnMut(&mut [T]),
    ) -> Self {
        let mut buffer = Self::new_impl(device, label, num_elements, num_elements, usage, true);

        if let Some(inner) = &mut buffer.inner {
            inner.with_mapped_mut(|view| fill(&mut view[..num_elements]))
        }
        else {
            fill(&mut []);
        }

        buffer
    }

    pub fn read_view<'a>(
        &'a self,
        range: impl RangeBounds<usize>,
        queue: &wgpu::Queue,
    ) -> TypedArrayBufferReadView<'a, T> {
        self.inner
            .as_ref()
            .and_then(|inner| {
                let index_range = normalize_index_bounds(range, inner.num_elements);
                (!index_range.is_empty())
                    .then(|| TypedArrayBufferReadView::new(index_range, inner, &self.device, queue))
            })
            .unwrap_or(TypedArrayBufferReadView {
                inner: None,
                _phantom: PhantomData,
            })
    }

    pub fn write_view<'a>(
        &'a mut self,
        range: impl RangeBounds<usize>,
        queue: &wgpu::Queue,
    ) -> TypedArrayBufferWriteView<'a, T> {
        self.inner
            .as_mut()
            .and_then(|inner| {
                let index_range = normalize_index_bounds(range, inner.num_elements);

                (!index_range.is_empty()).then(|| {
                    TypedArrayBufferWriteView::new(index_range, inner, &self.device, queue)
                })
            })
            .unwrap_or(TypedArrayBufferWriteView {
                inner: None,
                _phantom: PhantomData,
            })
    }

    /// Reallocates the buffer for a larger size.
    ///
    /// This only actually reallocates if the current capacity is less than
    /// `new_elements`.
    ///
    /// If a closure is passed as `on_reallocate`, it will be called with:
    ///
    /// 1. a mapped slice of the old data, if `pass_old_view` is `true` **and**
    ///    the buffer supports [`wgpu::BufferUsages::COPY_SRC`]
    /// 2. a mapped mut-slice of the new buffer. This is always present, as it's
    ///    cheap to map the buffer on creation.
    /// 3. the new [`wgpu::Buffer`]
    ///
    /// With this it's possible to copy data from the old buffer to the new one,
    /// if desired. The new underlying [`wgpu::Buffer`] can also be used to
    /// recreate any bind groups if necessary.
    ///
    /// This returns `true` if an reallocation did take place.
    pub fn reallocate_for_size<F>(
        &mut self,
        num_elements: usize,
        queue: &wgpu::Queue,
        mut on_reallocate: Option<F>,
        pass_old_view: bool,
    ) -> bool
    where
        F: FnMut(Option<&[T]>, &mut [T], &wgpu::Buffer),
    {
        let current_capacity = self.capacity();

        if num_elements > current_capacity {
            // todo: make this a generic parameter?
            let new_capacity = (current_capacity * 2).max(num_elements);

            let old_inner =
                self.allocate_inner(new_capacity, num_elements, on_reallocate.is_some());

            if let Some(on_reallocate) = &mut on_reallocate {
                let can_read = self.usage.contains(wgpu::BufferUsages::COPY_SRC);

                let old_view = (pass_old_view && can_read)
                    .then(|| {
                        old_inner.as_ref().map(|inner| {
                            TypedArrayBufferReadView::new(
                                0..inner.num_elements,
                                inner,
                                &self.device,
                                queue,
                            )
                        })
                    })
                    .flatten();

                let new_inner = self
                    .inner
                    .as_ref()
                    .expect("we just reallocated with larger capacity");

                // note: this unmaps the buffer
                new_inner.with_mapped_mut(|new_view| {
                    on_reallocate(
                        old_view.as_deref(),
                        &mut new_view[..num_elements],
                        &new_inner.buffer,
                    );
                });
            }

            true
        }
        else {
            false
        }
    }

    pub fn write_all(
        &mut self,
        queue: &wgpu::Queue,
        data: &[T],
        mut on_reallocate: impl FnMut(&wgpu::Buffer),
    ) -> bool {
        let did_reallocate = self.reallocate_for_size(
            data.len(),
            queue,
            Some(
                |_old_view: Option<&[T]>, new_view: &mut [T], new_buffer: &wgpu::Buffer| {
                    new_view.copy_from_slice(data);
                    on_reallocate(new_buffer);
                },
            ),
            false,
        );

        if !did_reallocate {
            // still need to write the data
            let mut view = self.write_view(..data.len(), queue);
            view.copy_from_slice(data);
        }

        did_reallocate
    }
}

#[derive(Debug)]
struct TypedArrayBufferInner {
    buffer: wgpu::Buffer,
    num_elements: usize,
    capacity: usize,
    unpadded_buffer_size: u64,
    padded_buffer_size: u64,
}

impl TypedArrayBufferInner {
    fn new<T>(
        device: &wgpu::Device,
        label: &str,
        capacity: usize,
        num_elements: usize,
        usage: wgpu::BufferUsages,
        mapped_at_creation: bool,
    ) -> Self {
        assert!(capacity > 0);

        let unpadded_buffer_size = unpadded_buffer_size::<T>(capacity);
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
            capacity,
            unpadded_buffer_size,
            padded_buffer_size,
        }
    }

    /// This is for when you mapped the buffer at creation
    fn with_mapped_mut<T, R>(&self, mut f: impl FnMut(&mut [T]) -> R) -> R
    where
        T: Pod,
    {
        let mut view = self.buffer.get_mapped_range_mut(..);
        let view_slice: &mut [T] =
            bytemuck::cast_slice_mut(&mut view[..(self.unpadded_buffer_size as usize)]);
        let output = f(view_slice);
        drop(view);
        self.buffer.unmap();
        output
    }
}

// note: don't make this Clone. While it would be nice to have, the Drop impl
// then needs to take into account if there are more outstanding mapped view,
// e.g. by adding a reference count. At this point the user can just Arc the
// whole view.
#[derive(Debug)]
pub struct TypedArrayBufferReadView<'a, T> {
    inner: Option<TypedBufferReadViewInner>,
    _phantom: PhantomData<&'a [T]>,
}

impl<'a, T> TypedArrayBufferReadView<'a, T> {
    fn new(
        index_range: Range<usize>,
        inner: &'a TypedArrayBufferInner,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Self {
        let alignment =
            StagingBufferAlignment::from_unaligned_buffer_range_typed::<T>(index_range.clone());

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read: staging"),
            size: alignment.copy_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut command_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("read: copy to staging"),
        });

        command_encoder.copy_buffer_to_buffer(
            &inner.buffer,
            alignment.buffer_start,
            &staging_buffer,
            0,
            alignment.copy_size,
        );

        let result_buf = AsyncResultBuf::default();

        command_encoder.map_buffer_on_submit(
            &staging_buffer,
            wgpu::MapMode::Read,
            ..,
            result_buf.callback(),
        );

        let submission_index = queue.submit([command_encoder.finish()]);

        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: None,
            })
            .expect("device poll failed");

        result_buf.unwrap().unwrap();

        let staging_view = staging_buffer.get_mapped_range(..);

        Self {
            inner: Some(TypedBufferReadViewInner {
                alignment,
                staging_buffer,
                staging_view: Arc::new(staging_view),
            }),
            _phantom: PhantomData,
        }
    }
}

impl<'a, T> AsRef<[T]> for TypedArrayBufferReadView<'a, T>
where
    T: Pod,
{
    fn as_ref(&self) -> &[T] {
        self.inner
            .as_ref()
            .map(|inner| bytemuck::cast_slice(&inner.staging_view[inner.alignment.staging_range()]))
            .unwrap_or(&[])
    }
}

impl<'a, T> Deref for TypedArrayBufferReadView<'a, T>
where
    T: Pod,
{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<'a, T> Drop for TypedArrayBufferReadView<'a, T> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            drop(inner.staging_view);
            inner.staging_buffer.unmap();
        }
    }
}

#[derive(Debug)]
struct TypedBufferReadViewInner {
    alignment: StagingBufferAlignment,
    staging_buffer: wgpu::Buffer,
    staging_view: Arc<wgpu::BufferView>,
}

#[derive(Debug)]
pub struct TypedArrayBufferWriteView<'a, T> {
    inner: Option<TypedArrayBufferWriteViewInner<'a>>,
    _phantom: PhantomData<&'a mut [T]>,
}

impl<'a, T> TypedArrayBufferWriteView<'a, T> {
    fn new(
        index_range: Range<usize>,
        inner: &'a TypedArrayBufferInner,
        device: &'a wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Self {
        let alignment =
            StagingBufferAlignment::from_unaligned_buffer_range_typed::<T>(index_range.clone());

        // this is just nasty to fix and we could make it a hard requirement anyway.
        #[allow(clippy::todo)]
        if !alignment.is_aligned() {
            todo!("unaligned write");
        }

        // note: we could use `wgpu::Queue::write_buffer_with`, but prefer something we
        // can customize better later.
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("write: staging"),
            size: alignment.copy_size,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: true,
        });

        let staging_view = staging_buffer.get_mapped_range_mut(..);

        Self {
            inner: Some(TypedArrayBufferWriteViewInner {
                alignment,
                staging_buffer,
                staging_view,
                destination_buffer: &inner.buffer,
                device,
                queue: queue.clone(),
            }),
            _phantom: PhantomData,
        }
    }

    pub fn finish_with(mut self, command_encoder: &mut wgpu::CommandEncoder) {
        if let Some(inner) = self.inner.take() {
            inner.dispatch_copy_with(command_encoder);
        }
    }

    pub fn finish(mut self) {
        // note: should we remove this? This is done on drop anyway
        if let Some(inner) = self.inner.take() {
            inner.dispatch_copy();
        }
    }
}

impl<'a, T> AsRef<[T]> for TypedArrayBufferWriteView<'a, T>
where
    T: Pod,
{
    fn as_ref(&self) -> &[T] {
        self.inner
            .as_ref()
            .map(|inner| bytemuck::cast_slice(&inner.staging_view[inner.alignment.staging_range()]))
            .unwrap_or(&[])
    }
}

impl<'a, T> AsMut<[T]> for TypedArrayBufferWriteView<'a, T>
where
    T: Pod,
{
    fn as_mut(&mut self) -> &mut [T] {
        self.inner
            .as_mut()
            .map(|inner| {
                bytemuck::cast_slice_mut(&mut inner.staging_view[inner.alignment.staging_range()])
            })
            .unwrap_or(&mut [])
    }
}

impl<'a, T> Deref for TypedArrayBufferWriteView<'a, T>
where
    T: Pod,
{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<'a, T> DerefMut for TypedArrayBufferWriteView<'a, T>
where
    T: Pod,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

impl<'a, T> Drop for TypedArrayBufferWriteView<'a, T> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.dispatch_copy();
        }
    }
}

#[derive(Debug)]
struct TypedArrayBufferWriteViewInner<'a> {
    alignment: StagingBufferAlignment,
    staging_buffer: wgpu::Buffer,
    staging_view: wgpu::BufferViewMut,
    destination_buffer: &'a wgpu::Buffer,
    device: &'a wgpu::Device,
    queue: wgpu::Queue,
}

impl<'a> TypedArrayBufferWriteViewInner<'a> {
    fn dispatch_copy(self) {
        let mut command_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("write: copy from staging"),
                });

        let queue = self.dispatch_copy_with(&mut command_encoder);

        queue.submit([command_encoder.finish()]);
    }

    fn dispatch_copy_with(self, command_encoder: &mut wgpu::CommandEncoder) -> wgpu::Queue {
        drop(self.staging_view);
        self.staging_buffer.unmap();

        command_encoder.copy_buffer_to_buffer(
            &self.staging_buffer,
            self.alignment.buffer_start,
            self.destination_buffer,
            self.alignment.staging_start,
            self.alignment.copy_size,
        );

        self.queue
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StagingBufferAlignment {
    pub buffer_start: u64,
    pub buffer_end: u64,
    pub staging_start: u64,
    pub staging_end: u64,
    pub copy_size: u64,
}

impl StagingBufferAlignment {
    pub fn from_unaligned_buffer_range_typed<T>(index_range: Range<usize>) -> Self {
        let unaligned_copy_source = Range {
            start: (std::mem::size_of::<T>() * index_range.start) as u64,
            end: (std::mem::size_of::<T>() * index_range.end) as u64,
        };
        Self::from_unaligned_buffer_range(unaligned_copy_source)
    }

    pub fn from_unaligned_buffer_range(unaligned_buffer_range: Range<u64>) -> Self {
        let unaligned_copy_size = unaligned_buffer_range.end - unaligned_buffer_range.start;

        let buffer_start = align_copy_start_offset(unaligned_buffer_range.start);
        let copy_size = pad_buffer_size_for_copy(unaligned_copy_size);
        let buffer_end = buffer_start + copy_size;

        let staging_start = unaligned_buffer_range.start - buffer_start;
        let staging_end = unaligned_buffer_range.end - buffer_start;

        Self {
            buffer_start,
            buffer_end,
            staging_start,
            staging_end,
            copy_size,
        }
    }

    pub fn staging_range(&self) -> Range<usize> {
        (self.staging_start as usize)..(self.staging_end as usize)
    }

    pub fn is_aligned(&self) -> bool {
        self.staging_start == 0 && self.staging_end == self.copy_size
    }
}

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

#[derive(Debug)]
pub struct StagedTypedArrayBuffer<T> {
    pub buffer: TypedArrayBuffer<T>,
    pub staging: Vec<T>,
}

impl<T> StagedTypedArrayBuffer<T> {
    pub fn new(device: &wgpu::Device, label: &str, usage: wgpu::BufferUsages) -> Self {
        Self::with_capacity(device, label, usage, 0)
    }

    pub fn with_capacity(
        device: &wgpu::Device,
        label: &str,
        usage: wgpu::BufferUsages,
        initial_capacity: usize,
    ) -> Self {
        let buffer = TypedArrayBuffer::with_capacity(
            device,
            label,
            usage | wgpu::BufferUsages::COPY_DST,
            initial_capacity,
        );
        Self::from_buffer(buffer)
    }

    pub fn from_buffer(buffer: TypedArrayBuffer<T>) -> Self {
        assert!(
            buffer.usage.contains(wgpu::BufferUsages::COPY_DST),
            "Buffer must contain BufferUsages::COPY_DST to be used as a staged buffer."
        );
        Self {
            buffer,
            staging: vec![],
        }
    }

    pub fn push(&mut self, item: T) {
        self.staging.push(item);
    }
}

impl<T> StagedTypedArrayBuffer<T>
where
    T: Pod,
{
    pub fn from_data(
        device: &wgpu::Device,
        label: &str,
        usage: wgpu::BufferUsages,
        data: Vec<T>,
    ) -> Self {
        let buffer = TypedArrayBuffer::from_slice(
            device,
            label,
            usage | wgpu::BufferUsages::COPY_DST,
            &data,
        );
        Self {
            buffer,
            staging: data,
        }
    }

    pub fn flush(&mut self, queue: &wgpu::Queue, on_reallocate: impl FnMut(&wgpu::Buffer)) -> bool {
        if self.staging.is_empty() {
            // the below code works fine for an empty instance buffer, and it'll basically
            // do nothing, but we can still exit early.
            return false;
        }

        let reallocated = self.buffer.write_all(queue, &self.staging, on_reallocate);

        self.staging.clear();

        reallocated
    }
}

pub trait WriteImageToTextureExt {
    fn write_to_texture(&self, queue: &wgpu::Queue, texture: &wgpu::Texture);
}

impl<Container> WriteImageToTextureExt for image::ImageBuffer<image::Rgba<u8>, Container>
where
    Container: Deref<Target = [u8]>,
{
    fn write_to_texture(&self, queue: &wgpu::Queue, texture: &wgpu::Texture) {
        // todo: see https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#performance-considerations-2
        //
        // note: the 256 bytes per row alignment doesn't apply for Queue::write_texture:
        // https://docs.rs/wgpu/latest/wgpu/constant.COPY_BYTES_PER_ROW_ALIGNMENT.html

        let size = Vector2::new(texture.width(), texture.height());

        let bytes_per_row = size.x * 4;
        if bytes_per_row < wgpu::COPY_BYTES_PER_ROW_ALIGNMENT {
            // https://docs.rs/wgpu/latest/wgpu/struct.TexelCopyBufferLayout.html#structfield.bytes_per_row
            //
            todo!("image needs padding")
        }

        assert_eq!(
            self.size(),
            size,
            "provided image size doesn't match texture"
        );

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            self.as_raw(),
            wgpu::TexelCopyBufferLayout {
                bytes_per_row: Some(bytes_per_row),
                ..Default::default()
            },
            wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
        );
    }
}

pub fn texture_view_from_color(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color: Srgba<u8>,
    label: &str,
) -> wgpu::TextureView {
    let color: [u8; 4] = color.into();
    let texture = device.create_texture_with_data(
        queue,
        &texture_descriptor(&Vector2::repeat(1), label),
        Default::default(),
        &color,
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(label),
        ..Default::default()
    })
}

pub fn texture_from_image<Container>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &image::ImageBuffer<image::Rgba<u8>, Container>,
    label: &str,
) -> wgpu::Texture
where
    Container: Deref<Target = [u8]>,
{
    let size = image.size();
    device.create_texture_with_data(
        queue,
        &texture_descriptor(&size, label),
        Default::default(),
        image.as_raw(),
    )
}

pub fn texture_descriptor<'a>(size: &Vector2<u32>, label: &'a str) -> wgpu::TextureDescriptor<'a> {
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
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }
}
