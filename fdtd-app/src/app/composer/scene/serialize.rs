use std::{
    any::{
        TypeId,
        type_name,
    },
    collections::HashMap,
    fmt::Debug,
    marker::PhantomData,
    sync::OnceLock,
};

use serde::ser::SerializeMap;

pub trait ComponentSerialize {
    // todo
}

pub trait ComponentDeserialize {
    // todo
}

pub struct SerializeEntity<'a> {
    pub entity_ref: hecs::EntityRef<'a>,
}

impl<'a> SerializeEntity<'a> {
    pub fn new(entity_ref: hecs::EntityRef<'a>) -> Self {
        Self { entity_ref }
    }
}

impl<'a> serde::Serialize for SerializeEntity<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        global().serialize(self.entity_ref, serializer)
    }
}

pub struct DeserializeEntity {
    pub entity: hecs::Entity,
    pub entity_builder: hecs::EntityBuilder,
}

impl<'de> serde::Deserialize<'de> for DeserializeEntity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (entity, entity_builder) = global().deserialize(deserializer)?;
        Ok(Self {
            entity,
            entity_builder,
        })
    }
}

#[derive(Debug, Default)]
pub struct EntitySerializer {
    handlers: Vec<Box<dyn Handler + Send + Sync + 'static>>,
    by_type_id: HashMap<TypeId, usize>,
    by_key: HashMap<String, usize>,
}

impl EntitySerializer {
    pub fn register<T>(&mut self)
    where
        for<'a> T: serde::Serialize + serde::Deserialize<'a> + hecs::Component,
    {
        let type_id = TypeId::of::<T>();

        self.by_type_id.entry(type_id).or_insert_with(|| {
            let handler = HandlerImpl {
                _phantom: PhantomData::<T>,
            };

            let key = handler.key().to_owned();

            let index = self.handlers.len();
            self.handlers.push(Box::new(handler));

            self.by_key.insert(key, index);

            index
        });
    }

    pub fn serialize<S>(
        &self,
        entity_ref: hecs::EntityRef,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        /*let num_elements = Some(entity_ref
        .component_types()
        .filter(|type_id| self.handlers.contains_key(type_id))
        .count()
        + 1);*/
        let num_elements = None;

        let mut map = serializer.serialize_map(num_elements)?;

        let id = entity_ref.entity().to_bits();
        map.serialize_entry("id", &id)?;

        for type_id in entity_ref.component_types() {
            if let Some(index) = self.by_type_id.get(&type_id) {
                let handler = &self.handlers[*index];
                let key = handler.key();
                let mut result = Ok(());

                handler.callback_with_erased_serialize(entity_ref, &mut |component| {
                    result = map.serialize_entry(key, component);
                });

                result?;
            }
        }

        map.end()
    }

    pub fn deserialize<'de, D>(
        &self,
        deserializer: D,
    ) -> Result<(hecs::Entity, hecs::EntityBuilder), D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(EntityVisitor::new(self))
    }
}

trait Handler: Debug + Send + Sync + 'static {
    fn key(&self) -> &str;

    fn callback_with_erased_serialize(
        &self,
        entity_ref: hecs::EntityRef,
        callback: &mut dyn FnMut(&dyn erased_serde::Serialize),
    );

    fn deserialize_into_entity_builder(
        &self,
        entity_builder: &mut hecs::EntityBuilder,
        deserializer: &mut dyn erased_serde::Deserializer,
    ) -> Result<(), erased_serde::Error>;
}

struct HandlerImpl<T> {
    _phantom: PhantomData<T>,
}

impl<T> Debug for HandlerImpl<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerImpl")
            .field("_phantom", &self._phantom)
            .finish()
    }
}

impl<T> Handler for HandlerImpl<T>
where
    for<'a> T: serde::Serialize + serde::Deserialize<'a> + hecs::Component,
{
    fn key(&self) -> &str {
        // todo
        type_name::<T>()
    }

    fn callback_with_erased_serialize(
        &self,
        entity_ref: hecs::EntityRef,
        callback: &mut dyn FnMut(&dyn erased_serde::Serialize),
    ) {
        let component = entity_ref.get::<&T>().unwrap();
        callback(&*component)
    }

    fn deserialize_into_entity_builder(
        &self,
        entity_builder: &mut hecs::EntityBuilder,
        deserializer: &mut dyn erased_serde::Deserializer,
    ) -> Result<(), erased_serde::Error> {
        let component = T::deserialize(deserializer)?;
        entity_builder.add(component);
        Ok(())
    }
}

struct EntityVisitor<'a> {
    handlers: &'a EntitySerializer,
}

impl<'a> EntityVisitor<'a> {
    pub fn new(handlers: &'a EntitySerializer) -> Self {
        Self { handlers }
    }
}

impl<'a, 'de> serde::de::Visitor<'de> for EntityVisitor<'a> {
    type Value = (hecs::Entity, hecs::EntityBuilder);

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an entity")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut id = None;
        let mut entity_builder = hecs::EntityBuilder::new();

        while let Some(key) = map.next_key::<&str>()? {
            if key == "id" {
                id = Some(map.next_value::<u64>()?);
            }
            else if let Some(index) = self.handlers.by_key.get(key) {
                let handler = &self.handlers.handlers[*index];
                map.next_value_seed(DeserializeSeedComponent {
                    handler: &**handler,
                    entity_builder: &mut entity_builder,
                })?;
            }
        }

        let id = id.ok_or_else(|| serde::de::Error::custom("Missing `id` field"))?;
        let id = hecs::Entity::from_bits(id)
            .ok_or_else(|| serde::de::Error::custom(format!("Invalid entity id: {}", id)))?;

        Ok((id, entity_builder))
    }
}

struct DeserializeSeedComponent<'a> {
    handler: &'a dyn Handler,
    entity_builder: &'a mut hecs::EntityBuilder,
}

impl<'a, 'de> serde::de::DeserializeSeed<'de> for DeserializeSeedComponent<'a> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut deserializer = <dyn erased_serde::Deserializer>::erase(deserializer);
        self.handler
            .deserialize_into_entity_builder(self.entity_builder, &mut deserializer)
            .map_err(serde::de::Error::custom)?;
        Ok(())
    }
}

pub fn global() -> &'static EntitySerializer {
    static GLOBAL: OnceLock<EntitySerializer> = OnceLock::new();
    GLOBAL.get_or_init(|| {
        let mut serializer = EntitySerializer::default();
        register_global::register_global(&mut serializer);
        serializer
    })
}

mod register_global {
    use crate::app::composer::{
        Selected,
        renderer::{
            ClearColor,
            Hidden,
            Outline,
            camera::{
                CameraConfig,
                CameraProjection,
            },
            grid::GridPlane,
            light::{
                AmbientLight,
                PointLight,
            },
            material::Material,
        },
        scene::{
            Label,
            serialize::EntitySerializer,
            transform::{
                GlobalTransform,
                LocalTransform,
            },
        },
        tree::ShowInTree,
    };

    pub(super) fn register_global(serializer: &mut EntitySerializer) {
        macro_rules! register {
            {$($ty:ty,)*} => {
                $(
                    serializer.register::<$ty>();
                )*
            };
        }

        register! {
            LocalTransform,
            GlobalTransform,
            Material,
            Label,
            Hidden,
            ShowInTree,
            Outline,
            Selected,
            CameraProjection,
            CameraConfig,
            AmbientLight,
            ClearColor,
            PointLight,
            GridPlane,
            //SharedShape,
        };
    }
}
