// clippy, it's just this module, i promise!
#![allow(clippy::type_complexity)]

use std::{
    any::{
        TypeId,
        type_name,
    },
    borrow::Cow,
    collections::HashMap,
    fmt::Debug,
};

use crate::composer::scene::ui::{
    ComponentUi,
    ComponentWidget,
};

#[derive(Debug, Default)]
pub struct ComponentRegistry {
    registry: Vec<RegisteredComponent>,
    by_type_id: HashMap<TypeId, usize>,
}

impl ComponentRegistry {
    pub fn register<T>(&mut self) -> RegisteredComponentBuilder<'_>
    where
        T: hecs::Component + RegisterComponent,
    {
        let type_id = TypeId::of::<T>();
        let index = self.by_type_id.entry(type_id).or_insert_with(|| {
            let index = self.registry.len();
            self.registry.push(RegisteredComponent::new::<T>());
            index
        });
        let mut builder = RegisteredComponentBuilder {
            inner: &mut self.registry[*index],
        };
        T::register(&mut builder);
        tracing::debug!(component = ?builder.inner, "registered component");
        builder
    }

    pub fn register_builtin(&mut self) {
        macro_rules! register {
            ($ty:ty) => {
                self.register::<$ty>();
            };
        }
        crate::for_all_builtin!(register);
    }

    pub fn contains<T>(&self) -> bool
    where
        T: 'static,
    {
        self.by_type_id.contains_key(&TypeId::of::<T>())
    }

    pub fn get<T>(&self) -> Option<&RegisteredComponent>
    where
        T: 'static,
    {
        let index = self.by_type_id.get(&TypeId::of::<T>())?;
        Some(&self.registry[*index])
    }

    pub fn iter(&self) -> impl Iterator<Item = &RegisteredComponent> {
        self.registry.iter()
    }
}

pub struct RegisteredComponent {
    type_id: TypeId,
    type_name: &'static str,
    display_name: Option<Cow<'static, str>>,
    has: Box<dyn Fn(hecs::EntityRef) -> bool>,
    create: Option<Box<dyn Fn(&mut hecs::EntityBuilder)>>,
    component_ui: Option<
        Box<
            dyn Fn(
                hecs::EntityRef,
                &mut hecs::CommandBuffer,
                &mut egui::Ui,
                bool,
                egui::RichText,
            ) -> Option<egui::Response>,
        >,
    >,
    tag_changed: bool,
}

impl RegisteredComponent {
    fn new<T>() -> Self
    where
        T: hecs::Component,
    {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: type_name::<T>(),
            display_name: None,
            has: Box::new(|entity| entity.has::<T>()),
            create: None,
            component_ui: None,
            tag_changed: false,
        }
    }

    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    pub fn has(&self, entity_ref: hecs::EntityRef) -> bool {
        (self.has)(entity_ref)
    }

    pub fn can_create(&self) -> bool {
        self.create.is_some()
    }

    pub fn create(&self, builder: &mut hecs::EntityBuilder) {
        if let Some(create) = &self.create {
            create(builder);
        }
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub fn display_name_with_fallback(&self) -> &str {
        self.display_name.as_deref().unwrap_or(self.type_name)
    }

    pub fn component_ui(
        &self,
        entity: hecs::EntityRef,
        command_buffer: &mut hecs::CommandBuffer,
        ui: &mut egui::Ui,
    ) -> Option<egui::Response> {
        self.component_ui.as_ref().and_then(|component_ui| {
            (component_ui)(
                entity,
                command_buffer,
                ui,
                self.tag_changed,
                self.display_name_with_fallback().into(),
            )
        })
    }
}

impl Debug for RegisteredComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredComponent")
            .field("type_id", &self.type_id)
            .field("type_name", &self.type_name)
            .field("display_name", &self.display_name)
            .field("tag_changed", &self.tag_changed)
            .finish()
    }
}

#[derive(Debug)]
pub struct RegisteredComponentBuilder<'a> {
    inner: &'a mut RegisteredComponent,
}

impl<'a> AsRef<RegisteredComponent> for RegisteredComponentBuilder<'a> {
    fn as_ref(&self) -> &RegisteredComponent {
        self.inner
    }
}

