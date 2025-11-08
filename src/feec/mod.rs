use std::path::Path;

use color_eyre::eyre::{
    Error,
    OptionExt,
};
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
    CreateAppContext,
    composer::{
        renderer::Renderer,
        scene::{
            Scene,
            Transform,
        },
        view::{
            ScenePointer,
            SceneView,
        },
    },
    geometry::simplex::half_edge::{
        Boundary,
        Coboundary,
        HalfEdgeMesh,
    },
};

#[derive(Debug)]
pub struct FeecApp {
    context: CreateAppContext,
    scene: Scene,
    renderer: Renderer,
    camera: Entity,
    scene_pointer: ScenePointer,
    debug: bool,
}

impl FeecApp {
    pub fn new(context: CreateAppContext) -> Self {
        let mut scene = Scene::default();

        let camera = scene.add_camera(Transform::look_at(
            &Point3::new(0.2, 0.2, -2.0),
            &Point3::origin(),
            &Vector3::y(),
        ));

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

        let renderer = Renderer::new(context.wgpu_context.clone());

        Self {
            context,
            scene,
            renderer,
            camera,
            scene_pointer: ScenePointer::default(),
            debug: false,
        }
    }
}

impl eframe::App for FeecApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.scene.update_octtree();
        self.renderer.prepare_world(&mut self.scene);

        ctx.input(|input| {
            self.debug ^= input.key_pressed(egui::Key::F5);
        });

        if self.debug {
            egui::SidePanel::left("debug").show(ctx, |ui| {
                ctx.inspection_ui(ui);
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(entity_under_pointer) = &self.scene_pointer.entity_under_pointer {
                ui.label(format!(
                    "Hovered: {:?} at ({}, {}, {}) with {} distance",
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
