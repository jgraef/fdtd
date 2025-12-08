use std::{
    collections::{
        HashMap,
        hash_map,
    },
    fs::File,
    hash::Hasher,
    io::{
        BufReader,
        BufWriter,
    },
    num::NonZero,
    path::{
        Path,
        PathBuf,
    },
};

use cem_util::{
    image::{
        ImageLoadExt,
        ImageSizeExt,
    },
    wgpu::image::{
        ImageTextureExt,
        MipLevels,
    },
};
use image::{
    ImageFormat,
    RgbaImage,
    imageops::FilterType,
};
use nalgebra::Vector2;
use seahash::SeaHasher;
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ImageHash(u64);

impl ImageHash {
    pub fn from_image(image: &RgbaImage) -> Self {
        let mut hasher = SeaHasher::new();

        // just a prefix so we could differentiate by image encoding or whatever.
        hasher.write_u32(0x00000001);

        hasher.write(image.as_raw());
        Self(hasher.finish())
    }
}

#[derive(Debug)]
pub struct MipMapCache {
    base_path: PathBuf,
    index_path: PathBuf,
    index: HashMap<ImageHash, IndexEntry>,
    filter: FilterType,
    dirty: bool,
}

impl MipMapCache {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let path = path.as_ref();
        tracing::debug!(path = %path.display(), "opening mip-map cache");

        std::fs::create_dir_all(path)?;

        let index_path = path.join("index.json");
        let index = if index_path.exists() {
            serde_json::from_reader(BufReader::new(File::open(&index_path)?))?
        }
        else {
            HashMap::new()
        };

        Ok(Self {
            base_path: path.to_owned(),
            index_path: path.join("index.json"),
            index,
            filter: FilterType::CatmullRom,
            dirty: false,
        })
    }

    pub fn flush(&mut self) -> Result<(), std::io::Error> {
        if self.dirty {
            serde_json::to_writer_pretty(
                BufWriter::new(File::create(&self.index_path)?),
                &self.index,
            )?;
            self.dirty = false;
        }

        Ok(())
    }

    pub fn create_texture<T>(
        &mut self,
        base_image: &RgbaImage,
        create_texture: impl FnOnce(NonZero<u32>, Vector2<u32>) -> T,
        mut insert_level: impl FnMut(&mut T, u32, Vector2<u32>, &RgbaImage),
    ) -> Result<T, image::ImageError> {
        let image_hash = ImageHash::from_image(base_image);
        let path_for_level = |level| {
            self.base_path
                .join(format!("{:016x}_{:02x}.png", image_hash.0, level))
        };
        let base_size = base_image.size();

        let texture = match self.index.entry(image_hash) {
            hash_map::Entry::Occupied(occupied_entry) => {
                let entry = occupied_entry.get();
                let mut size = base_size;

                let mut texture = create_texture(entry.mip_level_count, base_size);
                insert_level(&mut texture, 0, base_size, base_image);

                for i in 1..entry.mip_level_count.get() {
                    size = size.map(|c| 1.max(c / 2));
                    let image = RgbaImage::from_path(path_for_level(i))?;
                    insert_level(&mut texture, i, size, &image);
                }

                texture
            }
            hash_map::Entry::Vacant(vacant_entry) => {
                let mip_levels = MipLevels::Auto {
                    filter: self.filter,
                };
                let (mip_level_count, mip_levels) = mip_levels.get(base_size);

                let mut texture = create_texture(mip_level_count, base_size);

                base_image.generate_mip_levels(mip_levels, |mip_level, mip_size, image| {
                    insert_level(&mut texture, mip_level, mip_size, image);
                    if mip_level > 0 {
                        image.save_with_format(path_for_level(mip_level), ImageFormat::Png)?;
                    }

                    Ok::<(), image::ImageError>(())
                })?;

                vacant_entry.insert(IndexEntry {
                    base_size,
                    mip_level_count,
                });

                self.dirty = true;

                texture
            }
        };

        self.flush()?;

        Ok(texture)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct IndexEntry {
    base_size: Vector2<u32>,
    mip_level_count: NonZero<u32>,
}
