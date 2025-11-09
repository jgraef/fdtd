use std::{
    fs::File,
    io::BufReader,
    path::Path,
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
use parry3d::shape::Cuboid;

use crate::{
    Error,
    composer::{
        renderer::{
            Renderer,
            WgpuContext,
        },
        scene::{
            Label,
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
        nec::NecFile,
    },
    lipsum,
};

pub mod renderer;
pub mod scene;

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
        state.populate_scene();
        self.state = Some(state);
    }

    pub fn open_file(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        tracing::debug!(path = %path.display(), "open file");

        if let Some(file_format) = guess_file_format_from_path(path) {
            #[allow(unreachable_patterns)]
            match file_format {
                FileFormat::Nec => {
                    let reader = BufReader::new(File::open(path)?);
                    let nec = NecFile::from_reader(reader)?;
                    tracing::debug!("{nec:#?}");
                }
                _ => bail!("Unsupported file format: {file_format:?}"),
            }
        }
        else {
            tracing::debug!("todo: unknown file format");
        }

        Ok(())
    }

    pub fn has_open_file(&self) -> bool {
        self.state.is_some()
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
    scene: Scene,
    camera: hecs::Entity,
    scene_pointer: ScenePointer,
}

impl State {
    pub fn new() -> Self {
        let mut scene = Scene::default();

        let camera = scene.add_camera(Transform::look_at(
            &Point3::new(0.2, 0.2, -2.0),
            &Point3::origin(),
            &Vector3::y(),
        ));

        Self {
            scene,
            camera,
            scene_pointer: ScenePointer::default(),
        }
    }

    fn populate_scene(&mut self) {
        let shape = |size| Cuboid::new(Vector3::repeat(size));

        self.scene
            .add_object(Point3::new(-0.2, 0.0, 0.0), shape(0.1), palette::named::RED);
        self.scene
            .add_object(Point3::new(0.2, 0.0, 0.0), shape(0.1), palette::named::BLUE);
        self.scene.add_object(
            Point3::new(0.0, -0.2, 0.0),
            shape(0.1),
            palette::named::LIME,
        );
        self.scene.add_object(
            Point3::new(0.0, 0.2, 0.0),
            shape(0.1),
            palette::named::YELLOW,
        );
        self.scene.add_object(
            Point3::new(-0.02, -0.02, 0.2),
            shape(0.05),
            palette::named::MAGENTA,
        );
        self.scene.add_object(
            Point3::new(0.02, 0.02, -0.2),
            shape(0.05),
            palette::named::CYAN,
        );
    }
}
