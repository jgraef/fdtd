use std::marker::PhantomData;

use bevy_ecs::{
    entity::Entity,
    query::QueryFilter,
    reflect::{
        AppTypeRegistry,
        ReflectComponent,
    },
    world::World,
};
use bevy_reflect::{
    ReflectSerialize,
    TypeRegistry,
    serde::ReflectSerializer,
};
use cem_util::serde::FlattenMapSerializer;
use serde::{
    Serialize,
    Serializer,
    ser::{
        SerializeMap,
        SerializeSeq,
    },
};

pub struct WorldSerialize<'world, F> {
    pub world: &'world World,
    pub _filter: PhantomData<F>,
}

impl<'world, F> WorldSerialize<'world, F> {
    pub fn new(world: &'world World) -> Self {
        Self {
            world,
            _filter: PhantomData,
        }
    }
}

impl<'world, F> Serialize for WorldSerialize<'world, F>
where
    F: QueryFilter,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let type_registry = self.world.resource::<AppTypeRegistry>().read();

        let mut entities_seq = serializer.serialize_seq(None)?;

        for entity in self
            .world
            .try_query_filtered::<Entity, F>()
            .unwrap()
            .iter(&self.world)
        {
            entities_seq.serialize_element(&EntitySerialize {
                world: self.world,
                type_registry: &type_registry,
                entity,
            })?;
        }

        entities_seq.end()
    }
}

pub struct EntitySerialize<'world> {
    pub world: &'world World,
    pub entity: Entity,
    pub type_registry: &'world TypeRegistry,
}

impl<'world> Serialize for EntitySerialize<'world> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut components_map = serializer.serialize_map(None)?;

        components_map.serialize_entry("id", &self.entity)?;

        let entity = self.world.entity(self.entity);

        let reflect_components = entity
            .archetype()
            .components()
            .into_iter()
            .copied()
            .filter_map(|component_id| {
                let type_id = self.world.components().get_info(component_id)?.type_id()?;
                let type_registration = self.type_registry.get(type_id)?;

                if type_registration.contains::<ReflectSerialize>() {
                    let reflect_component = type_registration
                        .data::<ReflectComponent>()?
                        .reflect(entity)?;

                    Some(reflect_component)
                }
                else {
                    None
                }
            });

        for reflect_component in reflect_components {
            let reflect_serializer = ReflectSerializer::new(reflect_component, self.type_registry);
            reflect_serializer.serialize(FlattenMapSerializer::new(&mut components_map))?;
        }

        components_map.end()
    }
}
