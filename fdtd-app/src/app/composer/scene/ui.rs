pub use std::any::type_name;

use hecs_hierarchy::Hierarchy;
pub use show_all::show_all as debug;

use crate::app::composer::{
    EntityWindow,
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
    entity: hecs::Entity,
    id: egui::Id,
    deletable: bool,
}

impl<'a> EntityPropertiesWindow<'a> {
    pub fn new(id: egui::Id, scene: &'a mut Scene, entity: hecs::Entity) -> Self {
        Self {
            scene,
            entity,
            id,
            deletable: false,
        }
    }

    pub fn deletable(mut self, deletable: bool) -> Self {
        self.deletable = deletable;
        self
    }

    pub fn show<R>(
        &mut self,
        ctx: &egui::Context,
        title: impl FnOnce(hecs::EntityRef<'_>) -> egui::WidgetText,
        add_contents: impl FnOnce(&mut egui::Ui, hecs::EntityRef<'_>, &mut hecs::CommandBuffer) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
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
                            .add_enabled(self.deletable, egui::Button::new("Despawn").small())
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
                    });
                });
                ui.separator();

                add_contents(ui, entity_ref, &mut self.scene.command_buffer)
            });

        if delete_requested {
            self.scene.command_buffer.despawn(self.entity);
        }

        if !is_open {
            self.scene
                .command_buffer
                .remove_one::<EntityWindow>(self.entity);
        }

        response
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
    T: hecs::Component + PropertiesUi + ComponentUiHeading,
{
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let entity = self.entity_ref.entity();

        if let Some(mut value) = self.entity_ref.get::<&mut T>() {
            let mut deletion_requested = false;

            ui.horizontal(|ui| {
                ui.heading(value.heading());
                if self.deletable {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        if ui.small_button("Delete").clicked() {
                            deletion_requested = true;
                        }
                    });
                }
            });

            let response = ui
                .indent(egui::Id::NULL, |ui| value.properties_ui(ui, self.config))
                .inner;

            if deletion_requested {
                tracing::debug!(?entity, component = type_name::<T>(), "removing");
                self.command_buffer.remove_one::<T>(entity);
            }
            else if response.changed() && self.mark_changed {
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

pub trait ComponentUiHeading {
    fn heading(&self) -> impl Into<egui::RichText>;
}

mod show_all {
    use crate::app::composer::{
        Selectable,
        Selected,
        properties::PropertiesUi,
        renderer::{
            Outline,
            camera::CameraConfig,
            light::{
                AmbientLight,
                PointLight,
            },
            material::Material,
        },
        scene::{
            transform::{
                GlobalTransform,
                LocalTransform,
            },
            ui::{
                ComponentUi,
                ComponentUiHeading,
            },
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
        is_first: &mut bool,
        mark_changed: bool,
    ) where
        T: PropertiesUi + hecs::Component + ComponentUiHeading,
    {
        if entity_ref.has::<T>() {
            if !*is_first {
                ui.separator();
            }
            *is_first = false;

            let config = T::Config::default();
            let mut component_ui = ComponentUi::<T>::new(entity_ref, command_buffer, &config);

            if deletable_components {
                component_ui = component_ui.deletable();
            }
            if mark_changed {
                component_ui = component_ui.mark_changed();
            }

            //ui.label(label);

            ui.add(component_ui);
        }
    }

    pub fn show_all_with_config(
        ui: &mut egui::Ui,
        entity_ref: hecs::EntityRef,
        command_buffer: &mut hecs::CommandBuffer,
        deletable_components: bool,
    ) {
        let mut is_first = true;

        macro_rules! show_component {
            (@emit $ty:ty, $mark_changed:expr) => {{
                show_component::<$ty>(ui, entity_ref, command_buffer, deletable_components, &mut is_first, $mark_changed);
            }};
            ($ty:ty, Changed) => {
                show_component!(@emit $ty, true)
            };
            ($ty:ty) => {
                show_component!(@emit $ty, false)
            };
        }

        show_component!(LocalTransform, Changed);
        show_component!(GlobalTransform);
        show_component!(Material, Changed);
        show_component!(PointLight);
        show_component!(AmbientLight);
        show_component!(Outline);
        show_component!(CameraConfig);
        show_component!(Selectable);
        show_component!(Selected);
        // this then shows a checkbox to enable the despawn button lol
        //show_component!(EntityWindow);
    }
}
