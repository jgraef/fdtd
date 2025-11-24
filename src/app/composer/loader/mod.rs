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
            light::{
                LoadMaterialTextures,
                TextureAndView,
            },
            mesh::LoadMesh,
            resource::RenderResourceCreator,
        },
        scene::Scene,
    },
    util::ImageLoadExt,
};

#[derive(Debug)]
pub struct AssetLoader {
    render: RenderResourceCreator,

    /// Texture cache
    ///
    /// Caches textures loaded from files.
    texture_cache: TextureCache,
}

impl AssetLoader {
    pub fn new(render: RenderResourceCreator) -> Self {
        Self {
            render,
            texture_cache: Default::default(),
        }
    }

    pub fn run_all(&mut self, scene: &mut Scene) -> Result<(), Error> {
        let mut run_loaders = RunLoaders {
            scene,
            context: LoaderContext {
                render: &mut self.render,
                texture_cache: &mut self.texture_cache,
            },
        };

        let mut load_result = run_loaders.run::<LoadMaterialTextures>();
        load_result = load_result.and_then(|()| run_loaders.run::<LoadMesh>());
        if let Err(error) = &load_result {
            tracing::warn!(?error);
        }

        load_result
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
    pub render: &'a mut RenderResourceCreator,
    texture_cache: &'a mut TextureCache,
}

impl<'a> LoaderContext<'a> {
    pub fn load_texture_from_file<P>(&mut self, path: P) -> Result<Arc<TextureAndView>, Error>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        self.texture_cache.get_or_insert(path, || {
            let label = path.display().to_string();
            let image = image::RgbaImage::from_path(path)?;
            let texture = self.render.create_texture_from_image(&image, &label);
            Ok(TextureAndView::from_texture(texture, &label))
        })
    }
}

struct RunLoaders<'a> {
    scene: &'a mut Scene,
    context: LoaderContext<'a>,
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
                    self.scene.command_buffer.insert_one(entity, loading_state);
                }
                LoadingProgress::Ready(loaded) => {
                    self.scene.command_buffer.insert(entity, loaded);
                }
            }
        }
        Ok(())
    }

    fn poll_loaders<L: LoadingState>(&mut self) -> Result<(), Error> {
        for (entity, loading_state) in self.scene.entities.query_mut::<&mut L>() {
            match loading_state.poll(&mut self.context) {
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

    pub fn run<L: LoadAsset>(&mut self) -> Result<(), Error> {
        let mut result = self.start_loading::<L>();
        if result.is_ok() {
            result = self.poll_loaders::<L::State>();
        }

        // apply commands even if an error occurred.
        self.scene.apply_deferred();

        result
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
