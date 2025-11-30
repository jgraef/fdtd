pub use std::any::type_name;

use hecs_hierarchy::Hierarchy;

use crate::{
    composer::EntityWindow,
    scene::{
        Changed,
        EntityDebugLabel,
        Label,
        Scene,
    },
    util::egui::probe::PropertiesUi,
};

#[derive(derive_more::Debug)]
pub struct EntityPropertiesWindow<'a> {
    scene: &'a mut Scene,
    entity: hecs::Entity,
    id: egui::Id,
    entity_deletable: bool,
    components_deletable: bool,
}

impl<'a> EntityPropertiesWindow<'a> {
    pub fn new(id: egui::Id, scene: &'a mut Scene, entity: hecs::Entity) -> Self {
        Self {
            scene,
            entity,
            id,
            entity_deletable: false,
            components_deletable: false,
        }
    }

    pub fn deletable(mut self, deletable: bool) -> Self {
        self.entity_deletable = deletable;
        self
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        title: impl FnOnce(hecs::EntityRef<'_>) -> egui::WidgetText,
    ) -> Option<egui::Response> {
        let entity_ref = self.scene.entities.entity(self.entity).ok()?;

        let mut is_open = true;
        let mut delete_requested = false;

        let response = egui::Window::new(title(entity_ref))
            .id(self.id)
            .movable(true)
            .collapsible(true)
            .open(&mut is_open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Ok(parent) = self.scene.entities.parent::<()>(self.entity)
                        && ui.small_button(format!("Parent: {parent:?}")).clicked()
                    {
                        self.scene
                            .command_buffer
                            .insert_one(parent, EntityWindow::default());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        if ui
                            .add_enabled(
                                self.entity_deletable,
                                egui::Button::new("Despawn").small(),
                            )
                            .clicked()
                        {
                            delete_requested = true;
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
                            let addable_components =
                                self.scene.component_registry.iter().filter(|component| {
                                    !component.has(entity_ref) && component.can_create()
                                });

                            for component in addable_components {
                                if ui
                                    .small_button(component.display_name_with_fallback())
                                    .clicked()
                                {
                                    let mut builder = hecs::EntityBuilder::new();
                                    component.create(&mut builder);
                                    self.scene
                                        .command_buffer
                                        .insert(entity_ref.entity(), builder.build());
                                }
                            }
                        });
                    });
                });
                ui.separator();

                let mut emit_seperator = false;
                for component in self.scene.component_registry.iter() {
                    if emit_seperator {
                        ui.separator();
                        emit_seperator = false;
                    }

                    let response =
                        component.component_ui(entity_ref, &mut self.scene.command_buffer, ui);

                    if response.is_some() {
                        emit_seperator = true;
                    }
                }
            });

        if delete_requested {
            self.scene.command_buffer.despawn(self.entity);
        }

        if !is_open {
            self.scene
                .command_buffer
                .remove_one::<EntityWindow>(self.entity);
        }

        response.map(|response| response.response)
    }
}

#[derive(derive_more::Debug)]
pub struct ComponentWidget<'a, T>
where
    T: ?Sized,
{
    #[debug("hecs::EntityRef {{ ... }}")]
    pub entity: hecs::Entity,

    #[debug("hecs::CommandBuffer {{ ... }}")]
    pub command_buffer: &'a mut hecs::CommandBuffer,

    pub component: &'a mut T,

    pub heading: Option<&'a egui::RichText>,
    pub mark_changed: bool,
    pub deletable: bool,
}

impl<'a, T> ComponentWidget<'a, T> {
    pub fn new(
        entity: hecs::Entity,
        command_buffer: &'a mut hecs::CommandBuffer,
        component: &'a mut T,
    ) -> Self {
        Self {
            entity,
            command_buffer,
            component,
            heading: None,
            mark_changed: false,
            deletable: false,
        }
    }
}

impl<'a, T> egui::Widget for ComponentWidget<'a, T>
where
    T: ComponentUi,
{
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let mut deletion_requested = false;

        ui.horizontal(|ui| {
            if let Some(heading) = self.heading {
                ui.heading(heading.clone());
            }

            if self.deletable {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if ui.small_button("Delete").clicked() {
                        deletion_requested = true;
                    }
                });
            }
        });

        let response = ui
            .indent(egui::Id::NULL, |ui| self.component.properties_ui(ui, &()))
            .inner;

        if deletion_requested {
            tracing::debug!(entity = ?self.entity, component = type_name::<T>(), "removing");
            self.command_buffer.remove_one::<T>(self.entity);
        }
        else if response.changed() && self.mark_changed {
            self.command_buffer
                .insert_one(self.entity, Changed::<T>::default());
        }

        response
    }
}

pub fn default_title(entity_ref: hecs::EntityRef) -> egui::WidgetText {
    let label = entity_ref.get::<&Label>().map(|label| (*label).clone());
    EntityDebugLabel {
        entity: entity_ref.entity(),
        label,
        invalid: false,
    }
    .into()
}

/// Dyn-compatible trait for components that can render an UI
pub trait ComponentUi: hecs::Component + PropertiesUi<Config = ()> {}

impl<T> ComponentUi for T where T: hecs::Component + PropertiesUi<Config = ()> {}
