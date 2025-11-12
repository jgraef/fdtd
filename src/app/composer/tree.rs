use egui_ltreeview::{
    Action,
    TreeView,
    TreeViewState,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::app::composer::scene::{
    EntityDebugLabel,
    Label,
    Scene,
};

#[derive(Debug, Default)]
pub struct ObjectTree {
    tree_state: TreeViewState<ObjectTreeId>,

    // kinda hack to know what the tree view state has selected
    previous_selection: Option<hecs::Entity>,

    object_scratch: Vec<(hecs::Entity, Option<Label>)>,
}

impl ObjectTree {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        scene: &mut Scene,
        selected_object: &mut Option<hecs::Entity>,
    ) {
        // sync selection from composer with tree view
        if self.previous_selection != *selected_object {
            if let Some(selected_object) = *selected_object {
                self.tree_state
                    .set_one_selected(ObjectTreeId::Object(selected_object));
            }
            else {
                self.tree_state.set_selected(vec![]);
            }
        }

        let (_response, actions) = TreeView::new(ui.make_persistent_id("composer_object_tree"))
            .allow_multi_selection(false)
            .allow_drag_and_drop(false)
            .show_state(ui, &mut self.tree_state, |builder| {
                builder.dir(ObjectTreeId::ObjectDirectory, "Objects");

                for (entity, label) in scene
                    .entities
                    .query_mut::<Option<&Label>>()
                    .with::<&ShowInTree>()
                {
                    self.object_scratch.push((entity, label.cloned()));
                }

                self.object_scratch.sort_by_key(|(entity, _)| *entity);

                for (entity, label) in self.object_scratch.drain(..) {
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

        for action in actions {
            #[allow(clippy::single_match)]
            match action {
                Action::SetSelected(items) => {
                    assert!(items.len() == 1, "expected exactly one item in selection");
                    match items[0] {
                        ObjectTreeId::Object(entity) => {
                            // entity selected
                            tracing::debug!(
                                "object selected in tree: {}",
                                scene.entity_debug_label(entity)
                            );
                            *selected_object = Some(entity);
                            self.previous_selection = Some(entity);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

/// Tag for entities that are to be shown in the object tree
#[derive(Clone, Copy, Debug, Default)]
pub struct ShowInTree;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ObjectTreeId {
    ObjectDirectory,
    Object(hecs::Entity),
}

impl From<hecs::Entity> for ObjectTreeId {
    fn from(value: hecs::Entity) -> Self {
        Self::Object(value)
    }
}
