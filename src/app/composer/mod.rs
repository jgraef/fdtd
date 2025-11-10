pub mod renderer;
pub mod scene;

use std::{
    convert::Infallible,
    fs::File,
    io::BufReader,
    path::{
        Path,
        PathBuf,
    },
};

use color_eyre::eyre::bail;
use egui::{
    RichText,
    Widget,
    epaint::MarginF32,
};
use nalgebra::{
    Point3,
    Vector3,
};
use palette::WithAlpha;
use parry3d::shape::Cuboid;

use crate::{
    Error,
    app::composer::{
        renderer::{
            Renderer,
            WgpuContext,
        },
        scene::{
            Label,
            PopulateScene,
            Scene,
            Transform,
            view::{
                ScenePointer,
                SceneView,
            },
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

#[derive(Debug)]
pub struct Composer {
    state: Option<State>,
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

    pub fn new_file(&mut self) {
        let mut state = State::new();
        ExampleScene
            .populate_scene(&mut state.scene)
            .expect("populating example scene failed");
        self.state = Some(state);
    }

    pub fn open_file(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        tracing::debug!(path = %path.display(), "open file");

        if let Some(file_format) = guess_file_format_from_path(path) {
            let mut state = State::new_with_path(path);

            #[allow(unreachable_patterns)]
            match file_format {
                FileFormat::Nec => {
                    let reader = BufReader::new(File::open(path)?);
                    let nec_file = NecFile::from_reader(reader)?;
                    tracing::debug!("{nec_file:#?}");
                    PopulateWithNec {
                        nec_file: &nec_file,
                        color: palette::named::ORANGERED.into_format().with_alpha(1.0),
                    }
                    .populate_scene(&mut state.scene)?;
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

    pub fn has_open_file(&self) -> bool {
        self.state.is_some()
    }

    pub fn close_file(&mut self) {
        self.state = None;
    }
}

impl Widget for &mut Composer {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        egui::Frame::new()
            .show(ui, |ui| {
                if let Some(state) = &mut self.state {
                    state.scene.update_octtree();
                    self.renderer.prepare_world(&mut state.scene);

                    if let Some(entity_under_pointer) = &state.scene_pointer.entity_under_pointer {
                        let label = state
                            .scene
                            .entities
                            .query_one_mut::<Option<&Label>>(entity_under_pointer.entity)
                            .ok()
                            .flatten()
                            .map(|label| format!(" {label}"))
                            .unwrap_or_default();

                        ui.label(format!(
                            "Hovered: {:?}{label} at ({}, {}, {}) with {} distance",
                            entity_under_pointer.entity,
                            entity_under_pointer.point_hovered.x,
                            entity_under_pointer.point_hovered.y,
                            entity_under_pointer.point_hovered.z,
                            entity_under_pointer.distance_from_camera
                        ));
                    }
                    else {
                        ui.label("Nothing hovered");
                    }

                    ui.add(
                        SceneView::new(&mut state.scene, &mut self.renderer)
                            .with_camera(state.camera)
                            .with_scene_pointer(&mut state.scene_pointer),
                    );
                }
                else {
                    egui::Frame::new()
                        .inner_margin(MarginF32::symmetric(
                            ui.available_width() / 2.0,
                            ui.available_width() / 2.0,
                        ))
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new("Welcome!").heading());
                                ui.label(lipsum!(20));
                            });
                        });
                }
            })
            .response
    }
}

#[derive(Debug)]
struct State {
    path: Option<PathBuf>,
    modified: bool,
    scene: Scene,
    camera: hecs::Entity,
    scene_pointer: ScenePointer,
}

impl State {
    fn new() -> Self {
        let mut scene = Scene::default();

        let camera = scene.add_camera(Transform::look_at(
            &Point3::new(0.0, 0.0, -2.0),
            &Point3::origin(),
            &Vector3::y(),
        ));

        Self {
            path: None,
            modified: false,
            scene,
            camera,
            scene_pointer: ScenePointer::default(),
        }
    }

    fn new_with_path(path: impl AsRef<Path>) -> Self {
        let mut this = Self::new();
        this.path = Some(path.as_ref().to_owned());
        this
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExampleScene;

impl PopulateScene for ExampleScene {
    type Error = Infallible;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error> {
        let shape = |size| Cuboid::new(Vector3::repeat(size));

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
