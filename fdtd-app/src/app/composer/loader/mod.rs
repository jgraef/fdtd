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

use crate::{
    Error,
    app::composer::{
        renderer::{
            material::{
                LoadAlbedoTexture,
                LoadMaterialTexture,
                TextureAndView,
            },
            mesh::LoadMesh,
            resource::RenderResourceCreator,
        },
        scene::{
            Changed,
            Scene,
        },
    },
};

#[derive(Debug)]
pub struct AssetLoader {
    render_resource_creator: RenderResourceCreator,

    /// Texture cache
    ///
    /// Caches textures loaded from files.
    texture_cache: TextureCache,
}

impl AssetLoader {
    pub fn new(render_resource_creator: &RenderResourceCreator) -> Self {
        Self {
            render_resource_creator: render_resource_creator.clone(),
            texture_cache: Default::default(),
        }
    }

    pub fn run_all(&mut self, scene: &mut Scene) -> Result<(), Error> {
        let mut run_loaders = RunLoaders {
            scene,
            context: LoaderContext {
                render_resource_creator: &mut self.render_resource_creator,
                texture_cache: &mut self.texture_cache,
            },
            result: Ok(()),
        };

        run_loaders.run::<LoadAlbedoTexture>();
        run_loaders.run::<LoadMaterialTexture>();
        run_loaders.run::<LoadMesh>();

        if let Err(error) = &run_loaders.result {
            tracing::warn!(?error);
        }

        run_loaders.result
    }
}

pub trait LoadAsset: hecs::Component {
    type State: LoadingState;

    fn start_loading(&self, context: &mut LoaderContext) -> Result<Self::State, Error>;
}

// note: this is just a future -.-
pub trait LoadingState: hecs::Component {
    type Output: hecs::DynamicBundle;

    fn poll(&mut self, context: &mut LoaderContext)
    -> Result<LoadingProgress<Self::Output>, Error>;
}

pub enum LoadingProgress<T> {
    Pending,
    Ready(T),
}

impl<T> From<Option<T>> for LoadingProgress<T> {
    fn from(value: Option<T>) -> Self {
        value.map_or(LoadingProgress::Pending, LoadingProgress::Ready)
    }
}

#[derive(Debug)]
pub struct LoaderContext<'a> {
    pub render_resource_creator: &'a mut RenderResourceCreator,
    texture_cache: &'a mut TextureCache,
}

impl<'a> LoaderContext<'a> {
    pub fn load_texture_from_file<P, F>(
        &mut self,
        path: P,
        usage: wgpu::TextureUsages,
        mut preprocess_image: F,
    ) -> Result<Arc<TextureAndView>, Error>
    where
        P: AsRef<Path>,
        F: FnMut(&mut image::RgbaImage, &PreprocessImageInfo) -> Result<(), Error>,
    {
        let path = path.as_ref();
        self.texture_cache.get_or_insert(path, || {
            tracing::debug!(path = %path.display(), "loaing texture from file");

            let label = path.display().to_string();

            let image = image::ImageReader::open(path)?.decode()?;
            let info = PreprocessImageInfo {
                original_color_type: image.color(),
            };
            let mut image = image.into_rgba8();

            preprocess_image(&mut image, &info)?;

            let texture = self
                .render_resource_creator
                .create_texture_from_image(&image, usage, &label);
            Ok(TextureAndView::from_texture(texture, &label))
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PreprocessImageInfo {
    pub original_color_type: image::ColorType,
}

struct RunLoaders<'a> {
    scene: &'a mut Scene,
    context: LoaderContext<'a>,
    result: Result<(), Error>,
}

impl<'a> RunLoaders<'a> {
    fn start_loading<L: LoadAsset>(&mut self) -> Result<(), Error> {
        for (entity, loader) in self.scene.entities.query_mut::<&L>() {
            // remove first, so if an error occurs during loading, the loader will still be
            // removed.
            self.scene.command_buffer.remove_one::<L>(entity);

            let mut loading_state = loader.start_loading(&mut self.context)?;

            // try to load it immediately
            match loading_state.poll(&mut self.context)? {
                LoadingProgress::Pending => {
                    self.scene
                        .command_buffer
                        .insert_one(entity, LoadingStateContainer(loading_state));
                }
                LoadingProgress::Ready(loaded) => {
                    self.scene.command_buffer.insert(entity, loaded);
                }
            }
        }
        Ok(())
    }

    fn poll_loaders<L: LoadingState>(&mut self) -> Result<(), Error> {
        for (entity, loading_state) in self
            .scene
            .entities
            .query_mut::<&mut LoadingStateContainer<L>>()
        {
            match loading_state.0.poll(&mut self.context) {
                Ok(LoadingProgress::Pending) => {}
                Ok(LoadingProgress::Ready(loaded)) => {
                    self.scene.command_buffer.remove_one::<L>(entity);
                    self.scene.command_buffer.insert(entity, loaded);
                }
                Err(error) => {
                    self.scene.command_buffer.remove_one::<L>(entity);
                    return Err(error);
                }
            }
        }

        Ok(())
    }

    pub fn run<L: LoadAsset>(&mut self) {
        if self.result.is_ok() {
            self.result = self.start_loading::<L>();
            if self.result.is_ok() {
                self.result = self.poll_loaders::<L::State>();
            }

            // apply commands even if an error occurred.
            self.scene.apply_deferred();
        }
    }
}

// todo: make this generic
#[derive(Debug, Default)]
pub struct TextureCache {
    cache: HashMap<PathBuf, Weak<TextureAndView>>,
}

impl TextureCache {
    pub fn get_or_insert<E, L>(&mut self, path: &Path, load: L) -> Result<Arc<TextureAndView>, E>
    where
        L: FnOnce() -> Result<TextureAndView, E>,
    {
        if let Some(weak) = self.cache.get_mut(path) {
            if let Some(texture_and_view) = weak.upgrade() {
                Ok(texture_and_view)
            }
            else {
                let texture_and_view = Arc::new(load()?);
                *weak = Arc::downgrade(&texture_and_view);
                Ok(texture_and_view)
            }
        }
        else {
            let texture_and_view = Arc::new(load()?);
            self.cache
                .insert(path.to_owned(), Arc::downgrade(&texture_and_view));
            Ok(texture_and_view)
        }
    }
}

#[derive(Clone, Copy, Debug, hecs::Bundle)]
pub struct AndChanged<T> {
    pub component: T,
    pub changed: Changed<T>,
}

impl<T> From<T> for AndChanged<T> {
    fn from(value: T) -> Self {
        Self {
            component: value,
            changed: Default::default(),
        }
    }
}

/// A simple container we wrap around loading states, so that implementors for
/// LoadAsset can use Self as the state without confusing the loading systems.
#[derive(Debug)]
struct LoadingStateContainer<T>(T);
