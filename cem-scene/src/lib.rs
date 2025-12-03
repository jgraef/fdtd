#![allow(dead_code)]
#![warn(clippy::todo, unused_qualifications)]

mod label;
pub mod plugin;
pub mod schedule;
pub mod spatial;
pub mod transform;

use std::sync::OnceLock;

use bevy_ecs::{
    message::{
        Message,
        MessageRegistry,
        Messages,
        message_update_system,
    },
    resource::Resource,
    schedule::{
        IntoScheduleConfigs,
        Schedule,
        ScheduleLabel,
        Schedules,
    },
    system::ScheduleSystem,
    world::World,
};
pub use label::Label;

use crate::{
    plugin::{
        Plugin,
        PluginRegistry,
    },
    spatial::SpatialQueryPlugin,
    transform::TransformHierarchyPlugin,
};

#[derive(Debug)]
pub struct Scene {
    pub world: World,
}

impl Scene {
    pub fn update(&mut self) {
        self.world.run_schedule(schedule::PreUpdate);
        self.world.run_schedule(schedule::Update);
        self.world.run_schedule(schedule::PostUpdate);
    }

    pub fn render(&mut self) {
        self.world.run_schedule(schedule::Render);
    }
}

#[derive(Debug)]
pub struct SceneBuilder {
    pub world: World,
    pub plugins: PluginRegistry,
}

impl Default for SceneBuilder {
    fn default() -> Self {
        let mut schedules = Schedules::new();

        schedules.insert(Schedule::new(schedule::Startup));
        schedules.insert(Schedule::new(schedule::PostStartup));
        schedules.insert(Schedule::new(schedule::PreUpdate));
        schedules.insert(Schedule::new(schedule::Update));
        schedules.insert(Schedule::new(schedule::PostStartup));
        schedules.insert(Schedule::new(schedule::Render));

        schedules.add_systems(schedule::PreUpdate, message_update_system);

        let mut world = World::new();
        world.insert_resource(schedules);

        Self {
            world,
            plugins: Default::default(),
        }
    }
}

impl SceneBuilder {
    pub fn build(mut self) -> Scene {
        self.world.run_schedule(schedule::Startup);
        self.world.run_schedule(schedule::PostStartup);

        Scene { world: self.world }
    }

    pub fn register_plugin(&mut self, plugin: impl Plugin) {
        if let Some(plugin) = self.plugins.register(plugin) {
            plugin.setup(self);
        }
    }

    pub fn register_plugins(&mut self, plugins: &PluginRegistry) {
        let new = self.plugins.register_all(plugins).collect::<Vec<_>>();
        for plugin in &new {
            plugin.setup(self);
        }
    }

    pub fn insert_resource(&mut self, resource: impl Resource) -> &mut Self {
        self.world.insert_resource(resource);
        self
    }

    pub fn add_systems<M>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        let mut schedules = self.world.resource_mut::<Schedules>();
        schedules.add_systems(schedule, systems);
        self
    }

    pub fn register_message<M>(&mut self) -> &mut Self
    where
        M: Message,
    {
        if !self.world.contains_resource::<Messages<M>>() {
            MessageRegistry::register_message::<M>(&mut self.world);
        }
        self
    }
}

pub fn builtin_plugins() -> &'static PluginRegistry {
    static BUILTIN: OnceLock<PluginRegistry> = OnceLock::new();
    BUILTIN.get_or_init(|| {
        let mut builtin = PluginRegistry::default();
        builtin.register(TransformHierarchyPlugin);
        builtin.register(SpatialQueryPlugin);
        builtin
    })
}

pub trait PopulateScene {
    type Error;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error>;
}
