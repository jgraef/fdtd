pub mod renderer;
pub mod scene;
pub mod tree;

use std::{
    convert::Infallible,
    f32,
    fs::File,
    io::BufReader,
    path::{
        Path,
        PathBuf,
    },
};

use color_eyre::eyre::bail;
use egui::RichText;
use nalgebra::{
    Isometry3,
    Point3,
    Translation3,
    UnitQuaternion,
    Vector2,
    Vector3,
};

use crate::{
    Error,
    app::composer::{
        renderer::{
            Outline,
            Renderer,
            WgpuContext,
            camera::CameraProjection,
        },
        scene::{
            PopulateScene,
            Scene,
            Transform,
            undo::UndoBuffer,
            view::{
                ScenePointer,
                SceneView,
            },
        },
        tree::ObjectTree,
    },
    file_formats::{
        FileFormat,
        guess_file_format_from_path,
        nec::{
            NecFile,
            PopulateWithNec,
        },
    },
    lipsum,
};

/// Scene composer widget.
///
/// This is stateful, so `&mut Composer` is the actual widget. It exists whether
/// a file is open or not and keeps track of that.
#[derive(Debug)]
pub struct Composer {
    /// The state of an open file
    pub state: Option<ComposerState>,

    /// The renderer used to render a scene (if a file is open)
    renderer: Renderer,
}

impl Composer {
    pub fn new(wgpu_context: &WgpuContext) -> Self {
        let renderer = Renderer::new(wgpu_context);

        Self {
            state: None,
            renderer,
        }
    }

    /// Creates a new file with an example scene
    pub fn new_file(&mut self) {
        let mut state = ComposerState::default();
        ExampleScene
            .populate_scene(&mut state.scene)
            .expect("populating example scene failed");
        //state.fit_camera_to_scene();
        self.state = Some(state);
    }

    /// Opens a file and populate the scene with it.
    pub fn open_file(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        tracing::debug!(path = %path.display(), "open file");

        if let Some(file_format) = guess_file_format_from_path(path) {
            let mut state = ComposerState::new_with_path(path);

            #[allow(unreachable_patterns)]
            match file_format {
                FileFormat::Nec => {
                    let reader = BufReader::new(File::open(path)?);
                    let nec_file = NecFile::from_reader(reader)?;
                    tracing::debug!("{nec_file:#?}");
                    PopulateWithNec {
                        nec_file: &nec_file,
                        material: palette::named::ORANGERED.into(),
                    }
                    .populate_scene(&mut state.scene)?;
                    state.fit_camera_to_scene(&Default::default());
                }
                _ => bail!("Unsupported file format: {file_format:?}"),
            }

            self.state = Some(state);
        }
        else {
            tracing::debug!("todo: unknown file format");
        }

        Ok(())
    }

    pub fn has_file_open(&self) -> bool {
        self.state.is_some()
    }

    /// Closes currently open file.
    ///
    /// # TODO
    ///
    /// Check if we need to save and prompt user for it.
    pub fn close_file(&mut self) {
        self.state = None;
    }

    /// Useful for actions from UI elements outside the compose (e.g. the menu
    /// bar), and we already checked that a file is open (e.g. for disabling a
    /// button).
    pub fn expect_state_mut(&mut self) -> &mut ComposerState {
        self.state.as_mut().expect("no file open")
    }

