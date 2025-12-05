#![allow(clippy::type_complexity)]

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
            BvhLeaf,
            BvhMessage,
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
        Query<(NameOrEntity, &BvhLeaf)>,
        Query<
            (&GlobalTransform, &Collider, &BvhLeaf),
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
                    let bvh_leaf = transaction.insert(*entity, transform, collider);
                    tracing::debug!(entity = %name, ?bvh_leaf, "adding to bvh");
                    commands.entity(*entity).insert(bvh_leaf);
                }
            }
            BvhMessage::Remove { entity } => {
                let query = queries.p1();
                if let Ok((name, bvh_leaf)) = query.get(*entity) {
                    tracing::debug!(entity = %name, ?bvh_leaf, "removing from bvh");
                    transaction.remove(bvh_leaf);
                    commands.entity(*entity).remove::<BvhLeaf>();
                }
            }
        }
    });

    {
        let query = queries.p2();
        for (transform, collider, bvh_leaf) in query.iter() {
            transaction.update(bvh_leaf, transform, collider);
        }
    }
}
