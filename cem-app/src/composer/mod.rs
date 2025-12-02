pub mod entity;
pub mod file_formats;
pub mod loader;
pub mod menubar;
pub mod shape;
pub mod tree;
pub mod undo;
pub mod view;

use std::{
    borrow::Cow,
    convert::Infallible,
    fs::File,
    io::BufReader,
    path::{
        Path,
        PathBuf,
    },
};

use base64::engine::Engine;
use cem_solver::{
    FieldComponent,
    fdtd::{
        self,
        pml::GradedPml,
    },
    material::{
        Material,
        PhysicalConstants,
    },
    source::{
        ContinousWave,
        ScalarSourceFunctionExt,
        Source,
    },
};
use color_eyre::eyre::bail;
use hecs_hierarchy::HierarchyMut;
use nalgebra::{
    Isometry3,
    Point3,
    Translation3,
    UnitQuaternion,
    Vector2,
    Vector3,
};
use nec_file::NecFile;
use palette::WithAlpha;
use parry3d::shape::{
    Ball,
    Cuboid,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    Error,
    composer::{
        entity::EntityPropertiesWindow,
        file_formats::{
            FileFormat,
            guess_file_format_from_path,
            nec::PopulateWithNec,
        },
        loader::AssetLoader,
        menubar::ComposerMenuElements,
        shape::flat::{
            Quad,
            QuadMeshConfig,
        },
        tree::{
            ObjectTreeState,
            ShowInTree,
        },
        undo::{
            HadesId,
            UndoAction,
            UndoBuffer,
        },
        view::{
            ScenePointer,
            SceneView,
        },
    },
    config::{
        AppConfig,
        ComposerConfig,
    },
    debug::DebugUi,
    error::ResultExt,
    impl_register_component,
    lipsum,
    renderer::{
        ClearColor,
        Outline,
        Renderer,
        RendererInfo,
        camera::{
            CameraConfig,
            CameraProjection,
            CameraRenderInfo,
        },
        material,
        mesh::LoadMesh,
    },
    scene::{
        Changed,
        EntityDebugLabel,
        Label,
        PopulateScene,
        Scene,
        Spawn,
        serialize::DeserializeEntity,
        spatial::{
            Collider,
            SceneSpatialExt,
        },
        transform::{
            GlobalTransform,
            LocalTransform,
        },
    },
    solver::{
        config::{
            FixedVolume,
            Parallelization,
            SolverConfig,
            SolverConfigCommon,
            SolverConfigFdtd,
            SolverConfigSpecifics,
            StopCondition,
            Volume,
        },
        observer::{
            Observer,
            test_color_map,
        },
        runner::SolverRunner,
        ui::SolverConfigUiWindow,
    },
    util::egui::{
        EguiUtilUiExt,
        probe::PropertiesUi,
    },
};

#[derive(Debug, Default)]
pub struct Composers {
    composers: Vec<ComposerState>,
    active: Option<usize>,
}

