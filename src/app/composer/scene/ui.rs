pub use std::any::type_name;

pub use show_all::show_all as debug;

use crate::app::composer::{
    properties::PropertiesUi,
    scene::{
        Changed,
        EntityDebugLabel,
        Label,
        Scene,
    },
};

#[derive(derive_more::Debug)]
pub struct EntityPropertiesWindow<'a> {
    scene: &'a mut Scene,
    entity: &'a mut Option<hecs::Entity>,
    id: egui::Id,
}

impl<'a> EntityPropertiesWindow<'a> {
    pub fn new(id: egui::Id, scene: &'a mut Scene, entity: &'a mut Option<hecs::Entity>) -> Self {
        Self { scene, entity, id }
    }

    pub fn show<R>(
        &mut self,
        ctx: &egui::Context,
        title: impl FnOnce(hecs::EntityRef<'_>) -> egui::WidgetText,
        add_contents: impl FnOnce(&mut egui::Ui, hecs::EntityRef<'_>, &mut hecs::CommandBuffer) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
        let entity = (*self.entity)?;

        let Ok(entity_ref) = self.scene.entities.entity(entity)
        else {
            // entity deleted?
            *self.entity = None;
            return None;
        };

        let mut is_open = true;

        let response = egui::Window::new(title(entity_ref))
            .id(self.id)
            .movable(true)
            .collapsible(true)
            .open(&mut is_open)
            .show(ctx, |ui| {
                add_contents(ui, entity_ref, &mut self.scene.command_buffer)
            });

        self.scene.apply_deferred();

        if !is_open {
            *self.entity = None;
        }

        response
    }

    pub fn show_query<Q, R>(
        &mut self,
        ctx: &egui::Context,
        title: impl FnOnce(hecs::EntityRef<'_>) -> egui::WidgetText,
        add_contents: impl FnOnce(&mut egui::Ui, Q::Item<'_>, &mut hecs::CommandBuffer) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>>
    where
        Q: hecs::Query,
    {
        self.show(ctx, title, |ui, entity_ref, command_buffer| {
            let mut query = entity_ref.query::<Q>();
            if let Some(query) = query.get() {
                Some(add_contents(ui, query, command_buffer))
            }
            else {
                // todo: does this work?
                ui.colored_label(egui::Color32::RED, "Error: Entity doesn't match query");
                ui.close();
                None
            }
        })
        .map(|inner_response| {
            egui::InnerResponse {
                inner: inner_response.inner.flatten(),
                response: inner_response.response,
            }
        })
    }
}

#[derive(derive_more::Debug)]
pub struct ComponentUi<'a, T>
where
    T: PropertiesUi,
{
    #[debug("hecs::EntityRef {{ ... }}")]
    entity_ref: hecs::EntityRef<'a>,

    #[debug("hecs::CommandBuffer {{ ... }}")]
    command_buffer: &'a mut hecs::CommandBuffer,

    deletable: bool,
    mark_changed: bool,

    config: &'a T::Config,
}

impl<'a, T> ComponentUi<'a, T>
where
    T: PropertiesUi,
{
    pub fn new(
        entity_ref: hecs::EntityRef<'a>,
        command_buffer: &'a mut hecs::CommandBuffer,
        config: &'a T::Config,
    ) -> Self {
        Self {
            entity_ref,
            command_buffer,
            deletable: false,
            mark_changed: false,
            config,
        }
    }

    pub fn deletable(mut self) -> Self {
        self.deletable = true;
        self
    }

    pub fn mark_changed(mut self) -> Self {
        self.mark_changed = true;
        self
    }
}

impl<'a, T> egui::Widget for ComponentUi<'a, T>
where
    T: hecs::Component + PropertiesUi,
{
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let entity = self.entity_ref.entity();

        if let Some(mut value) = self.entity_ref.get::<&mut T>() {
            let mut deletion_requested = false;

            ui.horizontal(|ui| {
                ui.heading(type_name::<T>());
                if self.deletable && ui.small_button("Delete").clicked() {
                    deletion_requested = true;
                }
            });

            let response = value.properties_ui(ui, self.config);
            ui.separator();

            if deletion_requested {
                tracing::debug!(?entity, component = type_name::<T>(), "removing");
                self.command_buffer.remove_one::<T>(entity);
            }
            else if response.changed() && self.mark_changed {
                tracing::debug!(?entity, component = type_name::<T>(), "marking as changed");
                self.command_buffer
                    .insert_one(entity, Changed::<T>::default());
            }

            response
        }
        else {
            ui.allocate_response(Default::default(), egui::Sense::empty())
        }
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

mod show_all {
    use crate::app::composer::{
        properties::PropertiesUi,
        renderer::{
            Outline,
            light::{
                AmbientLight,
                Material,
                PointLight,
            },
        },
        scene::{
            transform::Transform,
            ui::ComponentUi,
        },
    };

    pub fn show_all(
        deletable_components: bool,
    ) -> impl FnOnce(&mut egui::Ui, hecs::EntityRef, &mut hecs::CommandBuffer) {
        move |ui, entity_ref, command_buffer| {
            show_all_with_config(ui, entity_ref, command_buffer, deletable_components)
        }
    }

    fn show_component<T>(
        ui: &mut egui::Ui,
        entity_ref: hecs::EntityRef,
        command_buffer: &mut hecs::CommandBuffer,
        deletable_components: bool,
        track_changes: bool,
    ) where
        T: PropertiesUi + hecs::Component,
    {
        let config = T::Config::default();
        let mut component_ui = ComponentUi::<T>::new(entity_ref, command_buffer, &config);

        if deletable_components {
            component_ui = component_ui.deletable();
        }
        if track_changes {
            component_ui = component_ui.mark_changed();
        }

        //ui.label(label);

        ui.add(component_ui);
    }

    pub fn show_all_with_config(
        ui: &mut egui::Ui,
        entity_ref: hecs::EntityRef,
        command_buffer: &mut hecs::CommandBuffer,
        deletable_components: bool,
    ) {
        macro_rules! show_all {
            {
                untagged: {$($ty1:ty,)*};
                tagged: {$($ty2:ty,)*};
            } => {
                $(
                    show_component::<$ty1>(ui, entity_ref, command_buffer, deletable_components, false);
                )*
                $(
                    show_component::<$ty2>(ui, entity_ref, command_buffer, deletable_components, true);
                )*
            };
        }

        show_all! {
            untagged: {
                Material,
                PointLight,
                AmbientLight,
                Outline,
            };
            tagged: {
                Transform,
            };
        };
    }
}
