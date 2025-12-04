use bevy_ecs::{
    component::Component,
    entity::Entity,
    query::With,
    system::{
        Commands,
        In,
        Query,
    },
    world::World,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    impl_register_component,
    renderer::components::Outline,
    util::egui::{
        EguiUtilUiExt,
        probe::PropertiesUi,
    },
};

/// Tag for entities that are selected.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Component)]
pub struct Selected;

impl PropertiesUi for Selected {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        ui.noop()
    }
}

impl_register_component!(Selected where ComponentUi, default);

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Component)]
pub struct Selectable;

impl PropertiesUi for Selectable {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        ui.noop()
    }
}

impl_register_component!(Selectable where ComponentUi, default);

#[derive(Debug)]
pub struct Selection<'a> {
    pub world: &'a mut World,
    pub outline: &'a Outline,
}

impl<'a> Selection<'a> {
    pub fn clear(&mut self) {
        self.world
            .run_system_cached(
                |selection: Query<Entity, With<Selected>>, mut commands: Commands| {
                    selection.iter().for_each(|entity| {
                        commands.entity(entity).remove::<(Selected, Outline)>();
                    });
                },
            )
            .unwrap();
    }

    pub fn select(&mut self, entity: Entity) {
        let mut entity = self.world.entity_mut(entity);
        if entity.contains::<Selectable>() {
            entity.insert((Selected, *self.outline));
        }
    }

    pub fn unselect(&mut self, entity: Entity) {
        self.world
            .entity_mut(entity)
            .remove::<(Selected, Outline)>();
    }

    pub fn toggle(&mut self, entity: Entity) {
        let mut entity = self.world.entity_mut(entity);
        if entity.contains::<Selected>() {
            entity.remove::<(Selected, Outline)>();
        }
        else {
            entity.insert((Selected, *self.outline));
        }
    }

    pub fn select_all(&mut self) {
        self.world
            .run_system_cached_with(
                |In(outline): In<Outline>,
                 selectable: Query<Entity, With<Selectable>>,
                 mut commands: Commands| {
                    selectable.iter().for_each(|entity| {
                        commands.entity(entity).insert((Selected, outline));
                    });
                },
                *self.outline,
            )
            .unwrap();
    }

    pub fn count(&mut self) -> usize {
        self.world
            .run_system_cached(|query: Query<(), With<Selected>>| query.count())
            .unwrap()
    }

    pub fn is_empty(&mut self) -> bool {
        self.count() == 0
    }

    pub fn entities(&mut self) -> Vec<Entity> {
        self.world
            .run_system_cached(|query: Query<Entity, With<Selected>>| {
                query.iter().collect::<Vec<_>>()
            })
            .unwrap()
    }
}
