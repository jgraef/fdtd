use egui_ltreeview::{
    Action,
    TreeView,
    TreeViewState,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::app::composer::{
    ComposerState,
    scene::{
        EntityDebugLabel,
        Label,
    },
};

#[derive(Debug, Default)]
pub struct ObjectTreeState {
    tree_state: TreeViewState<ObjectTreeId>,

    object_scratch: Vec<(hecs::Entity, Option<Label>)>,
}

impl ComposerState {
    pub(super) fn object_tree(&mut self, ui: &mut egui::Ui) -> egui::Response {
        self.object_tree
            .tree_state
            .set_selected(self.selection().with_query_iter::<(), _>(|selected| {
                selected.map(|(entity, ())| entity.into()).collect()
            }));

        let (response, actions) = TreeView::new(ui.make_persistent_id("composer_object_tree"))
            .allow_multi_selection(true)
            .allow_drag_and_drop(false)
            .show_state(ui, &mut self.object_tree.tree_state, |builder| {
                builder.dir(ObjectTreeId::ObjectDirectory, "Objects");

                for (entity, label) in self
                    .scene
                    .entities
                    .query_mut::<Option<&Label>>()
                    .with::<&ShowInTree>()
                {
                    self.object_tree
                        .object_scratch
                        .push((entity, label.cloned()));
                }

                self.object_tree
                    .object_scratch
                    .sort_by_key(|(entity, _)| *entity);

                for (entity, label) in self.object_tree.object_scratch.drain(..) {
                    builder.leaf(
                        entity.into(),
                        EntityDebugLabel {
                            entity,
                            label,
                            invalid: false,
                        },
                    );
                }
            });

        // whether something was selected in the tree view
        let mut set_selected = false;

        let mut selection = self.selection_mut();

        for action in actions {
            #[allow(clippy::single_match)]
            match action {
                Action::SetSelected(items) => {
                    // the tree view always gives us the complete selection, so we need to clear the
                    // selection first
                    selection.clear();

                    // add selected entities to selection
                    for item in items {
                        if let ObjectTreeId::Entity(entity) = item {
                            selection.select(entity);
                        }
                    }

                    // remember that we selected something for later
                    set_selected = true;
                }
                _ => {}
            }
        }

        // if the widget was clicked, but nothing was selected, clear selection
        if response.clicked() && !set_selected {
            selection.clear();
        }

        response
    }
}

/// Tag for entities that are to be shown in the object tree
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ShowInTree;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ObjectTreeId {
    ObjectDirectory,
    Entity(hecs::Entity),
}

impl From<hecs::Entity> for ObjectTreeId {
    fn from(value: hecs::Entity) -> Self {
        Self::Entity(value)
    }
}
