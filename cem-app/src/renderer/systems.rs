use bevy_ecs::{
    entity::{
        Entity,
        EntityHashSet,
    },
    message::{
        Message,
        MessageReader,
    },
    name::{
        NameOrEntity,
        NameOrEntityItem,
    },
    query::{
        Changed,
        Has,
        Or,
        QueryData,
        With,
        Without,
    },
    system::{
        Commands,
        EntityCommands,
        In,
        Local,
        Query,
        Res,
        ResMut,
    },
};
use cem_scene::transform::GlobalTransform;
use cem_util::wgpu::{
    ImageTextureExt,
    buffer::{
        WriteStagingCommit,
        WriteStagingTransaction,
    },
};

use crate::renderer::{
    Command,
    camera::{
        CameraBindGroup,
        CameraConfig,
        CameraData,
        CameraProjection,
        Viewport,
    },
    command::{
        CommandReceiver,
        CommandSender,
    },
    components::{
        ClearColor,
        Hidden,
        Outline,
    },
    draw_commands::{
        DrawCommand,
        DrawCommandFlags,
        DrawCommandInfoSink,
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
    resource::RenderResourceTransactionState,
    state::{
        InstanceData,
        RendererState,
    },
};

pub fn begin_frame(renderer: Res<SharedRenderer>, mut state: ResMut<RendererState>) {
    assert!(state.write_staging.is_none());

    let command_encoder =
        renderer
            .wgpu_context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render/prepare_world"),
            });

    let write_staging = WriteStagingTransaction::new(
        renderer.wgpu_context.staging_pool.belt(),
        renderer.wgpu_context.device.clone(),
        command_encoder,
    );

    state.write_staging = Some(write_staging);
}

pub fn end_frame(renderer: Res<SharedRenderer>, mut state: ResMut<RendererState>) {
    // finish all staged writes
    let command_encoder = state.write_staging.take().unwrap().commit();
    renderer
        .wgpu_context
        .queue
        .submit([command_encoder.finish()]);
}

#[derive(QueryData)]
pub struct UpdateInstanceBufferAndDrawCommandQueryData {
    global_transform: &'static GlobalTransform,
    mesh: &'static Mesh,
    mesh_bind_group: &'static MeshBindGroup,
    material: Option<&'static Material>,
    wireframe: Option<&'static Wireframe>,
    albedo_texture: Option<&'static AlbedoTexture>,
    material_texture: Option<&'static MaterialTexture>,
    outline: Option<&'static Outline>,
}

pub fn update_instance_buffer_and_draw_command(
    query: Query<
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
    // todo: pass `instance_buffer_reallocated` outside of renderer state.
    state.instance_buffer_reallocated = state.instance_buffer.flush(|_buffer| {}, write_staging);
}

#[derive(Debug, Message)]
pub enum UpdateMeshBindGroupMessage {
    MeshAdded { entity: Entity },
    MeshRemoved { entity: Entity },
    AlbedoTextureAdded { entity: Entity },
    AlbedoTextureRemoved { entity: Entity },
    MaterialTextureAdded { entity: Entity },
    MaterialTextureRemoved { entity: Entity },
}

#[derive(QueryData)]
pub struct UpdateMeshBindGroupsQueryData {
    name: NameOrEntity,
    mesh: &'static Mesh,
    albedo_texture: Option<&'static AlbedoTexture>,
    material_texture: Option<&'static MaterialTexture>,
}

pub fn update_mesh_bind_groups(
    renderer: Res<SharedRenderer>,
    query: Query<UpdateMeshBindGroupsQueryData>,
    mut messages: MessageReader<UpdateMeshBindGroupMessage>,
    mut commands: Commands,
    mut updated: Local<EntityHashSet>,
) {
    assert!(updated.is_empty());

    messages.read().for_each(|message| {
        match message {
            UpdateMeshBindGroupMessage::MeshAdded { entity }
            | UpdateMeshBindGroupMessage::AlbedoTextureAdded { entity }
            | UpdateMeshBindGroupMessage::MaterialTextureAdded { entity }
            | UpdateMeshBindGroupMessage::AlbedoTextureRemoved { entity }
            | UpdateMeshBindGroupMessage::MaterialTextureRemoved { entity } => {
                if updated.insert(*entity) {
                    let item = query.get(*entity).unwrap();
                    tracing::debug!(?message, name = %item.name, "update mesh bind group");

                    let entity_commands = commands.entity(*entity);

                    update_mesh_bind_group(
                        &*renderer,
                        entity_commands,
                        item.mesh,
                        item.albedo_texture,
                        item.material_texture,
                        item.name,
                    );
                }
            }
            UpdateMeshBindGroupMessage::MeshRemoved { entity } => {
                tracing::debug!(?message, "remove mesh bind group");

                updated.remove(entity);
                commands.entity(*entity).remove::<MeshBindGroup>();
            }
        }
    });

    updated.clear();
}

