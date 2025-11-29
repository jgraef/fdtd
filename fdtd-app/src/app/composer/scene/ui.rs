pub use std::any::type_name;

use hecs_hierarchy::Hierarchy;

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

                        egui::containers::menu::MenuButton::from_button(
                            egui::Button::new("+").small(),
                        )
                        .ui(ui, |ui| {
                            show_all::show_add_component_menu(
                                ui,
                                entity_ref,
                                &mut self.scene.command_buffer,
                            );
                        });
                    });
                });
                ui.separator();

                show_all::show_component_uis(ui, entity_ref, &mut self.scene.command_buffer, true);
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
    T: PropertiesUi,
{
    #[debug("hecs::EntityRef {{ ... }}")]
    entity_ref: hecs::EntityRef<'a>,

    #[debug("hecs::CommandBuffer {{ ... }}")]
    command_buffer: &'a mut hecs::CommandBuffer,

    deletable: bool,
    mark_changed: bool,

    type_name: &'a str,

    config: &'a T::Config,
}

impl<'a, T> ComponentWidget<'a, T>
where
    T: PropertiesUi,
{
    pub fn new(
        entity_ref: hecs::EntityRef<'a>,
        command_buffer: &'a mut hecs::CommandBuffer,
        type_name: &'a str,
        config: &'a T::Config,
    ) -> Self {
        Self {
            entity_ref,
            command_buffer,
            deletable: false,
            mark_changed: false,
            type_name,
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

impl<'a, T> egui::Widget for ComponentWidget<'a, T>
where
    T: hecs::Component + PropertiesUi + ComponentUi,
{
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let entity = self.entity_ref.entity();

        if let Some(mut value) = self.entity_ref.get::<&mut T>() {
            let mut deletion_requested = false;

            ui.horizontal(|ui| {
                //ui.heading(value.heading());
                ui.heading(self.type_name);

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

// todo: we can just use PropertiesUi for this
// it might be better to just use `stringify!($ty)` for this.
pub trait ComponentUi {
    fn heading(&self) -> impl Into<egui::RichText>;
}

mod show_all {
    use parry3d::bounding_volume::Aabb;

    use crate::app::composer::{
        Selectable,
        Selected,
        properties::PropertiesUi,
        renderer::{
            ClearColor,
            Hidden,
            Outline,
            camera::CameraConfig,
            light::{
                AmbientLight,
                PointLight,
            },
            material::{
                Material,
                Wireframe,
            },
        },
        scene::{
            Changed,
            transform::{
                GlobalTransform,
                LocalTransform,
            },
            ui::{
                ComponentUi,
                ComponentWidget,
            },
        },
    };

    fn show_component_ui<T>(
        ui: &mut egui::Ui,
        type_name: &'static str,
        entity_ref: hecs::EntityRef,
        command_buffer: &mut hecs::CommandBuffer,
        deletable_components: bool,
        is_first: &mut bool,
        mark_changed: bool,
    ) where
        T: PropertiesUi + hecs::Component + ComponentUi,
    {
        if entity_ref.has::<T>() {
            if !*is_first {
                ui.separator();
            }
            *is_first = false;

            let config = T::Config::default();
            let mut component_ui =
                ComponentWidget::<T>::new(entity_ref, command_buffer, type_name, &config);

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

    fn show_component_add_button<T>(
        ui: &mut egui::Ui,
        type_name: &'static str,
        entity_ref: hecs::EntityRef,
        command_buffer: &mut hecs::CommandBuffer,
        mark_changed: bool,
    ) where
        T: PropertiesUi + hecs::Component + ComponentUi + Default,
    {
        if !entity_ref.has::<T>() && ui.small_button(type_name).clicked() {
            let mut builder = hecs::EntityBuilder::new();
            if mark_changed {
                builder.add(Changed::<T>::default());
            }
            builder.add(T::default());
            command_buffer.insert(entity_ref.entity(), builder.build());
        }
    }

    macro_rules! for_all_components {
        ($callback:ident) => {
            // [mark as changed, add component button]

            $callback!(LocalTransform, [true, true]);
            $callback!(GlobalTransform, [false, false]);

            $callback!(Material, [true, true]);
            $callback!(Wireframe, [true, true]);
            $callback!(Outline, [false, true]);
            $callback!(Hidden, [false, true]);

            $callback!(PointLight, [false, true]);
            $callback!(AmbientLight, [false, true]);
            $callback!(CameraConfig, [false, true]);
            $callback!(ClearColor, [false, true]);

            $callback!(Selectable, [false, true]);
            $callback!(Selected, [false, true]);

            $callback!(Aabb, [false, false]);
        };
    }

    pub fn show_component_uis(
        ui: &mut egui::Ui,
        entity_ref: hecs::EntityRef,
        command_buffer: &mut hecs::CommandBuffer,
        deletable_components: bool,
    ) {
        let mut is_first = true;

        macro_rules! show_component {
            ($ty:ty, [$mark_changed:expr, $add_button:expr]) => {{
                show_component_ui::<$ty>(
                    ui,
                    stringify!($ty),
                    entity_ref,
                    command_buffer,
                    deletable_components,
                    &mut is_first,
                    $mark_changed,
                );
            }};
        }

        for_all_components!(show_component);

        // this then shows a checkbox to enable the despawn button lol
        //show_component!(EntityWindow);
    }

    pub fn show_add_component_menu(
        ui: &mut egui::Ui,
        entity_ref: hecs::EntityRef,
        command_buffer: &mut hecs::CommandBuffer,
    ) {
        macro_rules! show_component_add_button {
            ($ty:ty, [$mark_changed:expr, true]) => {{
                show_component_add_button::<$ty>(
                    ui,
                    stringify!($ty),
                    entity_ref,
                    command_buffer,
                    $mark_changed,
                );
            }};
            ($ty:ty, [$mark_changed:expr, false]) => {};
        }

        for_all_components!(show_component_add_button);
    }
}
