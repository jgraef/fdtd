use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    entity::Entity,
    query::With,
    reflect::ReflectComponent,
    system::{
        Commands,
        In,
        InRef,
        Query,
        SystemParam,
    },
    world::World,
};
use bevy_reflect::{
    Reflect,
    prelude::ReflectDefault,
};
use cem_probe::PropertiesUi;
use cem_scene::probe::{
    ComponentName,
    ReflectComponentUi,
};
use cem_util::egui::EguiUtilUiExt;
use serde::{
    Deserialize,
    Serialize,
};

use crate::renderer::components::Outline;

/// Tag component for entities that are selected.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Component, Reflect)]
#[reflect(Component, ComponentUi, @ComponentName::new("Selected"), Default)]
pub struct Selected;

impl PropertiesUi for Selected {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        ui.noop()
    }
}

/// Tag component for entities that can be selected.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Component, Reflect)]
#[reflect(Component, ComponentUi, @ComponentName::new("Selectable"), Default)]
pub struct Selectable;

impl PropertiesUi for Selectable {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        ui.noop()
    }
}

/// System parameter to query and modify the selection.
///
/// All modification are deferred via [`Commands`]
#[derive(SystemParam)]
pub struct Selection<'w, 's> {
    selected: Query<'w, 's, Entity, With<Selected>>,
    selectable: Query<'w, 's, Entity, With<Selectable>>,
    commands: Commands<'w, 's>,
}

impl<'w, 's> Selection<'w, 's> {
    pub fn clear(&mut self) {
        self.selected.iter().for_each(|entity| {
            self.commands.entity(entity).remove::<(Selected, Outline)>();
        })
    }

    pub fn select(&mut self, entity: Entity, outline: impl Bundle) {
        if self.selectable.contains(entity) {
            self.commands.entity(entity).insert((Selected, outline));
        }
    }

    pub fn unselect(&mut self, entity: Entity) {
        self.commands.entity(entity).remove::<(Selected, Outline)>();
    }

    pub fn toggle(&mut self, entity: Entity, outline: impl Bundle) {
        if self.selected.contains(entity) {
            self.commands.entity(entity).remove::<(Selected, Outline)>();
        }
        else if self.selectable.contains(entity) {
            self.commands.entity(entity).insert((Selected, outline));
        }
    }

    pub fn select_all<O>(&mut self, outline: O)
    where
        O: Bundle + Clone,
    {
        self.selectable.iter().for_each(|entity| {
            self.commands
                .entity(entity)
                .insert((Selected, outline.clone()));
        });
    }

    pub fn count(&mut self) -> usize {
        self.selected.iter().count()
    }

    pub fn is_empty(&mut self) -> bool {
        self.count() == 0
    }

    pub fn entities(&mut self) -> impl Iterator<Item = Entity> {
        self.selected.iter()
    }
}

/// A proxy to access the selection state of a world.
#[derive(Debug)]
pub struct SelectionWorldMut<'a> {
    pub world: &'a mut World,
    pub outline: &'a Outline,
}

impl<'a> SelectionWorldMut<'a> {
    pub fn with_selection<R, F>(&mut self, f: F) -> R
    where
        F: FnOnce(Selection, &Outline) -> R + 'static,
        R: 'static,
    {
        self.world
            .run_system_cached_with(
                |(In(f), InRef(outline)): (In<F>, InRef<Outline>), selection: Selection| {
                    f(selection, outline)
                },
                (f, self.outline),
            )
            .unwrap()
    }

    pub fn clear(&mut self) {
        self.with_selection(|mut selection: Selection<'_, '_>, _outline| selection.clear());
    }

    pub fn select(&mut self, entity: Entity) {
        self.with_selection(move |mut selection, outline| selection.select(entity, *outline));
    }

    pub fn unselect(&mut self, entity: Entity) {
        self.with_selection(move |mut selection, _outline| selection.unselect(entity));
    }

    pub fn toggle(&mut self, entity: Entity) {
        self.with_selection(move |mut selection, outline| selection.toggle(entity, *outline));
    }

    pub fn select_all(&mut self) {
        self.with_selection(move |mut selection, outline| selection.select_all(*outline));
    }

    pub fn count(&mut self) -> usize {
        self.with_selection(move |mut selection, _outline| selection.count())
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
