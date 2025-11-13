pub mod renderer;
pub mod scene;
pub mod solver;
pub mod tree;
pub mod view;

use std::{
    convert::Infallible,
    fs::File,
    io::BufReader,
    path::{
        Path,
        PathBuf,
    },
};

use base64::engine::Engine;
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
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    Error,
    app::{
        composer::{
            renderer::{
                ClearColor,
                Outline,
                Renderer,
                WgpuContext,
                camera::{
                    CameraConfig,
                    CameraProjection,
                },
            },
            scene::{
                EntityDebugLabel,
                Label,
                PopulateScene,
                Scene,
                Transform,
                serialize::DeserializeEntity,
                undo::{
                    HadesId,
                    UndoAction,
                    UndoBuffer,
                },
            },
            solver::SolverConfig,
            tree::ObjectTreeState,
            view::{
                ScenePointer,
                SceneView,
            },
        },
        config::{
            AppConfig,
            ComposerConfig,
        },
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
    pub fn new_file(&mut self, app_config: &AppConfig) {
        let mut state = ComposerState::new(app_config.composer.clone());
        ExampleScene
            .populate_scene(&mut state.scene)
            .expect("populating example scene failed");
        state.solver_configs = app_config.default_solver_configs.clone();
        self.state = Some(state);
    }

    /// Opens a file and populate the scene with it.
    pub fn open_file(
        &mut self,
        app_config: &AppConfig,
        path: impl AsRef<Path>,
    ) -> Result<(), Error> {
        let path = path.as_ref();
        tracing::debug!(path = %path.display(), "open file");

        if let Some(file_format) = guess_file_format_from_path(path) {
            #[allow(unreachable_patterns)]
            match file_format {
                FileFormat::Nec => {
                    let reader = BufReader::new(File::open(path)?);
                    let nec_file = NecFile::from_reader(reader)?;
                    tracing::debug!("{nec_file:#?}");

                    let mut state = ComposerState::new(app_config.composer.clone());

                    PopulateWithNec {
                        nec_file: &nec_file,
                        material: palette::named::ORANGERED.into(),
                    }
                    .populate_scene(&mut state.scene)?;

                    state.solver_configs = app_config.default_solver_configs.clone();
                    state.path = Some(path.to_owned());
                    state.fit_camera_to_scene(&Default::default());

                    self.state = Some(state);
                }
                _ => bail!("Unsupported file format: {file_format:?}"),
            }
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
        f: impl FnOnce(&mut ComposerState, Vec<hecs::Entity>) -> R,
    ) -> Option<R> {
        self.state.as_mut().map(|state| {
            let selected = state.selection().entities();
            f(state, selected)
        })
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

    pub fn solver_configurations(&self) -> impl Iterator<Item = &SolverConfig> {
        self.state
            .as_ref()
            .map(|state| state.solver_configs.iter())
            .into_iter()
            .flatten()
    }

    pub fn run_solver(&self, index: usize) {
        if let Some(state) = &self.state {
            let solver_config = &state.solver_configs[index];

            tracing::debug!("todo: run {} solver", solver_config.name);
        }
    }
}

/// State for an open file
#[derive(derive_more::Debug)]
pub struct ComposerState {
    config: ComposerConfig,

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

    /// the object tree shown in the left panel
    object_tree: ObjectTreeState,

    /// If an context menu is open, which entity is it about
    context_menu_object: Option<hecs::Entity>,

    /// Buffer storing undo and redo commands
    undo_buffer: UndoBuffer,

    /// Temporary ECS command buffer
    #[debug("hecs::CommandBuffer {{ ... }}")]
    command_buffer: hecs::CommandBuffer,

    solver_configs: Vec<SolverConfig>,
}

impl ComposerState {
    fn new(config: ComposerConfig) -> Self {
        let mut scene = Scene::default();

        // the only view we have right now
        // todo: don't create camera here. for a proper project file it will be
        // populated by it.
        let view_config = &config.views.view_3d;
        let camera_entity = scene.entities.spawn((
            Transform::look_at(
                &Point3::new(0.0, 0.0, -2.0),
                &Point3::origin(),
                &Vector3::y_axis(),
            ),
            ClearColor::from(view_config.background_color),
            CameraProjection::new(view_config.fovy.to_radians()),
            CameraConfig::default(),
            view_config.light_filter.unwrap_or_default(),
            //PointLight::default(),
            Label::new_static("camera"),
        ));

        let undo_buffer = UndoBuffer::new(config.undo_limit, config.redo_limit);

        Self {
            config,
            path: None,
            modified: false,
            scene,
            camera_entity,
            scene_pointer: Default::default(),
            object_tree: Default::default(),
            context_menu_object: None,
            undo_buffer,
            command_buffer: Default::default(),
            solver_configs: vec![],
        }
    }
}

impl ComposerState {
    pub fn show(&mut self, ctx: &egui::Context, renderer: &mut Renderer) {
        // prepare world
        self.scene.prepare();
        renderer.prepare_world(&mut self.scene);

        {
            let mut copy = false;
            let mut cut = false;
            let mut paste = None;

            ctx.input(|input| {
                for event in &input.events {
                    match event {
                        egui::Event::Copy => copy = true,
                        egui::Event::Cut => cut = true,
                        egui::Event::Paste(text) if paste.is_none() => paste = Some(text.clone()),
                        _ => {}
                    }
                }
            });

            if copy || cut {
                let selection = self.selection().entities();
                if !selection.is_empty() {
                    self.copy(ctx, selection.iter().copied());
                    if cut {
                        self.delete(selection);
                    }
                }
            }
            if let Some(text) = paste {
                self.paste(&text);
            }
        }

        // left panel: shows object tree
        egui::SidePanel::left(egui::Id::new("left_panel"))
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::both()
                    .scroll([false, true])
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .show(ui, |ui| self.object_tree(ui));
            });

        // central panel: shows scene views (cameras)
        egui::CentralPanel::default().show(ctx, |ui| {
            {
                // this whole block is just for debugging
                self.selection()
                    .with_query_iter::<Option<&Label>, _>(|mut selected| {
                        let num_selected = selected.len();
                        if num_selected == 0 {
                            ui.label("Nothing selected");
                        }
                        else {
                            let (entity, label) = selected.next().unwrap();

                            ui.label(format!(
                                "Selected: {}{}",
                                EntityDebugLabel {
                                    entity,
                                    label: label.cloned(),
                                    invalid: false
                                },
                                if num_selected > 1 {
                                    format!(" ({} more)", num_selected - 1)
                                }
                                else {
                                    Default::default()
                                }
                            ));
                        }
                    });

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
            }

            // actually render the scene
            let view_response = ui.add(
                SceneView::new(&mut self.scene, renderer)
                    .with_camera(self.camera_entity)
                    .with_scene_pointer(&mut self.scene_pointer),
            );

            if view_response.clicked() {
                // todo: shift should also remove from selection

                let shift_key = ui.input(|input| input.modifiers.shift);
                let entity = self
                    .scene_pointer
                    .entity_under_pointer
                    .as_ref()
                    .map(|entity_under_pointer| entity_under_pointer.entity);

                let mut selection = self.selection_mut();

                match (entity, shift_key) {
                    (Some(entity), false) => {
                        selection.clear();
                        selection.select(entity);
                    }
                    (Some(entity), true) => {
                        selection.toggle(entity);
                    }
                    (None, false) => {
                        selection.clear();
                    }
                    (None, true) => {}
                }
            }

            self.context_menu(&view_response);
        });
    }

    pub fn context_menu(&mut self, response: &egui::Response) {
        // todo: make this context menu work for the tree

        if response.secondary_clicked() {
            // if the clicked entity is in the selection, we might want to treat have to
            // context menu being about all objects

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
                self.copy(ui.ctx(), [entity]);
                self.delete([entity]);
            }

            if ui.button("Copy").clicked() {
                self.copy(ui.ctx(), [entity]);
            }

            if ui.button("Paste").clicked() {
                tracing::debug!("todo: cut");
            }

            ui.separator();

            if ui.button("Delete").clicked() {
                self.delete([entity]);
            }

            ui.separator();

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

    pub fn selection(&self) -> Selection<'_> {
        Selection {
            world: &self.scene.entities,
        }
    }

    pub fn selection_mut(&mut self) -> SelectionMut<'_> {
        SelectionMut {
            world: &mut self.scene.entities,
            command_buffer: &mut self.command_buffer,
            outline: &self.config.views.selection_outline,
        }
    }

    fn send_to_hades(
        &mut self,
        entities: impl IntoIterator<Item = hecs::Entity>,
        mut before_deletion: impl FnMut(&mut Scene, hecs::Entity),
    ) -> Vec<HadesId> {
        entities
            .into_iter()
            .filter_map(|entity| {
                // removes selection from to-be-removed entity. thus when the delete/cut is
                // undone, it isn't auto-selected. not sure what is a good behavior.
                let _ = self.scene.entities.remove_one::<Outline>(entity);
                let _ = self.scene.entities.remove_one::<Selected>(entity);

                before_deletion(&mut self.scene, entity);

                if let Some(taken_entity) = self.scene.delete(entity) {
                    Some(self.undo_buffer.send_to_hades(taken_entity))
                }
                else {
                    tracing::warn!(?entity, "Selected entity doesn't exist");
                    None
                }
            })
            .collect()
    }

    pub fn delete(&mut self, entities: impl IntoIterator<Item = hecs::Entity>) {
        let hades_ids = self.send_to_hades(entities, |_, _| {});
        self.undo_buffer
            .push_undo(UndoAction::DeleteEntity { hades_ids });
    }

    pub fn copy(&mut self, ctx: &egui::Context, entities: impl IntoIterator<Item = hecs::Entity>) {
        // this is rather hacky, doesn't use our local buffer/clipboard extension and
        // pollutes the OS clipboard.
        // todo: error handling

        let entities = entities
            .into_iter()
            .filter_map(|entity| self.scene.serialize(entity))
            .collect();

        let clipboard = SceneClipboard::Entities { entities };

        // serialize to json
        let json = serde_json::to_vec_pretty(&clipboard).unwrap();

        // compress (lz4)
        let compressed = lz4_flex::compress_prepend_size(&json);

        // base64 encode into data url
        let mut encoded = CLIPBOARD_PREFIX.to_owned();
        base64::engine::general_purpose::URL_SAFE.encode_string(&compressed, &mut encoded);

        // send to OS clipboard
        tracing::debug!("copying entities to clipboard: {} bytes", encoded.len());
        ctx.copy_text(encoded);
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(encoded) = text.strip_prefix(CLIPBOARD_PREFIX) {
            let mut compressed = Vec::with_capacity(encoded.len());
            base64::engine::general_purpose::URL_SAFE
                .decode_vec(encoded, &mut compressed)
                .unwrap();

            let decompressed = lz4_flex::decompress_size_prepended(&compressed).unwrap();

            let clipboard: SceneClipboard<DeserializeEntity> =
                serde_json::from_slice(&decompressed).unwrap();

            match clipboard {
                SceneClipboard::Entities { entities } => {
                    // todo: might want to modify them (positions?)
                    // todo: might want to spawn them in a temporary world to query them
                    for mut entity in entities {
                        self.scene.entities.spawn(entity.entity_builder.build());
                    }
                }
            }
        }
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

/// Tag for entities that are selected.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Selected;

#[derive(Serialize, Deserialize)]
enum SceneClipboard<E> {
    Entities {
        // todo: metadata: timestamp, version, ...
        entities: Vec<E>,
    },
}

#[derive(Clone, Copy, derive_more::Debug)]
pub struct Selection<'a> {
    #[debug("hecs::World {{ ... }}")]
    world: &'a hecs::World,
}

