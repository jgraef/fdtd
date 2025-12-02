use bevy_ecs::{
    message::MessageReader,
    name::NameOrEntity,
    query::{
        Changed,
        Or,
    },
    system::{
        Commands,
        Local,
        ParamSet,
        Query,
        ResMut,
    },
};

use crate::{
    spatial::{
        bvh::{
            Bvh,
            BvhMessage,
            LeafIndex,
        },
        collider::Collider,
    },
    transform::GlobalTransform,
};

pub fn update_bvh(
    mut bvh: ResMut<Bvh>,
    mut workspace: Local<parry3d::partitioning::BvhWorkspace>,
    mut queries: ParamSet<(
        Query<(NameOrEntity, &GlobalTransform, &Collider)>,
        Query<(NameOrEntity, &LeafIndex)>,
        Query<
            (&GlobalTransform, &Collider, &LeafIndex),
            Or<(Changed<GlobalTransform>, Changed<Collider>)>,
        >,
    )>,
    mut messages: MessageReader<BvhMessage>,
    mut commands: Commands,
) {
    let mut transaction = bvh.transaction(&mut workspace);

    messages.read().for_each(|message| {
        match message {
            BvhMessage::Insert { entity } => {
                let query = queries.p0();
                if let Ok((name, transform, collider)) = query.get(*entity) {
                    let leaf_index = transaction.insert(*entity, transform, collider);
                    tracing::debug!(entity = %name, ?leaf_index, "adding to bvh");
                    commands.entity(*entity).insert(leaf_index);
                }
            }
            BvhMessage::Remove { entity } => {
                let query = queries.p1();
                if let Ok((name, leaf_index)) = query.get(*entity) {
                    tracing::debug!(entity = %name, ?leaf_index, "removing from bvh");
                    transaction.remove(leaf_index);
                    commands.entity(*entity).remove::<LeafIndex>();
                }
            }
        }
    });

    {
        let query = queries.p2();
        for (transform, collider, leaf_index) in query.iter() {
            transaction.update(leaf_index, transform, collider);
        }
    }
}