pub fn update_mesh_bind_group(
    renderer: &Renderer,
    mut entity_commands: EntityCommands,
    mesh: &Mesh,
    albedo_texture: Option<&AlbedoTexture>,
    material_texture: Option<&MaterialTexture>,
    name: NameOrEntityItem,
) {
    if !mesh.flags.contains(MeshFlags::UVS)
        && (albedo_texture.is_some() || material_texture.is_some())
    {
        tracing::warn!(%name, "Mesh with textures, but no UV buffer");
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

pub fn update_camera_viewports(
    mut changed_viewports: Query<(&mut CameraProjection, &Viewport), Changed<Viewport>>,
) {
    changed_viewports
        .iter_mut()
        .for_each(|(mut camera_projection, viewport)| {
            camera_projection.set_viewport(viewport);
        });
}

#[derive(QueryData)]
pub struct CreateCameraBindGroupsQueryData {
    entity: Entity,
    camera_projection: &'static CameraProjection,
    global_transform: &'static GlobalTransform,
    clear_color: Option<&'static ClearColor>,
    ambient_light: Option<&'static AmbientLight>,
    point_light: Option<&'static PointLight>,
    camera_config: Option<&'static CameraConfig>,
}

pub fn create_camera_bind_groups(
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
            let camera_bind_group = CameraBindGroup::new(
                &renderer.camera_bind_group_layout,
                &renderer.wgpu_context.device,
                &camera_data,
                state.instance_buffer.buffer.buffer().unwrap(),
            );
            commands.entity(entity).insert(camera_bind_group);
        },
    )
}

pub fn destroy_camera_bind_groups(
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
pub struct UpdateCameraBindGroupsQueryData {
    camera_bind_group: &'static mut CameraBindGroup,
    camera_projection: &'static CameraProjection,
    global_transform: &'static GlobalTransform,
    clear_color: Option<&'static ClearColor>,
    ambient_light: Option<&'static AmbientLight>,
    point_light: Option<&'static PointLight>,
    camera_config: Option<&'static CameraConfig>,
}

pub fn update_camera_bind_groups(
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

/// Prepares rendering a frame for a specific view.
///
/// This just fetches camera information and the prepared draw commands
///
/// The [`DrawCommand`] can be cloned and passed via [`egui::PaintCallback`]
/// to do the actual rendering with a [`wgpu::RenderPass`].
///
/// Note that the actual draw commands are prepared in
/// [`update_instance_buffer_and_draw_command`], since they can be shared by
/// multiple view widgets.
pub fn grab_draw_list_for_camera(
    In(camera_entity): In<Entity>,
    renderer: Res<SharedRenderer>,
    state: Res<RendererState>,
    command_sender: Res<CommandSender>,
    cameras: Query<(
        &CameraBindGroup,
        Option<&CameraConfig>,
        Has<ClearColor>,
        &GlobalTransform,
    )>,
) -> Option<DrawCommand> {
    // get bind group and config for our camera
    let (camera_resources, camera_config, has_clear_color, camera_transform) =
        cameras.get(camera_entity).unwrap();

    // default to all, then apply configuration, so by default stuff will render and
    // we don't have to debug for 15 minutes to find that we don't enable the
    // pipeline
    let mut draw_command_flags = DrawCommandFlags::all();
    draw_command_flags.set(DrawCommandFlags::CLEAR, has_clear_color);
    if let Some(camera_config) = camera_config {
        camera_config.apply_to_draw_command_flags(&mut draw_command_flags);
    }

    Some(state.draw_command_buffer.finish(
        &renderer,
        camera_resources.bind_group.clone(),
        camera_transform.position(),
        draw_command_flags,
        DrawCommandInfoSink {
            command_sender: command_sender.clone(),
            camera_entity,
        },
    ))
}

pub fn commit_resource_transaction(mut transaction: ResMut<RenderResourceTransactionState>) {
    if let Some(transaction) = transaction.0.take() {
        tracing::debug!("commiting resource transaction");
        transaction.commit();
    }
}

pub fn handle_command_queue(
    renderer: Res<SharedRenderer>,
    mut transaction: ResMut<RenderResourceTransactionState>,
    mut command_receiver: ResMut<CommandReceiver>,
    mut commands: Commands,
) {
    // todo: for this to be able to run multi-threaded we would need to open
    // multiple staging transactions.

    for command in command_receiver.drain() {
        match command {
            Command::CopyImageToTexture(command) => {
                command.handle(|image, texture| {
                    transaction.with(&renderer, |transaction| {
                        image.write_to_texture(texture, &mut transaction.write_staging);
                    });
                });
            }
            Command::DrawCommandInfo {
                camera_entity,
                draw_command_info,
            } => {
                commands.entity(camera_entity).insert(draw_command_info);
            }
        }
    }
}