impl<'a> Selection<'a> {
    pub fn is_empty(&self) -> bool {
        self.world.query::<()>().with::<&Selected>().iter().len() == 0
    }

    pub fn query<Q>(&self) -> hecs::QueryBorrow<'_, hecs::With<Q, &Selected>>
    where
        Q: hecs::Query,
    {
        self.world.query::<Q>().with::<&Selected>()
    }

    pub fn with_query_iter<Q, R>(
        &self,
        f: impl FnOnce(hecs::QueryIter<'_, hecs::With<Q, &Selected>>) -> R,
    ) -> R
    where
        Q: hecs::Query,
    {
        let mut query = self.query::<Q>();
        f(query.iter())
    }

    pub fn entities(&self) -> Vec<hecs::Entity> {
        self.with_query_iter::<(), _>(|selected| selected.map(|(entity, ())| entity).collect())
    }
}

pub const CLIPBOARD_PREFIX: &'static str = "data:application/x-fdtd;base64,";

#[derive(derive_more::Debug)]
pub struct SelectionMut<'a> {
    #[debug("hecs::World {{ ... }}")]
    world: &'a mut hecs::World,
    #[debug("hecs::CommandBuffer {{ ... }}")]
    command_buffer: &'a mut hecs::CommandBuffer,
    outline: &'a Outline,
}

impl<'a> SelectionMut<'a> {
    pub fn clear(&mut self) {
        for (entity, ()) in self.world.query_mut::<()>().with::<&Selected>() {
            self.command_buffer.remove_one::<Selected>(entity);
            self.command_buffer.remove_one::<Outline>(entity);
        }
    }

    pub fn select(&mut self, entity: hecs::Entity) {
        self.command_buffer
            .insert(entity, (Selected, self.outline.clone()));
    }

    pub fn unselect(&mut self, entity: hecs::Entity) {
        self.command_buffer.remove_one::<Selected>(entity);
        self.command_buffer.remove_one::<Outline>(entity);
    }

    pub fn toggle(&mut self, entity: hecs::Entity) {
        if let Ok(selected) = self.world.satisfies::<&Selected>(entity) {
            if selected {
                self.unselect(entity);
            }
            else {
                self.select(entity);
            }
        }
    }
}

impl<'a> Drop for SelectionMut<'a> {
    fn drop(&mut self) {
        self.command_buffer.run_on(self.world);
    }
}
