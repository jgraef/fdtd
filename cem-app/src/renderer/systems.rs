use bevy_ecs::{
    entity::Entity,
    message::{
        Message,
        MessageReader,
    },
    query::{
        Changed,
        Or,
        QueryData,
        With,
        Without,
    },
    system::{
        Commands,
        EntityCommands,
        Query,
        Res,
        ResMut,
    },
};
use cem_scene::{
    Label,
    transform::GlobalTransform,
};
use cem_util::wgpu::buffer::{
    WriteStagingCommit,
    WriteStagingTransaction,
};

use crate::renderer::{
    camera::{
        CameraBindGroup,
        CameraConfig,
        CameraData,
        CameraProjection,
        Viewport,
    },
    components::{
        ClearColor,
        Hidden,
        Outline,
    },
    light::{
        AmbientLight,
        PointLight,
    },
    material::{
        AlbedoTexture,
        Material,
        MaterialTexture,
        Wireframe,
    },
    mesh::{
        Mesh,
        MeshBindGroup,
        MeshFlags,
    },
    renderer::{
        Renderer,
        SharedRenderer,
    },
    state::{
        InstanceData,
        RendererState,
    },
};

fn start_frame(renderer: Res<SharedRenderer>, mut state: ResMut<RendererState>) {
    assert!(state.write_staging.is_none());

    let mut command_encoder =
        renderer
            .wgpu_context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render/prepare_world"),
            });

    let mut write_staging = WriteStagingTransaction::new(
        renderer.wgpu_context.staging_pool.belt(),
        renderer.wgpu_context.device.clone(),
        command_encoder,
    );

    state.write_staging = Some(write_staging);
}

fn finish_frame(renderer: Res<SharedRenderer>, mut state: ResMut<RendererState>) {
    // finish all staged writes
    let command_encoder = state.write_staging.take().unwrap().commit();
    renderer
        .wgpu_context
        .queue
        .submit([command_encoder.finish()]);
}

#[derive(QueryData)]
struct UpdateInstanceBufferAndDrawCommandQueryData {
    label: Option<&'static Label>,
    global_transform: &'static GlobalTransform,
    mesh: &'static Mesh,
    mesh_bind_group: &'static MeshBindGroup,
    material: Option<&'static Material>,
    wireframe: Option<&'static Wireframe>,
    albedo_texture: Option<&'static AlbedoTexture>,
    material_texture: Option<&'static MaterialTexture>,
    outline: Option<&'static Outline>,
}

fn update_instance_buffer_and_draw_command(
    mut query: Query<
        UpdateInstanceBufferAndDrawCommandQueryData,
        (
            Or<(
                With<Material>,
                With<Wireframe>,
                With<AlbedoTexture>,
                With<MaterialTexture>,
            )>,
            Without<Hidden>,
        ),
    >,
    mut state: ResMut<RendererState>,
) {
    // for now every draw call will only draw one instance, but we could do
    // instancing for real later.
    let mut first_instance = 0;
    let mut next_instances = || {
        let instances = first_instance..(first_instance + 1);
        first_instance += 1;
        instances
    };

    let state = &mut *state;
    let write_staging = state.write_staging.as_mut().unwrap();

    // this is the second time I have a bug with this. keep this assert!
    assert!(
        state.instance_buffer.host_staging.is_empty(),
        "instance scratch buffer hasn't been cleared yet"
    );

    // prepare the actual draw commands
    let mut draw_command_builder = state.draw_command_buffer.builder();

    query.iter().for_each(|item| {
        let has_material = item.material.is_some()
            || item.albedo_texture.is_some()
            || item.material_texture.is_some();
        let has_wireframe = item.wireframe.is_some();

        // write per-instance data into a buffer
        state.instance_buffer.push(InstanceData::new_mesh(
            item.global_transform,
            item.mesh,
            item.material,
            item.wireframe,
            item.albedo_texture,
            item.material_texture,
            item.outline,
        ));

        let instances = next_instances();

        if has_material {
            // if it is transparent we need to remember its position to later sort by
            // distance from camera.
            let transparent = item
                .material
                .is_some_and(|material| material.transparent)
                .then(|| item.global_transform.position());

            draw_command_builder.draw_mesh(
                instances.clone(),
                item.mesh,
                item.mesh_bind_group,
                transparent,
                item.outline.is_some(),
            );
        }
        if item.outline.is_some() {
            draw_command_builder.draw_outline(instances.clone(), item.mesh, item.mesh_bind_group);
        }
        if has_wireframe {
            draw_command_builder.draw_wireframe(instances.clone(), item.mesh, item.mesh_bind_group);
        }
    });

    // send instance data to gpu
    state.instance_buffer_reallocated = state.instance_buffer.flush(|_buffer| {}, write_staging);
}

#[derive(Debug, Message)]
pub(super) enum UpdateMeshBindGroupMessage {
    MeshAdded { entity: Entity },
    MeshRemoved { entity: Entity },
}

#[derive(QueryData)]
struct UpdateMeshBindGroupsQueryData {
    label: Option<&'static Label>,
    mesh: &'static Mesh,
    albedo_texture: Option<&'static AlbedoTexture>,
    material_texture: Option<&'static MaterialTexture>,
}

