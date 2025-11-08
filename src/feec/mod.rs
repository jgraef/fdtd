use std::{
    f32,
    path::{
        Path,
        PathBuf,
    },
};

use color_eyre::eyre::{
    Error,
    OptionExt,
};
use egui_file_dialog::FileDialog;
use hecs::Entity;
use nalgebra::{
    Matrix3,
    Point2,
    Point3,
    RowVector3,
    Vector2,
    Vector3,
};
use parry3d::shape::Cuboid;

use crate::{
    CreateApp,
    CreateAppContext,
    composer::{
        debug::DebugPanel,
        renderer::Renderer,
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
    geometry::simplex::half_edge::{
        Boundary,
        Coboundary,
        HalfEdgeMesh,
    },
};

#[derive(Debug)]
pub struct App {
    scene: Scene,
    renderer: Renderer,
    camera: Entity,
    scene_pointer: ScenePointer,
    debug: DebugPanel,
    file_dialog: FileDialog,
}

impl App {
    pub fn new(context: CreateAppContext, _args: Args) -> Self {
        context.egui_context.all_styles_mut(|style| {
            style.compact_menu_style = false;
            // this doesn't seem to work :(
            style.spacing.menu_spacing = 0.0;
        });

        let file_dialog = FileDialog::new()
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::default())
            .add_file_filter_extensions("NEC", vec!["nec"]);

        let mut scene = Scene::default();

        let camera = scene.add_camera(Transform::look_at(
            &Point3::new(0.2, 0.2, -2.0),
            &Point3::origin(),
            &Vector3::y(),
        ));

        populate_scene(&mut scene);
        let renderer = Renderer::new(context.wgpu_context.clone());

        Self {
            scene,
            renderer,
            camera,
            scene_pointer: ScenePointer::default(),
            debug: DebugPanel::default(),
            file_dialog,
        }
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            ui.set_min_width(150.0);

            if ui.button("New File").clicked() {
                tracing::debug!("todo: new file");
            }

            ui.separator();

            if ui.button("Open File").clicked() {
                self.file_dialog.set_user_data(FileDialogAction::Open);
                self.file_dialog.pick_file();
            }
            ui.menu_button("Open Recent", |ui| {
                for i in 0..5 {
                    if ui.button(format!("~/placeholder/{i}.foo")).clicked() {
                        tracing::debug!("todo: open recent file");
                    }
                }
            });

            ui.separator();

            if ui.button("Save").clicked() {
                tracing::debug!("todo: save");
            }
            if ui.button("Save As").clicked() {
                self.file_dialog.set_user_data(FileDialogAction::SaveAs);
                self.file_dialog.pick_file();
            }

            ui.separator();

            if ui.button("Preferences").clicked() {
                tracing::debug!("todo: preferences");
            }

            ui.separator();

            if ui.button("Close File").clicked() {
                tracing::debug!("todo: close file");
            }

            ui.separator();

            if ui.button("Exit").clicked() {
                tracing::info!("App close requested by user");
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Edit", |ui| {
            ui.set_min_width(150.0);

            if ui.button("Undo").clicked() {
                tracing::debug!("todo: undo");
            }
            if ui.button("Redo").clicked() {
                tracing::debug!("todo: redo");
            }

            if ui.button("Cut").clicked() {
                tracing::debug!("todo: cut");
            }
            if ui.button("Copy").clicked() {
                tracing::debug!("todo: copy");
            }
            if ui.button("Past").clicked() {
                tracing::debug!("todo: paste");
            }
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.scene.update_octtree();
        self.renderer.prepare_world(&mut self.scene);

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.file_menu(ui);
                self.edit_menu(ui);
                ui.menu_button("Selection", |ui| {
                    todo_label(ui);
                });
                ui.menu_button("View", |ui| {
                    todo_label(ui);
                });
                ui.menu_button("Run", |ui| {
                    todo_label(ui);
                });
                ui.menu_button("Help", |ui| {
                    todo_label(ui);
                });
            });
        });

        self.debug.show(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(entity_under_pointer) = &self.scene_pointer.entity_under_pointer {
                let label = self
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
                SceneView::new(&mut self.scene, &mut self.renderer)
                    .with_camera(self.camera)
                    .with_scene_pointer(&mut self.scene_pointer),
            );
        });

        if let Some(file) = self.file_dialog.update(ctx).picked() {
            tracing::debug!(file = %file.display(), "File picked");
        }
    }
}