impl Composers {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        renderer: &mut Renderer,
        asset_loader: &mut AssetLoader,
    ) {
        if self.composers.is_empty() {
            // what is being shown when no file is open
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(100.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Welcome!").heading());
                    ui.label(lipsum!(20));
                });
            });
        }
        else if let Some(index) = self.active {
            if let Some(composer) = self.composers.get_mut(index) {
                composer.show(ctx, renderer, asset_loader);
            }
            else {
                tracing::error!(index, "invalid active composer");
                self.active = Some(0);
            }
        }
    }

    pub fn show_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // todo: these buttons won't work for our tabs, since we need a small close
            // button inside of it. we need to roll our own widget *sigh*

            let style = ui.style_mut();
            style.spacing.item_spacing.x = 1.0;
            style.spacing.button_padding.x = 10.0;
            style.visuals.selection.bg_fill = style.visuals.widgets.inactive.weak_bg_fill;
            style.visuals.selection.stroke = style.visuals.widgets.inactive.fg_stroke;
            style.visuals.widgets.inactive.bg_fill = style.visuals.panel_fill;
            style.visuals.widgets.inactive.weak_bg_fill = style.visuals.panel_fill;
            style.visuals.widgets.active.fg_stroke.color =
                style.visuals.widgets.inactive.fg_stroke.color;

            for (i, composer) in self.composers.iter().enumerate() {
                let is_active = self.active.is_some_and(|active| active == i);
                let title = format!("🗋 {}", composer.title);

                let button = egui::Button::new(title)
                    .corner_radius(egui::CornerRadius {
                        nw: 4,
                        ne: 4,
                        sw: 0,
                        se: 0,
                    })
                    .selected(is_active)
                    .frame(true)
                    .frame_when_inactive(true);

                if ui.add(button).clicked() {
                    self.active = Some(i);
                }
            }
        });
    }

    fn open_composer(&mut self, composer: ComposerState) {
        if let Some(path) = &composer.path {
            tracing::debug!(path = %path.display(), "open composer");
        }
        else {
            tracing::debug!("open composer (no path)");
        }

        let index = self.composers.len();
        self.composers.push(composer);
        self.active = Some(index);
    }

    /// Creates a new file with an example scene
    pub fn new_file(&mut self, app_config: &AppConfig) {
        let mut state = ComposerState::new(app_config.composer.clone());

        ExampleScene
            .populate_scene(&mut state.scene)
            .expect("populating example scene failed");

        //PresetScene.populate_scene(&mut state.scene).expect("populating example scene
        // failed");

        self.open_composer(state);
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

                    state.path = Some(path.to_owned());
                    if let Some(file_name) = path.file_name() {
                        state.title = file_name.to_string_lossy().into_owned().into();
                    }

                    state.camera_mut().fit_to_scene(&Default::default());

                    self.open_composer(state);
                }
                _ => bail!("Unsupported file format: {file_format:?}"),
            }
        }
        else {
            tracing::debug!("todo: unknown file format");
        }

        Ok(())
    }

    pub fn close_file(&mut self) {
        if let Some(index) = self.active {
            self.active = index.checked_sub(1);
            self.composers.remove(index);
        }
    }

    pub fn has_file_open(&self) -> bool {
        !self.composers.is_empty()
    }

    fn active_mut(&mut self) -> Option<&mut ComposerState> {
        self.active.and_then(|index| self.composers.get_mut(index))
    }

    fn with_active<'a, R>(&'a mut self, f: impl FnOnce(&'a mut ComposerState) -> R) -> Option<R>
    where
        R: 'a,
    {
        self.active_mut().map(f)
    }

    /// todo: do we want to move this into ComposerMenuElements? It's only used
    /// there at the moment
    fn with_selected<R>(
        &mut self,
        f: impl FnOnce(&mut ComposerState, Vec<hecs::Entity>) -> R,
    ) -> Option<R> {
        self.with_active(|composer| {
            let selected = composer.selection().entities();
            f(composer, selected)
        })
    }

    pub fn menu_elements<'a>(
        &'a mut self,
        solver_runner: &'a mut SolverRunner,
    ) -> ComposerMenuElements<'a> {
        ComposerMenuElements {
            composers: self,
            solver_runner,
        }
    }
}

/// State for an open file
#[derive(derive_more::Debug)]
struct ComposerState {
    config: ComposerConfig,

    /// The path of the file. This will be where it's saved to.
    ///
    /// This might need to keep track of how it's saved (e.g. file format)
    path: Option<PathBuf>,

    title: Cow<'static, str>,

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

    solver_configs: Vec<SolverConfig>,
    solver_config_window: SolverConfigUiWindow,

    // note: only used as a temporary scratch buffer. this is queries from the scene right before
    // the windows are rendered
    entity_windows: Vec<(hecs::Entity, EntityWindow)>,
}

