use std::collections::HashSet;

use hecs_hierarchy::Hierarchy;
use nalgebra::{
    Isometry3,
    Point3,
    Translation3,
    UnitQuaternion,
    UnitVector3,
    Vector3,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    app::composer::{
        properties::PropertiesUi,
        scene::Changed,
    },
    impl_register_component,
};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct LocalTransform {
    /// Rotation followed by translation that transforms points from the
    /// object's local frame to the global frame.
    pub isometry: Isometry3<f32>,
}

impl LocalTransform {
    pub fn identity() -> Self {
        Self {
            isometry: Isometry3::identity(),
        }
    }

    pub fn new(
        translation: impl Into<Translation3<f32>>,
        rotation: impl Into<UnitQuaternion<f32>>,
    ) -> Self {
        Self {
            isometry: Isometry3::from_parts(translation.into(), rotation.into()),
        }
    }

    pub fn translate_local(&mut self, translation: &Translation3<f32>) {
        self.isometry.translation.vector +=
            self.isometry.rotation.transform_vector(&translation.vector);
    }

    pub fn translate_global(&mut self, translation: &Translation3<f32>) {
        self.isometry.translation.vector += &translation.vector;
    }

    pub fn rotate_local(&mut self, rotation: &UnitQuaternion<f32>) {
        self.isometry.rotation *= rotation;
    }

    pub fn rotate_global(&mut self, rotation: &UnitQuaternion<f32>) {
        self.isometry.append_rotation_mut(rotation);
    }

    pub fn rotate_around(&mut self, anchor: &Point3<f32>, rotation: &UnitQuaternion<f32>) {
        self.isometry
            .append_rotation_wrt_point_mut(rotation, anchor);
    }

    pub fn look_at(eye: &Point3<f32>, target: &Point3<f32>, up: &Vector3<f32>) -> Self {
        Self {
            isometry: Isometry3::face_towards(eye, target, up),
        }
    }

    /// Pan and tilt object (e.g. a camera) with a given `up` vector.
    ///
    /// Pan is the horizontal turning. Tilt is the vertical turning.
    pub fn pan_tilt(&mut self, pan: f32, tilt: f32, up: &Vector3<f32>) {
        let local_up =
            UnitVector3::new_normalize(self.isometry.rotation.inverse_transform_vector(up));
        let local_right = Vector3::x_axis();

        let rotation = UnitQuaternion::from_axis_angle(&local_up, -pan)
            * UnitQuaternion::from_axis_angle(&local_right, tilt);

        self.isometry.rotation *= rotation;
    }

    pub fn position(&self) -> Point3<f32> {
        self.isometry.translation.vector.into()
    }
}

impl From<Isometry3<f32>> for LocalTransform {
    fn from(value: Isometry3<f32>) -> Self {
        Self { isometry: value }
    }
}

impl From<Translation3<f32>> for LocalTransform {
    fn from(value: Translation3<f32>) -> Self {
        Self::from(Isometry3::from(value))
    }
}

impl From<Vector3<f32>> for LocalTransform {
    fn from(value: Vector3<f32>) -> Self {
        Self::from(Isometry3::from(value))
    }
}

impl From<Point3<f32>> for LocalTransform {
    fn from(value: Point3<f32>) -> Self {
        Self::from(value.coords)
    }
}

impl From<UnitQuaternion<f32>> for LocalTransform {
    fn from(value: UnitQuaternion<f32>) -> Self {
        Self::from(Isometry3::from_parts(Default::default(), value))
    }
}

impl PropertiesUi for LocalTransform {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        self.isometry.properties_ui(ui, &Default::default())
    }
}

impl_register_component!(LocalTransform where Changed, ComponentUi, default);

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GlobalTransform {
    isometry: Isometry3<f32>,
}

impl GlobalTransform {
    #[cfg(test)]
    pub fn new_test(isometry: Isometry3<f32>) -> Self {
        Self { isometry }
    }

    pub fn isometry(&self) -> &Isometry3<f32> {
        &self.isometry
    }

    pub fn position(&self) -> Point3<f32> {
        self.isometry.translation.vector.into()
    }
}

impl PropertiesUi for GlobalTransform {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        self.isometry.properties_ui(ui, &Default::default())
    }
}

impl_register_component!(GlobalTransform where Changed, ComponentUi);

#[derive(Debug, Default)]
pub(super) struct TransformHierarchyUpdater {
    need_update: HashSet<hecs::Entity>,
    updated: HashSet<hecs::Entity>,
}

impl TransformHierarchyUpdater {
    pub fn update(&mut self, world: &mut hecs::World, command_buffer: &mut hecs::CommandBuffer) {
        assert!(self.need_update.is_empty());
        assert!(self.updated.is_empty());

        // collect which entities need updating
        for (entity, ()) in world
            .query::<()>()
            .with::<(&LocalTransform, &Changed<LocalTransform>)>()
            .iter()
        {
            if self.need_update.insert(entity) {
                command_buffer.remove_one::<Changed<LocalTransform>>(entity);

                for descendant in world.descendants_depth_first::<()>(entity) {
                    if self.need_update.insert(descendant) {
                        command_buffer.remove_one::<Changed<LocalTransform>>(entity);
                    }
                }
            }
        }

        // insert missing global transforms
        for (entity, ()) in world
            .query::<()>()
            .with::<&LocalTransform>()
            .without::<&GlobalTransform>()
            .iter()
        {
            let global = GlobalTransform {
                isometry: Default::default(),
            };
            command_buffer.insert_one(entity, global);
            if self.need_update.insert(entity) {
                command_buffer.remove_one::<Changed<LocalTransform>>(entity);
            }
        }

        command_buffer.run_on(world);

        // update global transforms
        let mut update = UpdateGlobals {
            world,
            locals: world.view::<&LocalTransform>(),
            globals: world.view::<&mut GlobalTransform>(),
            command_buffer,
            updated: &mut self.updated,
        };

        for entity in self.need_update.drain() {
            update.get_updated_global(entity);
        }

        drop(update);
        self.updated.clear();
        command_buffer.run_on(world);
    }
}

struct UpdateGlobals<'world, 'component> {
    world: &'world hecs::World,
    locals: hecs::ViewBorrow<'world, &'component LocalTransform>,
    globals: hecs::ViewBorrow<'world, &'component mut GlobalTransform>,
    command_buffer: &'world mut hecs::CommandBuffer,
    updated: &'world mut HashSet<hecs::Entity>,
}

impl<'world, 'component> UpdateGlobals<'world, 'component> {
    fn get_updated_global(&mut self, entity: hecs::Entity) -> Option<Isometry3<f32>> {
        let global = self.globals.get_mut(entity)?;
        if self.updated.insert(entity) {
            let new_global = self.calculate_global(entity)?;

            let global = self.globals.get_mut(entity).expect(
                "every entity with a local transform must have a global transform at this point",
            );
            global.isometry = new_global;

            self.command_buffer
                .insert_one(entity, Changed::<GlobalTransform>::default());

            Some(global.isometry)
        }
        else {
            Some(global.isometry)
        }
    }

    fn calculate_global(&mut self, entity: hecs::Entity) -> Option<Isometry3<f32>> {
        let local = self.locals.get(entity)?.isometry;

        let new_global = if let Ok(parent) = self.world.parent::<()>(entity)
            && let Some(parent_global) = self.get_updated_global(parent)
        {
            parent_global * local
        }
        else {
            local
        };

        Some(new_global)
    }
}