fn load_mesh(path: impl AsRef<Path>) -> Result<HalfEdgeMesh<Vertex, Edge, Face>, Error> {
    let (models, _) = tobj::load_obj(path.as_ref(), &Default::default())?;
    let model = models.into_iter().next().ok_or_eyre("no models")?;
    tracing::debug!("loading mesh: {}", model.name);
    assert!(model.mesh.face_arities.is_empty());

    let mesh = HalfEdgeMesh::from_trimesh(
        model
            .mesh
            .indices
            .chunks_exact(3)
            .map(|indices| indices.try_into().unwrap()),
        |index| {
            Vertex {
                position: Point3::new(
                    model.mesh.positions[index as usize * 3],
                    model.mesh.positions[index as usize * 3 + 1],
                    model.mesh.positions[index as usize * 3 + 2],
                ),
            }
        },
        |_| Edge::default(),
        |_| Face::default(),
    );

    Ok(mesh)
}

#[derive(Clone, Debug)]
struct Vertex {
    position: Point3<f32>,
}

#[derive(Clone, Debug, Default)]
struct Edge {
    electric_field: RowVector3<f64>,
    epsilon_inv: Matrix3<f64>,
}

#[derive(Clone, Debug, Default)]
struct Face {
    magnetic_field: RowVector3<f64>,
    mu_inv: Matrix3<f64>,
}

pub struct Simulation {
    mesh: HalfEdgeMesh<Vertex, Edge, Face>,
    dt: f64,
}

impl Simulation {
    pub fn step(&mut self) {
        // update magnetic field
        for face in self.mesh.faces() {
            let de = self
                .mesh
                .boundary(face)
                .map(|edge| self.mesh[edge].electric_field)
                .sum::<RowVector3<f64>>();
            let face = &mut self.mesh[face];
            face.magnetic_field += -self.dt * de * face.mu_inv;
        }

        // update electric field
        for edge in self.mesh.edges() {
            let he = self
                .mesh
                .coboundary(edge)
                .map(|face| self.mesh[face].magnetic_field)
                .sum::<RowVector3<f64>>();
            let edge = &mut self.mesh[edge];

            // todo: this is sigma * e plus some source current density
            let j = RowVector3::zeros();
            edge.electric_field += self.dt * (he - j) * edge.epsilon_inv;
        }
    }
}

fn generate_test_mesh() -> HalfEdgeMesh<Vertex, Edge, Face> {
    let size = Vector2::new(100, 100);

    let xy_to_index = |x, y| y * size.x + x;
    let index_to_xy = |i| Point2::new(i % size.x, i / size.x);

    let faces = (0..size.x).flat_map(|x| {
        (0..size.y).flat_map(move |y| {
            let v = [
                xy_to_index(x, y),
                xy_to_index(x + 1, y),
                xy_to_index(x, y + 1),
                xy_to_index(x + 1, y + 1),
            ];

            [[v[0], v[1], v[2]], [v[1], v[3], v[2]]]
        })
    });

    let epsilon_inv = Matrix3::identity();
    let mu_inv = Matrix3::identity();

    HalfEdgeMesh::from_trimesh(
        faces,
        |i| {
            let r = index_to_xy(i);
            Vertex {
                position: Point3::new(r.x as f32, r.y as f32, 0.0),
            }
        },
        |i| {
            let _r = i.map(index_to_xy);

            Edge {
                electric_field: Default::default(),
                epsilon_inv,
            }
        },
        |i| {
            let _r = i.map(index_to_xy);
            Face {
                magnetic_field: Default::default(),
                mu_inv,
            }
        },
    )
}

fn populate_scene(scene: &mut Scene) {
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
}

#[derive(Debug, clap::Parser)]
pub struct Args {
    file: Option<PathBuf>,
}

impl CreateApp for Args {
    type App = App;

    fn create_app(self, context: CreateAppContext) -> Self::App {
        App::new(context, self)
    }
}

fn todo_label(ui: &mut egui::Ui) {
    ui.label("todo");
}

#[derive(Clone, Copy, Debug)]
enum FileDialogAction {
    Open,
    SaveAs,
}
