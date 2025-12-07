use std::sync::Arc;

use bevy_ecs::{
    bundle::{
        Bundle,
        InsertMode,
    },
    entity::Entity,
    error::{
        CommandWithEntity,
        HandleError,
    },
    resource::Resource,
    schedule::{
        IntoScheduleConfigs,
        SystemSet,
    },
    system::{
        Command,
        EntityCommand,
        InMut,
        IntoSystem,
        Res,
        SystemParam,
        entity_command,
    },
    world::{
        CommandQueue,
        World,
    },
};
use bevy_tasks::{
    AsyncComputeTaskPool,
    TaskPoolBuilder,
};
use parking_lot::Mutex;

use crate::{
    plugin::Plugin,
    schedule,
};

#[derive(Clone, Debug)]
pub struct AsyncWorld {
    command_queue: SharedResource,
    update_trigger: AsyncUpdateTrigger,
}

impl AsyncWorld {
    pub fn entity(&self, entity: Entity) -> AsyncEntityWorld {
        AsyncEntityWorld {
            inner: self.clone(),
            entity,
        }
    }

    pub fn push(&self, command: impl Command) {
        let mut command_queue = self.command_queue.0.lock();
        command_queue.push(command);
        self.update_trigger.trigger();
    }

    pub fn append(&self, other: &mut CommandQueue) {
        let mut command_queue = self.command_queue.0.lock();
        command_queue.append(other);
        self.update_trigger.trigger();
    }
}

#[derive(Clone, Debug)]
pub struct AsyncEntityWorld {
    inner: AsyncWorld,
    entity: Entity,
}

impl AsyncEntityWorld {
    pub fn push(&mut self, command: impl EntityCommand) {
        self.inner
            .push(command.with_entity(self.entity).handle_error());
    }

    pub fn insert(&mut self, bundle: impl Bundle) {
        self.push(entity_command::insert(bundle, InsertMode::Replace));
    }
}

#[derive(Debug, SystemParam)]
pub struct SpawnAsync<'w> {
    command_queue: Res<'w, SharedResource>,
    update_trigger: Option<Res<'w, AsyncUpdateTrigger>>,
}

impl<'w> SpawnAsync<'w> {
    pub fn spawn<F, Fut, E>(&self, f: F)
    where
        F: FnOnce(AsyncWorld) -> Fut,
        Fut: Future<Output = Result<(), E>> + Send + 'static,
        E: std::error::Error,
    {
        let future = f(AsyncWorld {
            command_queue: self.command_queue.clone(),
            update_trigger: self.update_trigger.as_deref().cloned().unwrap_or_default(),
        });
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let result = future.await;
            if let Err(error) = result {
                // todo: handle error properly
                tracing::error!(%error);
            }
        });
        task.detach();
    }
}

#[derive(Clone, Debug, Default, Resource)]
struct SharedResource(Arc<Mutex<CommandQueue>>);

fn apply_async_commands(
    InMut((command_queue, buffer)): InMut<(SharedResource, CommandQueue)>,
    world: &mut World,
) {
    {
        let mut command_queue = command_queue.0.lock();
        std::mem::swap(&mut *command_queue, buffer);
    }
    buffer.apply(world);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AsyncPlugin;

impl Plugin for AsyncPlugin {
    fn setup(&self, builder: &mut crate::SceneBuilder) {
        // makes sure the AsyncComputeTaskPool is initialized
        let _pool = AsyncComputeTaskPool::get_or_init(|| {
            TaskPoolBuilder::new()
                .thread_name("scene-async".to_owned())
                .build()
        });

        let command_queue = SharedResource::default();
        let buffer = CommandQueue::default();

        builder.insert_resource(command_queue.clone()).add_systems(
            schedule::PostUpdate,
            apply_async_commands
                .with_input((command_queue, buffer))
                .in_set(AsyncSystems::ApplyDeferred),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, SystemSet)]
pub enum AsyncSystems {
    ApplyDeferred,
}

#[derive(derive_more::Debug, Clone, Resource)]
pub struct AsyncUpdateTrigger {
    #[debug(skip)]
    callback: Arc<dyn Fn() + Send + Sync>,
}

impl AsyncUpdateTrigger {
    pub fn new(callback: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    pub fn trigger(&self) {
        (self.callback)();
    }
}

impl Default for AsyncUpdateTrigger {
    fn default() -> Self {
        Self::new(|| ())
    }
}
