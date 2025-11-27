use egui_ltreeview::{
    Action,
    IndentHintStyle,
    TreeView,
    TreeViewBuilder,
    TreeViewState,
};
use hecs_hierarchy::{
    Child,
    Hierarchy,
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
}

impl ComposerState {
    pub(super) fn object_tree(&mut self, ui: &mut egui::Ui) -> egui::Response {
        self.object_tree
            .tree_state
            .set_selected(self.selection().with_query_iter::<(), _>(|selected| {
                selected.map(|(entity, ())| entity.into()).collect()
            }));

        let (response, actions) = TreeView::new(ui.id().with("composer_object_tree"))
            .allow_multi_selection(true)
            .allow_drag_and_drop(false)
            .indent_hint_style(IndentHintStyle::Line)
            .override_indent(Some(10.0))
            .show_state(ui, &mut self.object_tree.tree_state, |builder| {
                builder.dir(ObjectTreeId::Root, "Scene");
                let mut labels = self.scene.entities.view::<Option<&Label>>();
                let mut visitor = Visitor {
                    world: &self.scene.entities,
                    builder,
                    labels: &mut labels,
                };
                visitor.visit_roots();
                builder.close_dir();
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

struct Visitor<'a, 'ui, 'world> {
    world: &'a hecs::World,
    builder: &'a mut TreeViewBuilder<'ui, ObjectTreeId>,
    labels: &'a mut hecs::ViewBorrow<'world, Option<&'world Label>>,
}

impl<'a, 'ui, 'world> Visitor<'a, 'ui, 'world> {
    fn visit_all(&mut self, entities: impl Iterator<Item = hecs::Entity>) {
        let mut scratch = entities.collect::<Vec<_>>();
        scratch.sort_by_key(|entity| *entity);

        for entity in scratch {
            let label = self.labels.get(entity).flatten();
            let label = EntityDebugLabel {
                entity,
                label: label.cloned(),
                invalid: false,
            };

            let mut children = self.world.children::<()>(entity).peekable();

            if children.peek().is_some() {
                self.builder.dir(entity.into(), label);
                self.visit_all(children);
                self.builder.close_dir();
            }
            else {
                self.builder.leaf(entity.into(), label);
            }
        }
    }

    fn visit_roots(&mut self) {
        self.visit_all(
            self.world
                .query::<()>()
                .with::<&ShowInTree>()
                .without::<&Child<()>>()
                .iter()
                .map(|(entity, _)| entity),
        );
    }
}

/// Tag for entities that are to be shown in the object tree
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ShowInTree;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ObjectTreeId {
    Root,
    Entity(hecs::Entity),
}

impl From<hecs::Entity> for ObjectTreeId {
    fn from(value: hecs::Entity) -> Self {
        Self::Entity(value)
    }
}
