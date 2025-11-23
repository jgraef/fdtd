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
            Renderer,
            light::TextureAndView,
        },
        scene::Scene,
    },
};

pub trait Loader: hecs::Component {
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

pub struct LoaderContext<'a> {
    pub renderer: &'a mut Renderer,
}

impl<'a> LoaderContext<'a> {
    /// todo: some places (e.g. mesh from shape) need this. textures just use a
    /// bespoke function on the renderer. we need a better api.
    pub fn device(&self) -> &wgpu::Device {
        &self.renderer.wgpu_context.device
    }
}

pub struct RunLoaders<'a> {
    scene: &'a mut Scene,
    context: LoaderContext<'a>,
}

impl<'a> RunLoaders<'a> {
    pub fn new(renderer: &'a mut Renderer, scene: &'a mut Scene) -> Self {
        Self {
            scene,
            context: LoaderContext { renderer },
        }
    }

    fn start_loading<L: Loader>(&mut self) -> Result<(), Error> {
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

    pub fn run<L: Loader>(&mut self) -> Result<(), Error> {
        let mut result = self.start_loading::<L>();
        if result.is_ok() {
            result = self.poll_loaders::<L::State>();
        }

        // apply commands even if an error occurred.
        self.scene.apply_deferred();

        result
    }
}

#[derive(Debug, Default)]
pub struct TextureCache {
    cache: HashMap<PathBuf, Weak<TextureAndView>>,
}

impl TextureCache {
    pub fn get_or_insert<E, L>(&mut self, path: &Path, load: L) -> Result<Arc<TextureAndView>, E>
    where
        L: FnOnce() -> Result<Arc<TextureAndView>, E>,
    {
        if let Some(weak) = self.cache.get_mut(path) {
            if let Some(texture_and_view) = weak.upgrade() {
                Ok(texture_and_view)
            }
            else {
                let texture_and_view = load()?;
                *weak = Arc::downgrade(&texture_and_view);
                Ok(texture_and_view)
            }
        }
        else {
            let texture_and_view = load()?;
            self.cache
                .insert(path.to_owned(), Arc::downgrade(&texture_and_view));
            Ok(texture_and_view)
        }
    }
}