impl ComposerState {
    fn new(config: ComposerConfig) -> Self {
        let mut scene = Scene::default();

        // the only view we have right now
        // todo: don't create camera here. for a proper project file it will be
        // populated by it.
        let view_config = &config.views.view_3d;
        let camera_entity = scene.entities.spawn((
            LocalTransform::look_at(
                &Point3::new(0.0, 0.0, -1.5),
                &Point3::origin(),
                &Vector3::y_axis(),
            ),
            ClearColor::from(view_config.background_color),
            CameraProjection::new(view_config.fovy.to_radians()),
            CameraConfig {
                tone_map: view_config.tone_map,
                gamma: view_config.gamma,
                ..Default::default()
            },
            view_config.ambient_light,
            view_config.point_light,
            Label::new_static("camera"),
        ));

        let undo_buffer = UndoBuffer::new(config.undo_limit, config.redo_limit);

        // some test solver configs
        let solver_configs = {
            vec![
                make_config("CPU (single-threaded)", None),
                make_config(
                    "CPU (multi-threaded)",
                    Some(Parallelization::MultiThreaded { num_threads: None }),
                ),
                make_config("GPU", Some(Parallelization::Wgpu)),
            ]
        };

        Self {
            config,
            path: None,
            title: "Untitled".into(),
            modified: false,
            scene,
            camera_entity,
            scene_pointer: Default::default(),
            object_tree: Default::default(),
            context_menu_object: None,
            undo_buffer,
            solver_configs,
            solver_config_window: SolverConfigUiWindow::default(),
            entity_windows: vec![],
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        renderer: &mut Renderer,
        asset_loader: &mut AssetLoader,
    ) {
        // prepare world
        self.scene.prepare();

        asset_loader.run_all(&mut self.scene).ok_or_handle(ctx);
        renderer.prepare_world(&mut self.scene);

        // todo: give a RepaintTrigger to the solver runner
        //ctx.request_repaint_after(Duration::from_millis(1000 / 60));

        {
            // some input events are rather tricky to get

            let mut copy = false;
            let mut cut = false;
            let mut escape = false;
            let mut paste = None;

            ctx.input(|input| {
                for event in &input.events {
                    match event {
                        egui::Event::Copy => copy = true,
                        egui::Event::Cut => cut = true,
                        egui::Event::Paste(text) if paste.is_none() => paste = Some(text.clone()),
                        // todo: keybinds
                        egui::Event::Key {
                            key: egui::Key::Escape,
                            pressed: true,
                            repeat: false,
                            ..
                        } => escape = true,
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

            if escape {
                self.selection_mut().clear();
            }
        }

        // right panel: shows object tree
        egui::Panel::right(egui::Id::new("right_panel"))
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
            }

            {
                assert!(self.entity_windows.is_empty());
                for (entity, window) in self.scene.entities.query_mut::<&EntityWindow>() {
                    self.entity_windows.push((entity, *window));
                }

                for (entity, window) in &self.entity_windows {
                    EntityPropertiesWindow::new(
                        egui::Id::new("entity_properties").with(entity),
                        &mut self.scene,
                        *entity,
                    )
                    .deletable(window.despawn_button)
                    .show(ctx, entity::default_title);
                }

                self.scene.apply_deferred();

                self.entity_windows.clear();
            }

            self.solver_config_window
                .show(ctx, &mut self.solver_configs);
        });
    }

    pub fn context_menu(&mut self, response: &egui::Response) {
        // todo: make this context menu work for the tree

        if response.secondary_clicked() {
            // todo: if the clicked entity is in the selection, we might want to have the
            // context menu be about the whole selection

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
                let _ = self
                    .scene
                    .entities
                    .insert_one(entity, EntityWindow::default());
            }
        });

        if response.is_none() {
            self.context_menu_object = None;
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
            scene: &mut self.scene,
            outline: &self.config.views.selection_outline,
        }
    }