fn update_mesh_bind_groups(
    renderer: Res<SharedRenderer>,
    mut query: Query<UpdateMeshBindGroupsQueryData>,
    mut messages: MessageReader<UpdateMeshBindGroupMessage>,
    mut commands: Commands,
) {
    messages.read().for_each(|message| {
        match message {
            UpdateMeshBindGroupMessage::MeshAdded { entity } => {
                let item = query.get(*entity).unwrap();

                let entity_commands = commands.entity(*entity);

                update_mesh_bind_group(
                    &*renderer,
                    entity_commands,
                    item.mesh,
                    item.albedo_texture,
                    item.material_texture,
                    item.label,
                );
            }
            UpdateMeshBindGroupMessage::MeshRemoved { entity } => {
                commands.entity(*entity).remove::<MeshBindGroup>();
            }
        }
    });
}

fn update_mesh_bind_group(
    renderer: &Renderer,
    mut entity_commands: EntityCommands,
    mesh: &Mesh,
    albedo_texture: Option<&AlbedoTexture>,
    material_texture: Option<&MaterialTexture>,
    label: Option<&Label>,
) {
    if !mesh.flags.contains(MeshFlags::UVS)
        && (albedo_texture.is_some() || material_texture.is_some())
    {
        tracing::warn!(?label, "Mesh with textures, but no UV buffer");
    }

    let mesh_bind_group = MeshBindGroup::new(
        &renderer.wgpu_context.device,
        &renderer.mesh_bind_group_layout,
        mesh,
        albedo_texture,
        material_texture,
        &renderer.fallbacks,
    );

    entity_commands.insert(mesh_bind_group);
}

fn update_camera_viewports(
    mut changed_viewports: Query<(&mut CameraProjection, &Viewport), Changed<Viewport>>,
) {
    changed_viewports
        .iter_mut()
        .for_each(|(mut camera_projection, viewport)| {
            camera_projection.set_viewport(viewport);
        });
}

#[derive(QueryData)]
struct CreateCameraBindGroupsQueryData {
    entity: Entity,
    camera_projection: &'static CameraProjection,
    global_transform: &'static GlobalTransform,
    clear_color: Option<&'static ClearColor>,
    ambient_light: Option<&'static AmbientLight>,
    point_light: Option<&'static PointLight>,
    camera_config: Option<&'static CameraConfig>,
}

fn create_camera_bind_groups(
    renderer: Res<SharedRenderer>,
    state: Res<RendererState>,
    query: Query<CreateCameraBindGroupsQueryData, Without<CameraBindGroup>>,
    mut commands: Commands,
) {
    query.iter().for_each(
        |CreateCameraBindGroupsQueryDataItem {
             entity,
             camera_projection,
             global_transform,
             clear_color,
             ambient_light,
             point_light,
             camera_config,
         }| {
            tracing::debug!(
                ?entity,
                ?camera_projection,
                ?global_transform,
                ?clear_color,
                ?ambient_light,
                ?point_light,
                "creating camera"
            );
            let camera_data = CameraData::new(
                camera_projection,
                global_transform,
                clear_color,
                ambient_light,
                point_light,
                camera_config,
            );
            let camera_resources = CameraBindGroup::new(
                &renderer.camera_bind_group_layout,
                &renderer.wgpu_context.device,
                &camera_data,
                state.instance_buffer.buffer.buffer().unwrap(),
            );
            commands.entity(entity).insert(camera_resources);
        },
    )
}

fn destroy_camera_bind_groups(
    query: Query<
        Entity,
        (
            With<CameraBindGroup>,
            Or<(Without<GlobalTransform>, Without<CameraProjection>)>,
        ),
    >,
    mut commands: Commands,
) {
    query.iter().for_each(|entity| {
        commands.entity(entity).remove::<CameraBindGroup>();
    });
}

#[derive(QueryData)]
#[query_data(mutable)]
struct UpdateCameraBindGroupsQueryData {
    camera_bind_group: &'static mut CameraBindGroup,
    camera_projection: &'static CameraProjection,
    global_transform: &'static GlobalTransform,
    clear_color: Option<&'static ClearColor>,
    ambient_light: Option<&'static AmbientLight>,
    point_light: Option<&'static PointLight>,
    camera_config: Option<&'static CameraConfig>,
}

fn update_camera_bind_groups(
    renderer: Res<SharedRenderer>,
    mut state: ResMut<RendererState>,
    mut query: Query<UpdateCameraBindGroupsQueryData>,
) {
    let state = &mut *state;

    // todo: changed filter
    let updated_instance_buffer = state.instance_buffer_reallocated.then_some((
        &renderer.camera_bind_group_layout,
        state.instance_buffer.buffer.buffer().unwrap(),
    ));

    let mut write_staging = state.write_staging.as_mut().unwrap();

    query.iter_mut().for_each(
        |UpdateCameraBindGroupsQueryDataItem {
             mut camera_bind_group,
             camera_projection,
             global_transform,
             clear_color,
             ambient_light,
             point_light,
             camera_config,
         }| {
            let camera_data = CameraData::new(
                camera_projection,
                global_transform,
                clear_color,
                ambient_light,
                point_light,
                camera_config,
            );
            camera_bind_group.update(
                &renderer.wgpu_context.device,
                &mut write_staging,
                &camera_data,
                updated_instance_buffer,
            );
        },
    );
}
