use std::collections::VecDeque;

use cem_scene::Scene;

use crate::debug::DebugUi;

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
        Self::new(None, None)
    }
}

impl UndoBuffer {
    pub fn new(undo_limit: Option<usize>, redo_limit: Option<usize>) -> Self {
        Self {
            undo_actions: VecDeque::new(),
            undo_limit,
            redo_actions: VecDeque::new(),
            redo_limit,
            hades: Default::default(),
        }
    }

    pub fn send_to_hades(&mut self, entity: hecs::TakenEntity) -> HadesId {
        HadesId {
            entity: self.hades.spawn(entity),
        }
    }

    pub fn push_undo(&mut self, undo: UndoAction) {
        self.undo_actions.push_front(undo);
        self.limit_undo_buffer();
    }

    fn limit_undo_buffer(&mut self) {
        if let Some(undo_limit) = self.undo_limit {
            while self.undo_actions.len() > undo_limit {
                let undo_action = self.undo_actions.pop_back().unwrap();

                #[allow(clippy::single_match)]
                match undo_action {
                    UndoAction::DeleteEntity { hades_ids } => {
                        for hades_id in hades_ids {
                            self.hades.despawn(hades_id.entity).unwrap();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn undo_most_recent(&mut self, _scene: &mut Scene) {
        /*if let Some(undo_action) = self.undo_actions.pop_front() {
            match undo_action {
                UndoAction::DeleteEntity { hades_ids } => {
                    for hades_id in hades_ids {
                        let resurrected_entity = self.hades.take(hades_id.entity).unwrap();
                        let entity = scene.entities.spawn(resurrected_entity);
                        self.redo_actions
                            .push_front(RedoAction::DeleteEntity { entity });
                    }
                }
                UndoAction::CreateEntity { entity: _ } => {
                    //scene.delete(entity);
                    //todo!();
                }
            }
        }*/
        // todo bevy-migrate
        todo!();
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
    DeleteEntity { hades_ids: Vec<HadesId> },
    CreateEntity { entity: hecs::Entity },
}

#[derive(Debug)]
pub enum RedoAction {
    DeleteEntity { entity: hecs::Entity },
}

/// It's just an [`hecs::Entity`], but wrapped to avoid mixups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HadesId {
    entity: hecs::Entity,
}

impl DebugUi for UndoBuffer {
    fn show_debug(&self, ui: &mut egui::Ui) {
        ui.label("Undo:");
        let mut empty = true;
        for undo_action in self.undo_actions.iter().take(10) {
            empty = false;
            ui.code(format!("{undo_action:?}"));
        }
        if empty {
            ui.label("No undo actions");
        }

        ui.separator();
        ui.label("Redo:");
        let mut empty = true;
        for redo_action in self.redo_actions.iter().take(10) {
            empty = false;
            ui.code(format!("{redo_action:?}"));
        }
        if empty {
            ui.label("No redo actions");
        }
    }
}
