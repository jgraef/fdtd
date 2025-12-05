pub use std::any::type_name;

use bevy_ecs::{
    component::Component,
    entity::Entity,
    name::Name,
    reflect::{
        AppTypeRegistry,
        ReflectComponent,
    },
    world::World,
};
use bevy_reflect::{
    TypeRegistry,
    prelude::ReflectDefault,
};
use cem_scene::probe::{
    ReflectComponentUi,
    component_name,
};

/// Component for entities that have an entity window open
#[derive(Clone, Copy, Debug, Component)]
pub struct EntityWindow {
    pub despawn_button: bool,
    pub component_delete_buttons: bool,
}

impl Default for EntityWindow {
    fn default() -> Self {
        Self {
            despawn_button: true,
            component_delete_buttons: true,
        }
    }
}

pub fn show_entity_windows(ctx: &egui::Context, world: &mut World) {
    let type_registry = world.resource::<AppTypeRegistry>().clone();

    let mut query = world.query::<(Entity, &EntityWindow)>();
    let windows = query
        .iter(world)
        .map(|(entity, window)| (entity, *window))
        .collect::<Vec<_>>();

    let type_registry = type_registry.read();
    for (entity, window) in windows {
        EntityWindowRenderer::new(world, entity, &type_registry)
            .entity_deletable(window.despawn_button)
            .components_deletable(window.component_delete_buttons)
            .show(ctx);
    }
}

#[derive(derive_more::Debug)]
pub struct EntityWindowRenderer<'a> {
    id: egui::Id,
    world: &'a mut World,
    entity: Entity,
    #[debug(skip)]
    type_registry: &'a TypeRegistry,
    entity_deletable: bool,
    components_deletable: bool,
}

impl<'a> EntityWindowRenderer<'a> {
    pub fn new(world: &'a mut World, entity: Entity, type_registry: &'a TypeRegistry) -> Self {
        let id = egui::Id::new("entity_window").with(entity);
        Self {
            id,
            world,
            entity,
            type_registry,
            entity_deletable: false,
            components_deletable: false,
        }
    }

    pub fn entity_deletable(mut self, deletable: bool) -> Self {
        self.entity_deletable = deletable;
        self
    }

    pub fn components_deletable(mut self, deletable: bool) -> Self {
        self.components_deletable = deletable;
        self
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<egui::Response> {
        let mut entity = self.world.entity_mut(self.entity);

        let mut is_open = true;
        let mut delete_entity = false;

        let title = entity.get::<Name>().map_or_else(
            || egui::WidgetText::from(entity.id().to_string()).monospace(),
            |name| egui::WidgetText::from(&**name),
        );

        let response = egui::Window::new(title)
            .id(self.id)
            .movable(true)
            .collapsible(true)
            .open(&mut is_open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // todo: bevy-migrate: entity ui: goto parent
                    /*if let Ok(parent) = self.scene.entities.parent::<()>(self.entity)
                        && ui.small_button(format!("Parent: {parent:?}")).clicked()
                    {
                        self.scene
                            .command_buffer
                            .insert_one(parent, EntityWindow::default());
                    }*/

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        if ui
                            .add_enabled(
                                self.entity_deletable,
                                egui::Button::new("Despawn").small(),
                            )
                            .clicked()
                        {
                            delete_entity = true;
                        }

                        //let selectable = entity_ref.satisfies::<&Selectable>();
                        let selectable = false; // todo
                        if ui
                            .add_enabled(selectable, egui::Button::new("Select").small())
                            .clicked()
                        {
                            // todo: need to be able to create a SelectionMut. we
                            // have the scene, and we could store the default
                            // outline in egui data
                            tracing::debug!("todo");
                        }

                        egui::containers::menu::MenuButton::from_button(
                            egui::Button::new("+").small(),
                        )
                        .ui(ui, |ui| {
                            // todo: bevy-migrate: add component

                            for (type_registration, _reflect_component_ui) in
                                self.type_registry.iter_with_data::<ReflectComponentUi>()
                            {
                                let type_info = type_registration.type_info();

                                let reflect_component = type_registration
                                    .data::<ReflectComponent>()
                                    .unwrap_or_else(|| {
                                        panic!(
                                            "ReflectComponentUi without ReflectComponent: {}",
                                            type_info.type_path()
                                        );
                                    });

                                let has_component = reflect_component.contains(&entity);

                                if let Some(reflect_default) =
                                    type_registration.data::<ReflectDefault>()
                                {
                                    if ui
                                        .add_enabled(
                                            !has_component,
                                            egui::Button::new(component_name(type_info)).small(),
                                        )
                                        .clicked()
                                    {
                                        let default = reflect_default.default();
                                        entity.insert_reflect(default);
                                    }
                                }
                            }
                        });
                    });
                });
                ui.separator();

                for (type_registration, reflect_component_ui) in
                    self.type_registry.iter_with_data::<ReflectComponentUi>()
                {
                    // todo: bevy-migrate entity ui

                    let type_info = type_registration.type_info();

                    let reflect_component = type_registration
                        .data::<ReflectComponent>()
                        .unwrap_or_else(|| {
                            panic!(
                                "ReflectComponentUi without ReflectComponent: {}",
                                type_info.type_path()
                            );
                        });

                    let mut delete_component = false;

                    if let Some(mut reflect) = reflect_component.reflect_mut(&mut entity) {
                        if let Some(component_ui) = reflect_component_ui.get_mut(&mut *reflect) {
                            egui::CollapsingHeader::new(component_name(type_info))
                                .id_salt(self.id.with("component").with(type_info.type_id()))
                                .default_open(true)
                                .show(ui, |ui| {
                                    component_ui.properties_ui(ui, &());

                                    if self.components_deletable {
                                        if ui.small_button("Delete").clicked() {
                                            delete_component = true;
                                        }
                                    }
                                });
                        }
                    }

                    if delete_component {
                        reflect_component.remove(&mut entity);
                    }
                }
            });

        if delete_entity {
            entity.despawn();
        }
        else if !is_open {
            entity.remove::<EntityWindow>();
        }

        response.map(|response| response.response)
    }
}
