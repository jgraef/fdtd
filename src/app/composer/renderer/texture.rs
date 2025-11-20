use std::{
    convert::Infallible,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use image::{
    ImageReader,
    RgbaImage,
};
use nalgebra::Vector2;
use parking_lot::Mutex;
use wgpu::util::DeviceExt;

use crate::{
    Error,
    app::solver::util::WriteImage,
};

#[derive(Clone, Debug)]
pub struct Texture {
    pub texture: wgpu::Texture,
    pub texture_view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
    pub size: Vector2<u32>,
}

impl Texture {
    pub fn new(
        device: &wgpu::Device,
        sampler: &wgpu::Sampler,
        bind_group_layout: &wgpu::BindGroupLayout,
        size: Vector2<u32>,
    ) -> Self {
        let texture = device.create_texture(&create_texture_descriptor(size));
        Self::new_with_texture(device, sampler, bind_group_layout, size, texture)
    }

    pub fn new_with_data(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sampler: &wgpu::Sampler,
        bind_group_layout: &wgpu::BindGroupLayout,
        image: &RgbaImage,
    ) -> Self {
        let size = Vector2::new(image.width(), image.height());
        let texture = device.create_texture_with_data(
            queue,
            &create_texture_descriptor(size),
            Default::default(),
            image.as_raw(),
        );
        Self::new_with_texture(device, sampler, bind_group_layout, size, texture)
    }

    fn new_with_texture(
        device: &wgpu::Device,
        sampler: &wgpu::Sampler,
        bind_group_layout: &wgpu::BindGroupLayout,
        size: Vector2<u32>,
        texture: wgpu::Texture,
    ) -> Self {
        tracing::debug!(?size, "creating texture");

        let texture_view = texture.create_view(&Default::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture bind group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });

        Self {
            texture,
            texture_view,
            sampler: sampler.clone(),
            bind_group,
            size,
        }
    }

    pub fn write_image(&self, image: &RgbaImage, queue: &wgpu::Queue) {
        // todo: see https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#performance-considerations-2

        assert_eq!(
            image_size(image),
            self.size,
            "provided image size doesn't match texture"
        );

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            image.as_raw(),
            wgpu::TexelCopyBufferLayout {
                bytes_per_row: Some(4 * self.size.x),
                ..Default::default()
            },
            wgpu::Extent3d {
                width: self.size.x,
                height: self.size.y,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn create_texture_descriptor(size: Vector2<u32>) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("texture output"),
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

/// # TODO
///
/// - we probably only want to send stuff to the GPU before we render it. I.e.
///   buffer the RgbaImage and write texture when needed (e.g. before rendering
///   scene views). furthermore we could then not generate any RgbaImages when
///   they're not needed.
/// - instead of generating the image in the CPU, we could send the field values
///   to the GPU and do the color gradient in the fragment shader.
#[derive(Clone, Debug)]
pub struct TextureWriter {
    size: Vector2<u32>,
    shared: Arc<Mutex<TextureWriterShared>>,
}

#[derive(Debug)]
struct TextureWriterShared {
    image: RgbaImage,
    dirty: bool,
}

impl TextureWriter {
    pub fn new(size: Vector2<u32>) -> Self {
        Self {
            size,
            shared: Arc::new(Mutex::new(TextureWriterShared {
                image: RgbaImage::new(size.x, size.y),
                dirty: false,
            })),
        }
    }
}

impl WriteImage for TextureWriter {
    type Error = Infallible;

    fn size(&self) -> Vector2<u32> {
        self.size
    }

    fn write_image(&mut self, mut f: impl FnMut(&mut RgbaImage)) -> Result<(), Self::Error> {
        let mut shared = self.shared.lock();
        f(&mut shared.image);
        shared.dirty = true;
        Ok(())
    }
}

fn sync_texture_writers_with_textures(world: &mut hecs::World, queue: &wgpu::Queue) {
    for (_entity, (texture_writer, texture)) in world.query_mut::<(&TextureWriter, &Texture)>() {
        let mut shared = texture_writer.shared.lock();
        if shared.dirty {
            texture.write_image(&shared.image, queue);
            shared.dirty = false;
        }
    }
}

fn create_textures_for_texture_writers(
    world: &mut hecs::World,
    command_buffer: &mut hecs::CommandBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sampler: &wgpu::Sampler,
    texture_bind_group_layout: &wgpu::BindGroupLayout,
) {
    for (entity, texture_writer) in world.query_mut::<&TextureWriter>().without::<&Texture>() {
        let mut shared = texture_writer.shared.lock();

        let texture = if shared.dirty {
            shared.dirty = false;
            Texture::new_with_data(
                device,
                queue,
                sampler,
                texture_bind_group_layout,
                &shared.image,
            )
        }
        else {
            Texture::new(
                device,
                sampler,
                texture_bind_group_layout,
                texture_writer.size,
            )
        };

        command_buffer.insert_one(entity, texture);
    }
}

fn load_textures_from_files(
    world: &mut hecs::World,
    command_buffer: &mut hecs::CommandBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sampler: &wgpu::Sampler,
    texture_bind_group_layout: &wgpu::BindGroupLayout,
) {
    for (entity, load_texture) in world.query_mut::<&LoadTexture>() {
        tracing::debug!(path = %load_texture.path.display(), "loading texture from file");

        let texture = match load_image(&load_texture.path) {
            Ok(image) => {
                Texture::new_with_data(device, queue, sampler, texture_bind_group_layout, &image)
            }
            Err(error) => {
                tracing::debug!("failed to load image: {error}");
                continue;
            }
        };

        command_buffer.insert_one(entity, texture);
        command_buffer.remove_one::<LoadTexture>(entity);
    }
}

pub(super) fn update_textures(
    world: &mut hecs::World,
    command_buffer: &mut hecs::CommandBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sampler: &wgpu::Sampler,
    texture_bind_group_layout: &wgpu::BindGroupLayout,
) {
    load_textures_from_files(
        world,
        command_buffer,
        device,
        queue,
        sampler,
        texture_bind_group_layout,
    );

    create_textures_for_texture_writers(
        world,
        command_buffer,
        device,
        queue,
        sampler,
        texture_bind_group_layout,
    );

    sync_texture_writers_with_textures(world, queue);
}

fn image_size(image: &RgbaImage) -> Vector2<u32> {
    Vector2::new(image.width(), image.height())
}

#[derive(Clone, Debug)]
pub struct LoadTexture {
    pub path: PathBuf,
}

pub(super) fn load_image(path: impl AsRef<Path>) -> Result<RgbaImage, Error> {
    let image = ImageReader::open(path)?.decode()?;
    Ok(image.to_rgba8())
}
