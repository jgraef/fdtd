use std::{
    borrow::Cow,
    fmt::Display,
};

use bevy_ecs::component::Component;

#[derive(Clone, Debug, Component)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Label {
    pub label: Cow<'static, str>,
}

impl Label {
    pub fn new(label: impl Display) -> Self {
        Self {
            label: label.to_string().into(),
        }
    }

    pub fn new_static(label: &'static str) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

impl From<&str> for Label {
    fn from(value: &str) -> Self {
        Self {
            label: value.to_owned().into(),
        }
    }
}

impl From<String> for Label {
    fn from(value: String) -> Self {
        Self {
            label: value.into(),
        }
    }
}