    /// TODO: Eventually we want a way to select which camera
    pub fn camera_mut(&mut self) -> CameraMut<'_> {
        CameraMut {
            scene: &mut self.scene,
            camera_entity: self.camera_entity,
        }
    }

    pub fn open_camera_window(&mut self) {
        let _ = self
            .scene
            .entities
            .insert_one(self.camera_entity, EntityWindow::default());
    }

    pub fn open_solver_config_window(&mut self) {
        self.solver_config_window.open();
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

                if let Some(taken_entity) = self.scene.take(entity) {
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

// note: moved here, because I keep going between the source/observer configs
// here and the solver config. this way I can edit the test setup here.
fn make_config(name: &str, parallelization: Option<Parallelization>) -> SolverConfig {
    SolverConfig {
        label: format!("Test FDTD ({name})"),
        common: SolverConfigCommon {
            volume: Volume::Fixed(FixedVolume {
                isometry: Isometry3::identity(),
                half_extents: Vector3::new(0.5, 0.5, 0.0),
            }),
            physical_constants: PhysicalConstants::REDUCED,
            default_material: Material {
                // intoduce dissipation
                eletrical_conductivity: 10.0,
                ..Material::VACUUM
            },
            parallelization,
            memory_limit: Some(200_000_000),
        },
        specifics: SolverConfigSpecifics::Fdtd(SolverConfigFdtd {
            resolution: fdtd::Resolution {
                spatial: Vector3::repeat(0.01),
                temporal: 0.001,
            },
            stop_condition: StopCondition::Never,
        }),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExampleScene;

impl PopulateScene for ExampleScene {
    type Error = Infallible;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error> {
        // device

        let em_material = Material {
            relative_permittivity: 3.9,
            ..Material::VACUUM
        };

        let cube = scene
            .add_object(
                Point3::new(-0.2, 0.0, 0.0),
                Cuboid::new(Vector3::repeat(0.1)),
            )
            .material(material::presets::BRASS)
            .component(em_material)
            .spawn(scene);

        let ball = scene
            .add_object(Point3::new(0.4, 0.0, 0.0), Ball::new(0.1))
            .material(material::presets::BLACKBOARD)
            .component(em_material)
            .spawn(scene);
        scene.entities.attach::<()>(ball, cube).unwrap();

        // pml (wip)

        {
            let cuboid = Cuboid::new(Vector3::new(0.05, 0.5, 0.5));
            let transform = LocalTransform::from(Point3::new(-0.45, 0.0, 0.0));
            let normal = Vector3::x_axis();
            let pml = GradedPml {
                m: 4.0,
                m_a: 3.0,
                sigma_max: 2.5,
                kappa_max: 2.5,
                a_max: 0.1,
                normal,
            };
            scene.entities.spawn((
                Label::new_static("PML"),
                pml,
                transform,
                Collider::from(cuboid),
                material::Wireframe::new(palette::named::PURPLE.into_format().with_alpha(1.0)),
                LoadMesh::from_shape(cuboid, ()),
                Selectable,
                ShowInTree,
            ));
        }

        // observer

        {
            let half_extents = Vector2::repeat(0.5);
            let quad = Quad::new(half_extents);
            scene.entities.spawn((
                Label::new_static("Observer"),
                Observer {
                    write_to_gif: None,
                    display_as_texture: true,
                    field: FieldComponent::E,
                    color_map: test_color_map(1.0, Vector3::z_axis()),
                    half_extents,
                },
                material::LoadAlbedoTexture::new("assets/test_pattern.png"),
                material::Material::from(material::presets::OFFICE_PAPER),
                LocalTransform::identity(),
                Collider::from(quad),
                Selectable,
                ShowInTree,
                LoadMesh::from_shape(quad, QuadMeshConfig { back_face: true }),
            ));
        }

        // source

        {
            let shape = Ball::new(0.01);
            scene.entities.spawn((
                Label::new_static("Source"),
                Source::from(
                    //GaussianPulse::new(0.05, 0.01)
                    ContinousWave::new(0.0, 5.0)
                        .with_amplitudes(Vector3::z() * 50.0, Vector3::zeros()),
                ),
                LocalTransform::identity(),
                material::Material::from(material::presets::COPPER),
                Collider::from(shape),
                LoadMesh::from_shape(shape, Default::default()),
                Selectable,
                ShowInTree,
            ));
        }

        Ok(())
    }
}

pub struct PresetScene;

impl PopulateScene for PresetScene {
    type Error = Infallible;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error> {
        let presets = material::presets::ALL;

        let per_line = (presets.len() as f32).sqrt().round() as usize;

        let mut x = 0;
        let mut y = 0;
        for preset in presets {
            scene
                .add_object(Point3::new(x as f32, y as f32, -5.0), Ball::new(0.25))
                .material(**preset)
                .label(preset.name)
                .spawn(scene);

            x += 1;
            if x == per_line {
                x = 0;
                y += 1;
            }
        }

        Ok(())
    }
}

/// Tag for entities that are selected.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Selected;

impl PropertiesUi for Selected {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        ui.noop()
    }
}

impl_register_component!(Selected where ComponentUi, default);

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

pub const CLIPBOARD_PREFIX: &str = "data:application/x-fdtd;base64,";

#[derive(derive_more::Debug)]
pub struct SelectionMut<'a> {
    scene: &'a mut Scene,
    outline: &'a Outline,
}

impl<'a> SelectionMut<'a> {
    pub fn clear(&mut self) {
        for (entity, ()) in self.scene.entities.query_mut::<()>().with::<&Selected>() {
            self.scene.command_buffer.remove_one::<Selected>(entity);
            self.scene.command_buffer.remove_one::<Outline>(entity);
        }
    }

    pub fn select(&mut self, entity: hecs::Entity) {
        if self
            .scene
            .entities
            .satisfies::<&Selectable>(entity)
            .unwrap_or_default()
        {
            self.scene
                .command_buffer
                .insert(entity, (Selected, *self.outline));
        }
    }

    pub fn unselect(&mut self, entity: hecs::Entity) {
        self.scene.command_buffer.remove_one::<Selected>(entity);
        self.scene.command_buffer.remove_one::<Outline>(entity);
    }

    pub fn toggle(&mut self, entity: hecs::Entity) {
        if let Ok(entity_ref) = self.scene.entities.entity(entity) {
            if entity_ref.satisfies::<&Selected>() {
                self.scene.command_buffer.remove_one::<Selected>(entity);
                self.scene.command_buffer.remove_one::<Outline>(entity);
            }
            else if entity_ref.satisfies::<&Selectable>() {
                self.scene
                    .command_buffer
                    .insert(entity, (Selected, *self.outline));
            }
        }
    }

    pub fn select_all(&mut self) {
        // todo: we should add a tag for selectable entities
        for (entity, ()) in self.scene.entities.query_mut::<()>().with::<&Selectable>() {
            self.scene
                .command_buffer
                .insert(entity, (Selected, *self.outline));
        }
    }

    // not great to replicate all these. is there a better way?
    pub fn is_empty(&self) -> bool {
        self.scene
            .entities
            .query::<()>()
            .with::<&Selected>()
            .iter()
            .len()
            == 0
    }

    pub fn query<Q>(&self) -> hecs::QueryBorrow<'_, hecs::With<Q, &Selected>>
    where
        Q: hecs::Query,
    {
        self.scene.entities.query::<Q>().with::<&Selected>()
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

impl<'a> Drop for SelectionMut<'a> {
    fn drop(&mut self) {
        self.scene.apply_deferred();
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Selectable;

impl PropertiesUi for Selectable {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        ui.noop()
    }
}

impl_register_component!(Selectable where ComponentUi, default);

#[derive(derive_more::Debug)]
pub struct CameraMut<'a> {
    scene: &'a mut Scene,
    camera_entity: hecs::Entity,
}

impl<'a> CameraMut<'a> {
    /// Moves the camera such that it fits the whole scene.
    ///
    /// Specifically this only translates the camera. It will be translated (by
    /// moving backwards) such that it will fit the AABB of the scene. The
    /// AABB is calculated relative to the camera orientation. The camera will
    /// also be translated laterally to its view axis to center to the AABB.
    pub fn fit_to_scene(&mut self, margin: &Vector2<f32>) {
        // get camera transform and projection
        // note: we could use another transform if we want to reposition the camera e.g.
        // along a coordinate axis.
        let Ok((camera_transform, camera_projection)) = self
            .scene
            .entities
            .query_one_mut::<(&GlobalTransform, &CameraProjection)>(self.camera_entity)
            .map(|(t, p)| (*t, *p))
        else {
            return;
        };

        // compute scene AABB relative to camera
        let Some(scene_aabb) = self
            .scene
            .compute_aabb_relative_to_observer(camera_transform.isometry(), false)
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
            .query_one_mut::<&mut LocalTransform>(self.camera_entity)
            .expect("camera should still exist");
        camera_transform.translate_local(&Translation3::from(translation));

        self.scene
            .command_buffer
            .insert_one(self.camera_entity, Changed::<LocalTransform>::default());
    }

    /// Fit the camera to the scene looking along a specified axis.
    ///
    /// This is meant to be used along the canonical axis of the scene. It will
    /// not calculate the scene's AABB as viewed along the axis, but instead
    /// just rotate the scene's AABB.
    pub fn fit_to_scene_looking_along_axis(
        &mut self,
        axis: &Vector3<f32>,
        up: &Vector3<f32>,
        margin: &Vector2<f32>,
    ) {
        let scene_aabb = self.scene.aabb();

        let Ok((camera_transform, camera_projection)) =
            self.scene
                .entities
                .query_one_mut::<(&mut LocalTransform, &CameraProjection)>(self.camera_entity)
        else {
            return;
        };

        let rotation = UnitQuaternion::face_towards(axis, up);

        let reference_transform = Isometry3::from_parts(Translation3::identity(), rotation);

        let scene_aabb = scene_aabb.transform_by(&reference_transform);

        let distance = camera_projection.distance_to_fit_aabb_into_fov(&scene_aabb, margin);

        let mut new_local = LocalTransform::from(Isometry3::from_parts(
            Translation3::from(scene_aabb.center().coords),
            rotation,
        ));
        new_local.translate_local(&Translation3::from(-Vector3::z() * distance));

        // FIXME: this doesn't work anymore if the camera has a parent
        *camera_transform = new_local;

        self.scene
            .command_buffer
            .insert_one(self.camera_entity, Changed::<LocalTransform>::default());
    }

    pub fn point_to_scene_center(&mut self) {
        let scene_center = self.scene.aabb().center();

        let Ok(camera_transform) = self
            .scene
            .entities
            .query_one_mut::<&mut LocalTransform>(self.camera_entity)
        else {
            return;
        };

        let eye = camera_transform.position();

        // normally up is always +Y
        let mut up = Vector3::y();

        // but we need to take into account when we're directly above the scene center
        const COLLINEAR_THRESHOLD: f32 = 0.01f32.to_radians();
        if (eye - scene_center).cross(&up).norm_squared() < COLLINEAR_THRESHOLD {
            // we would be looking straight up or down, so keep the up vector from the
            // camera
            up = camera_transform.isometry.rotation.transform_vector(&up);
            tracing::debug!(?eye, ?scene_center, ?up, "looking straight up or down");
        }

        *camera_transform = LocalTransform::look_at(&eye, &scene_center, &up);

        self.scene
            .command_buffer
            .insert_one(self.camera_entity, Changed::<LocalTransform>::default());
    }

    pub fn query<Q>(&mut self) -> Option<Q::Item<'_>>
    where
        Q: hecs::Query,
    {
        self.scene
            .entities
            .query_one_mut::<Q>(self.camera_entity)
            .ok()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EntityWindow {
    pub despawn_button: bool,
}

impl Default for EntityWindow {
    fn default() -> Self {
        Self {
            despawn_button: true,
        }
    }
}

impl DebugUi for Composers {
    fn show_debug(&self, ui: &mut egui::Ui) {
        if !self.composers.is_empty() {
            ui.collapsing("Composers", |ui| {
                for composer in &self.composers {
                    ui.collapsing(&*composer.title, |ui| {
                        if let Some(renderer_info) = composer.scene.resources.get::<RendererInfo>()
                        {
                            renderer_info.show_debug(ui);
                        }

                        composer
                            .selection()
                            .with_query_iter::<Option<&Label>, _>(|selected| {
                                let num_selected = selected.len();
                                if num_selected == 0 {
                                    ui.label("Nothing selected");
                                }
                                else {
                                    ui.label("Selected:");
                                    ui.indent(egui::Id::NULL, |ui| {
                                        for (entity, label) in selected {
                                            ui.label(EntityDebugLabel {
                                                entity,
                                                label: label.cloned(),
                                                invalid: false,
                                            });
                                        }
                                    });
                                }
                            });

                        if let Some(entity_under_pointer) =
                            &composer.scene_pointer.entity_under_pointer
                        {
                            ui.label("Hovered");
                            ui.indent(egui::Id::NULL, |ui| {
                                ui.label(
                                    composer
                                        .scene
                                        .entity_debug_label(entity_under_pointer.entity),
                                );
                                ui.label(format!(
                                    "({:.4}, {:.4}, {:.4})",
                                    entity_under_pointer.point_hovered.x,
                                    entity_under_pointer.point_hovered.y,
                                    entity_under_pointer.point_hovered.z,
                                ));
                                ui.label(format!(
                                    "Distance: {}",
                                    entity_under_pointer.distance_from_camera
                                ));
                            });
                        }
                        else {
                            ui.label("Nothing hovered");
                        }

                        for (entity, info) in
                            composer.scene.entities.query::<&CameraRenderInfo>().iter()
                        {
                            ui.collapsing(format!("Camera {entity:?}"), |ui| {
                                ui.label(format!("Total: {:?}", info.total));
                                ui.label(format!("Opaque: {:?}", info.num_opaque));
                                ui.label(format!("Transparent: {:?}", info.num_transparent));
                                ui.label(format!("Outlines: {:?}", info.num_outlines));
                            });
                        }

                        ui.collapsing("Undo Buffer", |ui| {
                            composer.undo_buffer.show_debug(ui);
                        });

                        composer.scene.show_debug(ui);
                    });
                }
            });
        }
    }
}
