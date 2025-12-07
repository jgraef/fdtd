use bevy_ecs::{
    component::Component,
    entity::Entity,
    hierarchy::{
        ChildOf,
        Children,
    },
    name::NameOrEntity,
    query::{
        QueryData,
        With,
        Without,
    },
    reflect::ReflectComponent,
    system::{
        In,
        InMut,
        Query,
    },
};
use bevy_reflect::{
    Reflect,
    ReflectSerialize,
    prelude::ReflectDefault,
};
use cem_probe::PropertiesUi;
use cem_render::material::Outline;
use cem_scene::probe::{
    ComponentName,
    ReflectComponentUi,
};
use cem_util::egui::EguiUtilUiExt;
use egui_ltreeview::{
    Action,
    IndentHintStyle,
    TreeView,
    TreeViewBuilder,
    TreeViewState,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::composer::{
    ComposerState,
    selection::Selection,
};

#[derive(Debug, Default)]
pub struct ObjectTreeState {
    tree_state: TreeViewState<ObjectTreeId>,
}

impl ComposerState {
    pub(super) fn object_tree(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let selection_outline = self.config.views.selection_outline;
        self.scene
            .world
            .run_system_cached_with(
                render_object_tree_system,
                (ui, &mut self.object_tree.tree_state, selection_outline),
            )
            .unwrap()
    }
}

#[derive(QueryData)]
struct Node {
    name: NameOrEntity,
    children: Option<&'static Children>,
}

fn render_object_tree_system(
    (InMut(ui), InMut(tree_view_state), In(selection_outline)): (
        InMut<egui::Ui>,
        InMut<TreeViewState<ObjectTreeId>>,
        In<Outline>,
    ),
    roots: Query<Node, (With<ShowInTree>, Without<ChildOf>)>,
    children: Query<Node, (With<ShowInTree>, With<ChildOf>)>,
    mut selection: Selection,
) -> egui::Response {
    /// Renders a list of nodes including their children
    fn show<'a, 'w, 's, I>(
        items: I,
        builder: &'a mut TreeViewBuilder<ObjectTreeId>,
        children: &'a Query<Node, (With<ShowInTree>, With<ChildOf>)>,
    ) where
        I: Iterator<Item = NodeItem<'w, 's>>,
    {
        let mut items_sorted = items.collect::<Vec<_>>();
        items_sorted.sort_unstable_by_key(|item| item.name.entity);

        for item in items_sorted {
            if let Some(children_of_item) = item
                .children
                .filter(|children_of_item| !children_of_item.is_empty())
            {
                builder.dir(item.name.entity.into(), item.name.to_string());
                show_children(children_of_item, builder, children);
                builder.close_dir();
            }
            else {
                builder.leaf(item.name.entity.into(), item.name.to_string());
            }
        }
    }

    // note: `show` could just directly recurse but this causes the compiler to
    // recurse endlessly (because show is generic). instead we need to call a
    // non-generic function during the recursion that breaks up the cycle.
    fn show_children(
        children_of_item: &Children,
        builder: &mut TreeViewBuilder<ObjectTreeId>,
        children: &Query<Node, (With<ShowInTree>, With<ChildOf>)>,
    ) {
        show(
            children_of_item
                .iter()
                .map(|child| children.get(*child).unwrap()),
            builder,
            children,
        );
    }

    // sync ecs with tree view state
    tree_view_state.set_selected(selection.entities().map(Into::into).collect());

    // render tree view
    let (response, actions) = TreeView::new(ui.id().with("composer_object_tree"))
        .allow_multi_selection(true)
        .allow_drag_and_drop(false)
        .indent_hint_style(IndentHintStyle::Line)
        .override_indent(Some(10.0))
        .show_state(ui, tree_view_state, |builder| {
            builder.dir(ObjectTreeId::Root, "Scene");
            show(roots.iter(), builder, &children);
            builder.close_dir();
        });

    // whether something was selected in the tree view
    let mut set_selected = false;

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
                        selection.select(entity, selection_outline);
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

/// Tag for entities that are to be shown in the object tree
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Component, Reflect)]
#[reflect(Component, ComponentUi, @ComponentName::new("Show in Tree"), Default, Serialize)]
pub struct ShowInTree;

impl PropertiesUi for ShowInTree {
    type Config = ();

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let _ = config;
        ui.noop()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ObjectTreeId {
    Root,
    Entity(Entity),
}

impl From<Entity> for ObjectTreeId {
    fn from(value: Entity) -> Self {
        Self::Entity(value)
    }
}
