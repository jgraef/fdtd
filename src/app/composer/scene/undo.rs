use std::collections::VecDeque;

use crate::app::composer::scene::Scene;

#[derive(derive_more::Debug)]
pub struct UndoBuffer {
    undo_actions: VecDeque<UndoAction>,
    undo_limit: Option<usize>,

    redo_actions: VecDeque<RedoAction>,
    redo_limit: Option<usize>,

    #[debug("hecs::World {{ ... }}")]
    hades: hecs::World,
}

impl Default for UndoBuffer {
    fn default() -> Self {
        Self {
            undo_actions: VecDeque::new(),
            undo_limit: Some(100),
            redo_actions: VecDeque::new(),
            redo_limit: Some(100),
            hades: hecs::World::new(),
        }
    }
}

impl UndoBuffer {
    pub fn deleted_entity(&mut self, taken_entity: hecs::TakenEntity) {
        let hades_entity = self.hades.spawn(taken_entity);

        // todo: we might want to remove some components from the entity to save
        // resources (e.g. the mesh)

        self.undo_actions
            .push_front(UndoAction::DeleteEntity { hades_entity });

        self.limit_undo_buffer();
    }

    fn limit_undo_buffer(&mut self) {
        if let Some(undo_limit) = self.undo_limit {
            while self.undo_actions.len() > undo_limit {
                let undo_action = self.undo_actions.pop_back().unwrap();

                for hades_entity in undo_action.hades_entities() {
                    self.hades.despawn(hades_entity).unwrap();
                }
            }
        }
    }

    pub fn undo_most_recent(&mut self, scene: &mut Scene) {
        if let Some(undo_action) = self.undo_actions.pop_front() {
            match undo_action {
                UndoAction::DeleteEntity { hades_entity } => {
                    let resurrected_entity = self.hades.take(hades_entity).unwrap();
                    let entity = scene.entities.spawn(resurrected_entity);
                    self.redo_actions
                        .push_front(RedoAction::DeleteEntity { entity });
                }
                UndoAction::CreateEntity { entity: _ } => {
                    //scene.delete(entity);
                    //todo!();
                }
            }
        }
    }

    pub fn iter_undo(&self) -> std::collections::vec_deque::Iter<'_, UndoAction> {
        self.undo_actions.iter()
    }

    pub fn iter_redo(&self) -> std::collections::vec_deque::Iter<'_, RedoAction> {
        self.redo_actions.iter()
    }

    pub fn has_undos(&self) -> bool {
        !self.undo_actions.is_empty()
    }

    pub fn has_redos(&self) -> bool {
        !self.redo_actions.is_empty()
    }
}

#[derive(Debug)]
pub enum UndoAction {
    DeleteEntity { hades_entity: hecs::Entity },
    CreateEntity { entity: hecs::Entity },
}

impl UndoAction {
    fn hades_entities(&self) -> Vec<hecs::Entity> {
        match self {
            UndoAction::DeleteEntity { hades_entity } => vec![*hades_entity],
            _ => vec![],
        }
    }
}

#[derive(Debug)]
pub enum RedoAction {
    DeleteEntity { entity: hecs::Entity },
}
