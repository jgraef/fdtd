use std::borrow::Cow;

use bevy_ecs::{
    component::Component,
    query::With,
    reflect::ReflectComponent,
    world::World,
};
use bevy_reflect::{
    Reflect,
    prelude::ReflectDefault,
};
use cem_scene::serde::WorldSerialize;
use chrono::{
    DateTime,
    Local,
};
use serde::{
    Deserialize,
    Serialize,
};

pub const MAGIC: &str = "cem-project";
pub const VERSION: u64 = 0;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectFileData<S> {
    pub magic: Cow<'static, str>,
    pub version: u64,
    pub save_timestamp: DateTime<Local>,
    pub scene: S,
}

impl<'world> ProjectFileData<WorldSerialize<'world, With<SaveToFile>>> {
    pub fn from_world(world: &'world World) -> Self {
        Self {
            magic: MAGIC.into(),
            version: VERSION,
            save_timestamp: Local::now(),
            scene: WorldSerialize::<With<SaveToFile>>::new(world),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Component, Reflect)]
#[reflect(Component, Default)]
pub struct SaveToFile;