impl<'a> RegisteredComponentBuilder<'a> {
    pub fn register_default<T>(&mut self, default: impl Fn() -> T + Send + Sync + 'static)
    where
        T: hecs::Component + Clone,
    {
        self.inner.create = Some(Box::new(move |builder| {
            builder.add(default());
        }));
    }

    pub fn register_display_name(&mut self, display_name: impl Into<Cow<'static, str>>) {
        self.inner.display_name = Some(display_name.into());
    }

    pub fn register_component_ui<T>(&mut self)
    where
        T: ComponentUi,
    {
        self.inner.component_ui = Some(Box::new(
            |entity, command_buffer, ui, mark_changed, display_name| {
                entity.get::<&mut T>().map(|mut component| {
                    let mut widget =
                        ComponentWidget::<T>::new(entity.entity(), command_buffer, &mut component);
                    widget.mark_changed = mark_changed;
                    widget.heading = Some(&display_name);
                    // todo: we could either pass this in from the caller, or from a flag stored in
                    // the registered component, or use a tag component to determine this.
                    widget.deletable = true;
                    ui.add(widget)
                })
            },
        ));
    }

    pub fn register_tag_changed(&mut self) {
        self.inner.tag_changed = true;
    }
}

pub trait RegisterComponent {
    fn register(builder: &mut RegisteredComponentBuilder) {
        let _ = builder;
    }
}

// todo: proc-macro
#[macro_export]
macro_rules! impl_register_component {
    ($ty:ty) => {
        impl_register_component!(@begin($ty, []));
    };
    ($ty:ty where $($params:tt)*) => {
        impl_register_component!(@begin($ty, [$($params)*]));
    };
    // begin impl with a given list of params.
    // this needs to pass the builder into the muncher
    (@begin($ty:ty, [$($params:tt)*])) => {
        impl $crate::composer::scene::components::RegisterComponent for $ty {
            #[allow(unused_variables)]
            fn register(builder: &mut $crate::composer::scene::components::RegisteredComponentBuilder) {
                builder.register_display_name(stringify!($ty));
                impl_register_component!(@munch($ty, [$($params)*], builder, {}));
            }
        }
    };
    // munch the params token stream
    (@munch($ty:ty, [default = $value:expr $(,$($rest:tt)*)?], $builder:ident, {$($code:tt)*})) => {
        impl_register_component!(@munch($ty, [$($($rest)*)?], $builder, {$($code)* $builder.register_default($value);}));
    };
    (@munch($ty:ty, [default $(,$($rest:tt)*)?], $builder:ident, {$($code:tt)*})) => {
        impl_register_component!(@munch($ty, [$($($rest)*)?], $builder, {$($code)* $builder.register_default(<$ty as Default>::default);}));
    };
    (@munch($ty:ty, [display_name = $value:expr $(,$($rest:tt)*)?], $builder:ident, {$($code:tt)*})) => {
        impl_register_component!(@munch($ty, [$($($rest)*)?], $builder, {$($code)* $builder.register_display_name($value);}));
    };
    (@munch($ty:ty, [ComponentUi $(,$($rest:tt)*)?], $builder:ident, {$($code:tt)*})) => {
        impl_register_component!(@munch($ty, [$($($rest)*)?], $builder, {$($code)* $builder.register_component_ui::<$ty>();}));
    };
    (@munch($ty:ty, [Changed $(,$($rest:tt)*)?], $builder:ident, {$($code:tt)*})) => {
        impl_register_component!(@munch($ty, [$($($rest)*)?], $builder, {$($code)* $builder.register_tag_changed();}));
    };
    (@munch($ty:ty, [], $builder:ident, {$($code:tt)*})) => {
        $($code)*
    };
}

mod builtin {
    #[macro_export]
    macro_rules! for_all_builtin {
        ($callback:ident) => {{
            use parry3d::bounding_volume::Aabb;
            use $crate::{
                composer::{
                    Selectable,
                    Selected,
                    scene::transform::{
                        GlobalTransform,
                        LocalTransform,
                    },
                },
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
                solver::observer::Observer,
            };

            $callback!(LocalTransform);
            $callback!(GlobalTransform);
            $callback!(Material);
            $callback!(Wireframe);
            $callback!(Outline);
            $callback!(Hidden);
            $callback!(PointLight);
            $callback!(AmbientLight);
            $callback!(CameraConfig);
            $callback!(ClearColor);
            $callback!(Selectable);
            $callback!(Selected);
            $callback!(Aabb);
            $callback!(Observer);
            $callback!(cem_solver::material::Material);
        }};
    }
}
