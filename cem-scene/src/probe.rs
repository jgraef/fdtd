use std::{
    borrow::Cow,
    fmt::Display,
};

use bevy_reflect::{
    Reflect,
    TypeInfo,
    reflect_trait,
};
use cem_probe::PropertiesUi;

/// Dyn-compatible trait for components that can render an UI
#[reflect_trait]
pub trait ComponentUi: PropertiesUi<Config = ()> {}

impl<T> ComponentUi for T where T: PropertiesUi<Config = ()> {}

#[derive(Clone, Debug, Reflect)]
pub struct ComponentName {
    pub name: Cow<'static, str>,
}

impl ComponentName {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self { name: name.into() }
    }

    pub fn from_type_info(type_info: &TypeInfo) -> Option<&Self> {
        get_custom_attribute::<Self>(type_info)
    }
}

impl Display for ComponentName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

fn get_custom_attribute<T>(type_info: &TypeInfo) -> Option<&T>
where
    T: Reflect,
{
    match type_info {
        TypeInfo::Struct(struct_info) => struct_info.get_attribute::<T>(),
        TypeInfo::TupleStruct(tuple_struct_info) => tuple_struct_info.get_attribute::<T>(),
        TypeInfo::Enum(enum_info) => enum_info.get_attribute::<T>(),
        _ => None,
    }
}

pub fn component_name(type_info: &TypeInfo) -> egui::WidgetText {
    if let Some(component_name) = ComponentName::from_type_info(type_info) {
        egui::WidgetText::from(&*component_name.name)
    }
    else {
        egui::WidgetText::from(type_info.type_path()).monospace()
    }
}