    /// Returns the camera.
    ///
    /// This runs an arbitrary query against the camera entity.
    /// Returns `None` if no file is open, or the camera entity doesn't exist
    /// anymore (which we maybe should treat as a bug).
    ///
    /// This can be used to easily modify the camera from outside of the
    /// composer (e.g. menu bar).
    ///
    /// # TODO
    ///
    /// Eventually we'll have multiple views/cameras. We could just use an enum
    /// describing which view we mean, or pass in the camera entity itself.
    /// Either way we will need a way to iterate over available cameras to e.g.
    /// construct the camera menu in the menu bar.
    pub fn camera_mut<'a, Q>(&'a mut self) -> Option<Q::Item<'a>>
    where
        Q: hecs::Query,
    {
        self.state.as_mut().and_then(|state| {
            state
                .scene
                .entities
                .query_one_mut::<Q>(state.camera_entity)
                .ok()
        })
    }

    pub fn with_selected<R>(
        &mut self,
        f: impl FnOnce(&mut ComposerState, hecs::Entity) -> R,
    ) -> Option<R> {
        self.state
            .as_mut()
            .and_then(|state| state.selected_object.map(|entity| f(state, entity)))
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if let Some(state) = &mut self.state {
            state.show(ctx, &mut self.renderer);
        }
        else {
            // what is being shown when no file is open
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(100.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Welcome!").heading());
                    ui.label(lipsum!(20));
                });
            });
        }
    }

    pub fn show_debug(&mut self, ui: &mut egui::Ui) {
        if let Some(state) = &mut self.state {
            ui.collapsing("Undo Buffer", |ui| {
                ui.label("Undo:");
                let mut empty = true;
                for undo_action in state.undo_buffer.iter_undo().take(10) {
                    empty = false;
                    ui.code(format!("{undo_action:?}"));
                }
                if empty {
                    ui.label("No undo actions");
                }

                ui.separator();
                ui.label("Redo:");
                let mut empty = true;
                for redo_action in state.undo_buffer.iter_redo().take(10) {
                    empty = false;
                    ui.code(format!("{redo_action:?}"));
                }
                if empty {
                    ui.label("No redo actions");
                }
            });
        }
    }
}

/// State for an open file
#[derive(Debug)]
pub struct ComposerState {
    /// The path of the file. This will be where it's saved to.
    ///
    /// This might need to keep track of how it's saved (e.g. file format)
    path: Option<PathBuf>,

    /// Whether the file was modified, since it was loaded or saved.
    modified: bool,

    /// The scene containing all objects
    scene: Scene,

    /// The camera used to render the scene.
    ///
    /// There will be one per view eventually
    camera_entity: hecs::Entity,

    /// Stores where in the scene our mouse is pointing.
    ///
    /// We also need one of these per camera.
    scene_pointer: ScenePointer,

    object_tree: ObjectTree,

    selected_object: Option<hecs::Entity>,
    context_menu_object: Option<hecs::Entity>,

    undo_buffer: UndoBuffer,
}

impl Default for ComposerState {
    fn default() -> Self {
        let mut scene = Scene::default();

        let camera_entity = scene.add_camera(Transform::look_at(
            &Point3::new(0.0, 0.0, -2.0),
            &Point3::origin(),
            &Vector3::y_axis(),
        ));

        Self {
            path: None,
            modified: false,
            scene,
            camera_entity,
            scene_pointer: Default::default(),
            object_tree: Default::default(),
            selected_object: None,
            context_menu_object: None,
            undo_buffer: Default::default(),
        }
    }
}

impl ComposerState {
    pub fn new_with_path(path: impl AsRef<Path>) -> Self {
        Self {
            path: Some(path.as_ref().to_owned()),
            ..Default::default()
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, renderer: &mut Renderer) {
        // prepare world
        self.scene.prepare();
        renderer.prepare_world(&mut self.scene);

        let selected_before_ui_input = self.selected_object;

        // left panel: shows object tree
        egui::SidePanel::left(egui::Id::new("left_panel"))
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::both()
                    .scroll([false, true])
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .show(ui, |ui| {
                        self.object_tree
                            .show(ui, &mut self.scene, &mut self.selected_object)
                    });
            });

        // central panel: shows scene views (cameras)
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(selected_entity) = self.selected_object {
                ui.label(format!(
                    "Selected: {}",
                    self.scene.entity_debug_label(selected_entity)
                ));
            }
            else {
                ui.label("Nothing selected");
            }

