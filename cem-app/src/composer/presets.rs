use std::convert::Infallible;

use bevy_ecs::name::Name;
use cem_render::{
    material as render_material,
    mesh::{
        IntoGenerateMesh,
        LoadMesh,
    },
    texture::{
        Sampler,
        TextureSource,
    },
};
use cem_scene::{
    PopulateScene,
    Scene,
    spatial::Collider,
    transform::LocalTransform,
};
use cem_solver::{
    FieldComponent,
    fdtd::pml::GradedPml,
    material::Material as PhysicsMaterial,
    source::{
        ContinousWave,
        ScalarSourceFunctionExt,
        Source,
    },
};
use cem_util::wgpu::MipLevels;
use nalgebra::{
    Point3,
    UnitQuaternion,
    Vector2,
    Vector3,
};
use palette::WithAlpha;
use parry3d::shape::{
    Ball,
    Cuboid,
};

use crate::{
    composer::{
        selection::Selectable,
        shape::flat::{
            HalfSpace,
            Quad,
            QuadMeshConfig,
        },
        tree::ShowInTree,
    },
    solver::observer::{
        Observer,
        test_color_map,
    },
    util::scene::{
        EntityBuilderExt,
        SceneExt,
    },
};

#[derive(Clone, Copy, Debug)]
pub struct ExampleScene;

impl PopulateScene for ExampleScene {
    type Error = Infallible;

    fn populate_scene(&self, scene: &mut Scene) -> Result<(), Self::Error> {
        // device

        let em_material = PhysicsMaterial {
            relative_permittivity: 3.9,
            ..PhysicsMaterial::VACUUM
        };

        let cube = scene
            .add_object(
                Point3::new(-0.2, 0.5, 0.0),
                Cuboid::new(Vector3::repeat(0.1)),
            )
            .material(render_material::presets::BRASS)
            .insert(em_material)
            .id();

        let ball = scene
            .add_object(Point3::new(0.4, 0.0, 0.0), Ball::new(0.1))
            .material(render_material::presets::BLACKBOARD)
            .insert(em_material)
            .id();

        scene.world.entity_mut(cube).add_child(ball);

        // plane
        scene.world.spawn((
            LocalTransform::new(
                Point3::origin(),
                UnitQuaternion::face_towards(&Vector3::y(), &Vector3::z()),
            ),
            //render_material::Material::from_albedo(
            //    palette::named::CHARTREUSE.into_format().with_alpha(1.0),
            //),
            render_material::LoadAlbedoTexture::new(TextureSource::from_path_with_mip_levels(
                // good options:
                //"tmp/kenney_prototype-textures/PNG/Dark/texture_04.png",
                //"tmp/kenney_prototype-textures/PNG/Light/texture_08.png",
                "tmp/kenney_prototype-textures/PNG/Light/texture_03.png",
                //"tmp/kenney_prototype-textures/PNG/Green/texture_05.png",
                //"tmp/kenney_prototype-textures/PNG/Green/texture_10.png",
                MipLevels::Auto {
                    filter: image::imageops::FilterType::CatmullRom,
                },
            ))
            .with_sampler(Sampler::Repeat),
            LoadMesh::from_generator(HalfSpace.into_generate_mesh(()).unwrap()),
            Collider::from(HalfSpace),
            Name::new("Ground"),
        ));

        // pml (wip)

        {
            let cuboid = Cuboid::new(Vector3::new(0.05, 0.5, 0.5));
            let transform = LocalTransform::from(Point3::new(-0.45, 0.5, 0.0));
            let normal = Vector3::x_axis();
            let pml = GradedPml {
                m: 4.0,
                m_a: 3.0,
                sigma_max: 2.5,
                kappa_max: 2.5,
                a_max: 0.1,
                normal,
            };
            scene.world.spawn((
                Name::new("PML"),
                pml,
                transform,
                Collider::from(cuboid),
                render_material::Wireframe::new(
                    palette::named::PURPLE.into_format().with_alpha(1.0),
                ),
                LoadMesh::from_shape(cuboid, ()),
                Selectable,
                ShowInTree,
            ));
        }

        // observer

        {
            let half_extents = Vector2::repeat(0.5);
            let quad = Quad::new(half_extents);
            scene.world.spawn((
                Name::new("Observer"),
                Observer {
                    write_to_gif: None,
                    display_as_texture: true,
                    field: FieldComponent::E,
                    color_map: test_color_map(1.0, Vector3::z_axis()),
                    half_extents,
                },
                render_material::LoadAlbedoTexture::new("assets/test_pattern.png"),
                render_material::Material::from(render_material::presets::OFFICE_PAPER),
                LocalTransform::from(Point3::new(0.0, 0.5, 0.0)),
                Collider::from(quad),
                Selectable,
                ShowInTree,
                LoadMesh::from_shape(quad, QuadMeshConfig { back_face: true }),
            ));
        }

        // source

        {
            let shape = Ball::new(0.01);
            scene.world.spawn((
                Name::new("Source"),
                Source::from(
                    //GaussianPulse::new(0.05, 0.01)
                    ContinousWave::new(0.0, 5.0)
                        .with_amplitudes(Vector3::z() * 50.0, Vector3::zeros()),
                ),
                LocalTransform::from(Point3::new(0.0, 0.5, 0.0)),
                render_material::Material::from(render_material::presets::COPPER),
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
        let presets = render_material::presets::ALL;

        let per_line = (presets.len() as f32).sqrt().round() as usize;

        let mut x = 0;
        let mut y = 0;
        for preset in presets {
            scene
                .add_object(Point3::new(x as f32, y as f32, -5.0), Ball::new(0.25))
                .material(**preset)
                .name(preset.name);

            x += 1;
            if x == per_line {
                x = 0;
                y += 1;
            }
        }

        Ok(())
    }
}
