use crate::{
    Error,
    app::composer::{
        renderer::Renderer,
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

pub struct LoaderContext<'a> {
    pub renderer: &'a Renderer,
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
