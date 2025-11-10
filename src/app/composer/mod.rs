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
    Translation3,
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
            camera::CameraProjection,
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

    /// Useful for actions from UI elements outside the compose (e.g. the menu
    /// bar), and we already checked that a file is open (e.g. for disabling a
    /// button).
    pub fn expect_state_mut(&mut self) -> &mut State {
        self.state.as_mut().expect("no file open")
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
                            .with_camera(state.camera_entity)
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
pub struct State {
    path: Option<PathBuf>,
    modified: bool,
    scene: Scene,
    camera_entity: hecs::Entity,
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
            camera_entity: camera,
            scene_pointer: ScenePointer::default(),
        }
    }

    fn new_with_path(path: impl AsRef<Path>) -> Self {
        let mut this = Self::new();
        this.path = Some(path.as_ref().to_owned());
        this
    }

    /// Moves the camera such that it fits the whole scene.
    ///
    /// Specifically this only translates the camera. It will be translated (by
    /// moving backwards) such that it will fit the AABB of the scene. The
    /// AABB is calculated relative to the camera orientation. The camera will
    /// also be translated laterally to its view axis to center to the AABB.
    pub fn fit_camera(&mut self) {
        // get camera transform and projection
        // note: we could use another transform if we want to reposition the camera e.g.
        // along a coordinate axis.
        let Ok((camera_transform, camera_projection)) = self
            .scene
            .entities
            .query_one_mut::<(&Transform, &CameraProjection)>(self.camera_entity)
            .map(|(t, p)| (t.clone(), p.clone()))
        else {
            return;
        };

        // compute scene AABB relative to camera
        let Some(scene_aabb) = self
            .scene
            .compute_aabb_relative_to_observer(&camera_transform)
        else {
            return;
        };

        let scene_aabb_half_extents = scene_aabb.half_extents();

        // center camera on aabb
        let mut translation = scene_aabb.center().coords;

        // camera projection parameters
        let half_fovy = 0.5 * camera_projection.fovy();
        let aspect_ratio = camera_projection.aspect_ratio();
        let half_fovx = half_fovy / aspect_ratio;

        // how far back do we have to be from the face of the AABB to fit the vertical
        // FOV of the camera? simple geometry tells us that tan(fovy/2) = y/z,
        // where y is the half-extend of the AABB in y-direction.
        let dz_vertical = scene_aabb_half_extents.y / half_fovy.tan();

        // same for horizontal fit
        let dz_horizontal = scene_aabb_half_extents.x / half_fovx.tan();

        // we want to fit both, so we take the max. we also need to add the distance
        // from the center of the AABB to its face along the z-axis.
        translation.z -= scene_aabb_half_extents.z + dz_vertical.max(dz_horizontal);

        // apply translation to camera
        let camera_transform = self
            .scene
            .entities
            .query_one_mut::<&mut Transform>(self.camera_entity)
            .expect("camera should still exist");
        camera_transform.translate_local(&Translation3::from(translation));
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
