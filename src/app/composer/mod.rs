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
    Isometry3,
    Point3,
    Translation3,
    UnitQuaternion,
    Vector2,
    Vector3,
};
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
        //state.fit_camera_to_scene();
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
    pub fn new() -> Self {
        let mut scene = Scene::default();

        let camera = scene.add_camera(Transform::look_at(
            &Point3::new(0.0, 0.0, -2.0),
            &Point3::origin(),
            &Vector3::y_axis(),
        ));

        Self {
            path: None,
            modified: false,
            scene,
            camera_entity: camera,
            scene_pointer: ScenePointer::default(),
        }
    }

    pub fn new_with_path(path: impl AsRef<Path>) -> Self {
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
    pub fn fit_camera_to_scene(&mut self, margin: &Vector2<f32>) {
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
            let scene_center = self.scene.octtree.center();
            let eye = camera_transform.position();

            // normally up is always +Y
            let mut up = Vector3::y();

            // but we need to take into account when we're directly above the scene center
            const COLLINEAR_THRESHOLD: f32 = 0.01f32.to_radians();
            if (&eye - &scene_center).cross(&up).norm_squared() < COLLINEAR_THRESHOLD {
                // we would be looking straight up or down, so keep the up vector from the
                // camera
                up = camera_transform.transform.rotation.transform_vector(&up);
                tracing::debug!(?eye, ?scene_center, ?up, "looking straight up or down");
            }

            *camera_transform = Transform::look_at(&eye, &scene_center, &up);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExampleScene;

impl PopulateScene for ExampleScene {
    type Error = Infallible;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error> {
        let shape = |size| Cuboid::new(Vector3::repeat(size));
        //let shape = |size| Ball::new(size);

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