            // this just shows the hovered entity at the top. will be removed later.
            if let Some(entity_under_pointer) = &self.scene_pointer.entity_under_pointer {
                ui.label(format!(
                    "Hovered: {} at ({}, {}, {}) with {} distance",
                    self.scene.entity_debug_label(entity_under_pointer.entity),
                    entity_under_pointer.point_hovered.x,
                    entity_under_pointer.point_hovered.y,
                    entity_under_pointer.point_hovered.z,
                    entity_under_pointer.distance_from_camera
                ));
            }
            else {
                ui.label("Nothing hovered");
            }

            // actually render the scene
            let view_response = ui.add(
                SceneView::new(&mut self.scene, renderer)
                    .with_camera(self.camera_entity)
                    .with_scene_pointer(&mut self.scene_pointer),
            );

            if view_response.clicked() {
                // object selected/delected by left-lick
                self.selected_object = self
                    .scene_pointer
                    .entity_under_pointer
                    .as_ref()
                    .map(|entity_under_pointer| entity_under_pointer.entity);
            }

            self.context_menu(&view_response);
        });

        // selection changed
        if selected_before_ui_input != self.selected_object {
            if let Some(entity) = selected_before_ui_input {
                // remove outline tag from previously selected entity
                let _ = self.scene.entities.remove_one::<Outline>(entity);
            }

            if let Some(entity) = self.selected_object {
                // add outline tag to new selection
                let _ = self.scene.entities.insert_one(entity, Outline);
            }
        }
    }

    pub fn context_menu(&mut self, response: &egui::Response) {
        if response.secondary_clicked() {
            self.context_menu_object = self
                .scene_pointer
                .entity_under_pointer
                .map(|entity_under_pointer| entity_under_pointer.entity);
        }

        let Some(entity) = self.context_menu_object
        else {
            return;
        };

        let response = egui::Popup::context_menu(response).show(|ui| {
            ui.label(self.scene.entity_debug_label(entity));
            ui.separator();

            if ui.button("Cut").clicked() {
                tracing::debug!("todo: cut");
            }

            if ui.button("Copy").clicked() {
                tracing::debug!("todo: cut");
            }

            if ui.button("Paste").clicked() {
                tracing::debug!("todo: cut");
            }

            ui.separator();

            if ui.button("Delete").clicked() {
                self.delete(entity);
            }

            if ui.button("Properties").clicked() {
                tracing::debug!("todo: properties");
            }
        });

        if response.is_none() {
            self.context_menu_object = None;
        }
    }

    /// Moves the camera such that it fits the whole scene.
    ///
    /// Specifically this only translates the camera. It will be translated (by
    /// moving backwards) such that it will fit the AABB of the scene. The
    /// AABB is calculated relative to the camera orientation. The camera will
    /// also be translated laterally to its view axis to center to the AABB.
    pub fn fit_camera_to_scene(&mut self, margin: &Vector2<f32>) {
        // get camera transform and projection
        // note: we could use another transform if we want to reposition the camera e.g.
        // along a coordinate axis.
        let Ok((camera_transform, camera_projection)) = self
            .scene
            .entities
            .query_one_mut::<(&Transform, &CameraProjection)>(self.camera_entity)
            .map(|(t, p)| (*t, *p))
        else {
            return;
        };

        // compute scene AABB relative to camera
        let Some(scene_aabb) = self
            .scene
            .compute_aabb_relative_to_observer(&camera_transform, false)
        else {
            return;
        };

        // center camera on aabb
        let mut translation = scene_aabb.center().coords;
        translation.z -= camera_projection.distance_to_fit_aabb_into_fov(&scene_aabb, margin);

        // apply translation to camera
        let camera_transform = self
            .scene
            .entities
            .query_one_mut::<&mut Transform>(self.camera_entity)
            .expect("camera should still exist");
        camera_transform.translate_local(&Translation3::from(translation));
    }

    /// Fit the camera to the scene looking along a specified axis.
    ///
    /// This is meant to be used along the canonical axis of the scene. It will
    /// not calculate the scene's AABB as viewed along the axis, but instead
    /// just rotate the scene's AABB.
    pub fn fit_camera_to_scene_looking_along_axis(
        &mut self,
        axis: &Vector3<f32>,
        up: &Vector3<f32>,
        margin: &Vector2<f32>,
    ) {
        let Ok((camera_transform, camera_projection)) =
            self.scene
                .entities
                .query_one_mut::<(&mut Transform, &CameraProjection)>(self.camera_entity)
        else {
            return;
        };

        let rotation = UnitQuaternion::face_towards(axis, up);

        let reference_transform = Isometry3::from_parts(Translation3::identity(), rotation);

        let scene_aabb = self
            .scene
            .octtree
            .root_aabb()
            .transform_by(&reference_transform);

        let distance = camera_projection.distance_to_fit_aabb_into_fov(&scene_aabb, margin);

        let mut transform = Transform::from(Isometry3::from_parts(
            Translation3::from(scene_aabb.center().coords),
            rotation,
        ));
        transform.translate_local(&Translation3::from(-Vector3::z() * distance));

        *camera_transform = transform;
    }

    pub fn point_camera_to_scene_center(&mut self) {
        if let Ok(camera_transform) = self
            .scene
            .entities
            .query_one_mut::<&mut Transform>(self.camera_entity)
        {
            let scene_center = self.scene.octtree.root_aabb().center();
            let eye = camera_transform.position();

            // normally up is always +Y
            let mut up = Vector3::y();

            // but we need to take into account when we're directly above the scene center
            const COLLINEAR_THRESHOLD: f32 = 0.01f32.to_radians();
            if (eye - scene_center).cross(&up).norm_squared() < COLLINEAR_THRESHOLD {
                // we would be looking straight up or down, so keep the up vector from the
                // camera
                up = camera_transform.transform.rotation.transform_vector(&up);
                tracing::debug!(?eye, ?scene_center, ?up, "looking straight up or down");
            }

            *camera_transform = Transform::look_at(&eye, &scene_center, &up);
        }
    }

    pub fn has_undos(&self) -> bool {
        self.undo_buffer.has_undos()
    }

    pub fn has_redos(&self) -> bool {
        self.undo_buffer.has_redos()
    }

    pub fn undo(&mut self) {
        self.undo_buffer.undo_most_recent(&mut self.scene);
    }

    pub fn redo(&mut self) {
        tracing::debug!("todo: redo");
    }

    pub fn has_selected(&self) -> bool {
        self.selected_object.is_some()
    }

    pub fn delete(&mut self, entity: hecs::Entity) {
        let _ = self.scene.entities.remove_one::<Outline>(entity);

        if let Some(taken_entity) = self.scene.delete(entity) {
            self.undo_buffer.deleted_entity(taken_entity);
        }
        else {
            tracing::warn!(?entity, "Selected entity doesn't exist");
        }
    }

    pub fn cut(&mut self, _entity: hecs::Entity) {
        tracing::debug!("todo");
    }

    pub fn copy(&mut self, _entity: hecs::Entity) {
        tracing::debug!("todo");
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExampleScene;

impl PopulateScene for ExampleScene {
    type Error = Infallible;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error> {
        let shape = |size| parry3d::shape::Cuboid::new(Vector3::repeat(size));
        //let shape = |size| parry3d::shape::Ball::new(size);

        scene.add_object(Point3::new(-0.2, 0.0, 0.0), shape(0.1), palette::named::RED);
        scene.add_object(Point3::new(0.2, 0.0, 0.0), shape(0.1), palette::named::BLUE);
        scene.add_object(
            Point3::new(0.0, -0.2, 0.0),
            shape(0.1),
            palette::named::LIME,
        );
        scene.add_object(
            Point3::new(0.0, 0.2, 0.0),
            shape(0.1),
            palette::named::YELLOW,
        );
        scene.add_object(
            Point3::new(-0.02, -0.02, 0.2),
            shape(0.05),
            palette::named::MAGENTA,
        );
        scene.add_object(
            Point3::new(0.02, 0.02, -0.2),
            shape(0.05),
            palette::named::CYAN,
        );

        Ok(())
    }
}
