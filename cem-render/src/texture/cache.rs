use std::{
    collections::HashMap,
    path::{
        Path,
        PathBuf,
    },
    sync::{
        Arc,
        Weak,
    },
};

use bevy_ecs::resource::Resource;

use crate::texture::TextureAndView;

#[derive(Debug, Default, Resource)]
pub struct TextureCache {
    cache: HashMap<PathBuf, (Weak<TextureAndView>, ImageInfo)>,
}

impl TextureCache {
    pub fn get_or_insert<E, L>(
        &mut self,
        path: &Path,
        load: L,
    ) -> Result<(Arc<TextureAndView>, ImageInfo), E>
    where
        L: FnOnce() -> Result<(TextureAndView, ImageInfo), E>,
    {
        if let Some((weak, info)) = self.cache.get_mut(path) {
            if let Some(texture_and_view) = weak.upgrade() {
                Ok((texture_and_view, *info))
            }
            else {
                let (texture_and_view, info) = load()?;
                let texture_and_view = Arc::new(texture_and_view);
                *weak = Arc::downgrade(&texture_and_view);
                Ok((texture_and_view, info))
            }
        }
        else {
            let (texture_and_view, info) = load()?;
            let texture_and_view = Arc::new(texture_and_view);
            self.cache
                .insert(path.to_owned(), (Arc::downgrade(&texture_and_view), info));
            Ok((texture_and_view, info))
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImageInfo {
    pub original_color_type: image::ColorType,
}
