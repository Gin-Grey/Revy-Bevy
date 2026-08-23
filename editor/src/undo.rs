//! Scene command history backed by stable-ID snapshots.

use std::collections::HashMap;

use bevy::{
    gizmos::transform_gizmo::{TransformGizmoState, TransformGizmoSystems},
    input_focus::InputFocus,
    prelude::*,
    text::EditableText,
    ui::InteractionDisabled,
    ui_widgets::Activate,
};

use crate::{
    animation_timeline::{
        AnimationPreviewOriginalSpriteFrame, AnimationPreviewOriginalTransform,
        AnimationPreviewOriginalUiLayout,
    },
    entities::{
        AddedEntityComponents, BuiltinComponent, EntityCustomComponents, EntityKind,
        EntityScriptBinding, EntitySystemBindings, SceneAnimationPlayer, SceneCollisionRect2D,
        SceneModel3D, SceneSprite2D, SceneUiContent, SceneUiLayout, insert_builtin_component,
        spawn_entity,
    },
    hierarchy::{SceneNodeId, SceneParentId, SceneRoot, SceneRootMenuState, SceneSiblingOrder},
    scene::SceneDocument,
    selection::{EditableObject, Selection},
    workspace::{SceneSpace, WorkspaceMode, WorkspaceSelections},
};

const HISTORY_LIMIT: usize = 256;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryAction {
    #[default]
    Undo,
    Redo,
}

#[derive(Component, Clone, Copy, Default)]
pub struct HistoryActionButton(pub HistoryAction);

#[derive(Component, Clone, Copy, Default)]
pub struct HistoryStatusLabel;

#[derive(Clone, Debug, PartialEq)]
pub struct SceneSnapshot {
    nodes: Vec<SceneNodeSnapshot>,
    selected: Option<SceneNodeId>,
    mode: WorkspaceMode,
}

#[derive(Clone, Debug, PartialEq)]
struct SceneNodeSnapshot {
    id: SceneNodeId,
    parent: Option<SceneNodeId>,
    order: u32,
    name: String,
    kind: EntityKind,
    components: Vec<BuiltinComponent>,
    custom_components: EntityCustomComponents,
    entity_script: EntityScriptBinding,
    systems: EntitySystemBindings,
    space: SceneSpace,
    transform: Option<Transform>,
    visibility: Option<Visibility>,
    ui_layout: Option<SceneUiLayout>,
    ui_content: Option<SceneUiContent>,
    sprite: Option<SceneSprite2D>,
    model: Option<SceneModel3D>,
    animation_player: Option<SceneAnimationPlayer>,
    collision_rect: Option<SceneCollisionRect2D>,
    root: bool,
}

#[derive(Clone, Debug)]
struct HistoryEntry {
    label: String,
    before: SceneSnapshot,
    after: SceneSnapshot,
}

#[derive(Clone, Debug)]
struct PendingAction {
    label: String,
    before: SceneSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HistoryRequest {
    Reset { saved: bool },
    MarkSaved,
    Clear,
}

#[derive(Resource, Debug, Default)]
pub struct SceneHistory {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    pending: Option<PendingAction>,
    saved: Option<SceneSnapshot>,
    request: Option<HistoryRequest>,
    revision: u64,
}

impl SceneHistory {
    pub fn begin(&mut self, label: impl Into<String>, before: SceneSnapshot) {
        if self.pending.is_none() {
            self.pending = Some(PendingAction {
                label: label.into(),
                before,
            });
        }
    }

    pub fn request_reset(&mut self, saved: bool) {
        self.request = Some(HistoryRequest::Reset { saved });
    }

    pub fn request_mark_saved(&mut self) {
        self.request = Some(HistoryRequest::MarkSaved);
    }

    pub fn request_clear(&mut self) {
        self.request = Some(HistoryRequest::Clear);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Records an interaction that owns its full lifetime, such as a viewport drag.
    pub fn commit(
        &mut self,
        label: impl Into<String>,
        before: SceneSnapshot,
        after: SceneSnapshot,
        document: &mut SceneDocument,
    ) {
        self.pending = None;
        self.record(label.into(), before, after, document);
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|entry| entry.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|entry| entry.label.as_str())
    }

    fn record(
        &mut self,
        label: String,
        before: SceneSnapshot,
        after: SceneSnapshot,
        document: &mut SceneDocument,
    ) {
        if before == after {
            return;
        }
        self.undo.push(HistoryEntry {
            label,
            before,
            after: after.clone(),
        });
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.update_dirty(&after, document);
        self.bump_revision();
    }

    fn update_dirty(&self, current: &SceneSnapshot, document: &mut SceneDocument) {
        document.dirty = self
            .saved
            .as_ref()
            .is_none_or(|saved| saved.nodes != current.nodes);
        document.bump_revision();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(bevy::ecs::query::QueryData)]
pub struct SceneSnapshotQuery {
    entity: Entity,
    object: &'static EditableObject,
    id: &'static SceneNodeId,
    parent: &'static SceneParentId,
    order: &'static SceneSiblingOrder,
    space: &'static SceneSpace,
    transform: Option<&'static Transform>,
    preview_transform: Option<&'static AnimationPreviewOriginalTransform>,
    kind: Option<&'static EntityKind>,
    components: Option<&'static AddedEntityComponents>,
    custom_components: Option<&'static EntityCustomComponents>,
    entity_script: Option<&'static EntityScriptBinding>,
    systems: Option<&'static EntitySystemBindings>,
    visibility: Option<&'static Visibility>,
    ui_layout: Option<&'static SceneUiLayout>,
    preview_ui_layout: Option<&'static AnimationPreviewOriginalUiLayout>,
    ui_content: Option<&'static SceneUiContent>,
    sprite: Option<&'static SceneSprite2D>,
    preview_sprite_frame: Option<&'static AnimationPreviewOriginalSpriteFrame>,
    model: Option<&'static SceneModel3D>,
    animation_player: Option<&'static SceneAnimationPlayer>,
    collision_rect: Option<&'static SceneCollisionRect2D>,
    root: Option<&'static SceneRoot>,
}

pub fn capture_scene_snapshot(
    nodes: &Query<SceneSnapshotQuery>,
    selection: &Selection,
    mode: WorkspaceMode,
) -> SceneSnapshot {
    let selected = selection
        .0
        .and_then(|selected| nodes.get(selected).ok().map(|node| *node.id));
    let mut snapshot_nodes: Vec<_> = nodes
        .iter()
        .map(|node| SceneNodeSnapshot {
            id: *node.id,
            parent: node.parent.0,
            order: node.order.0,
            name: node.object.name.clone(),
            kind: node.kind.copied().unwrap_or(match *node.space {
                SceneSpace::TwoD => EntityKind::Empty2D,
                SceneSpace::ThreeD => EntityKind::Empty3D,
            }),
            components: node
                .components
                .map(|components| components.0.clone())
                .unwrap_or_default(),
            custom_components: node.custom_components.cloned().unwrap_or_default(),
            entity_script: node.entity_script.cloned().unwrap_or_default(),
            systems: node.systems.cloned().unwrap_or_default(),
            space: *node.space,
            transform: node
                .preview_transform
                .map(|original| original.0)
                .or(node.transform.copied()),
            visibility: node.visibility.copied(),
            ui_layout: node
                .preview_ui_layout
                .map(|original| original.0)
                .or(node.ui_layout.copied()),
            ui_content: node.ui_content.cloned(),
            sprite: node.sprite.map(|sprite| {
                let mut sprite = sprite.clone();
                if let Some(original) = node.preview_sprite_frame {
                    sprite.frame = original.0;
                }
                sprite
            }),
            model: node.model.cloned(),
            animation_player: node.animation_player.cloned(),
            collision_rect: node.collision_rect.copied(),
            root: node.root.is_some(),
        })
        .collect();
    snapshot_nodes.sort_by_key(|node| node.id.to_string());
    SceneSnapshot {
        nodes: snapshot_nodes,
        selected,
        mode,
    }
}

pub struct SceneHistoryPlugin;

impl Plugin for SceneHistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneHistory>()
            .add_observer(handle_history_button)
            .add_systems(Update, (handle_history_shortcuts, sync_history_chrome))
            .add_systems(
                PostUpdate,
                (
                    track_gizmo_history.after(TransformGizmoSystems),
                    process_history_request,
                    finalize_pending_action,
                )
                    .chain(),
            );
    }
}

fn handle_history_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    buttons: Query<(Entity, &HistoryActionButton)>,
    mut commands: Commands,
) {
    if focus
        .as_deref()
        .and_then(InputFocus::get)
        .is_some_and(|entity| editable_text.get(entity).is_ok())
    {
        return;
    }
    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !control {
        return;
    }
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let action = if keyboard.just_pressed(KeyCode::KeyZ) {
        Some(if shift {
            HistoryAction::Redo
        } else {
            HistoryAction::Undo
        })
    } else if keyboard.just_pressed(KeyCode::KeyY) {
        Some(HistoryAction::Redo)
    } else {
        None
    };
    let Some(action) = action else {
        return;
    };
    if let Some((entity, _)) = buttons.iter().find(|(_, button)| button.0 == action) {
        commands.trigger(Activate { entity });
    }
}

fn handle_history_button(
    activate: On<Activate>,
    buttons: Query<&HistoryActionButton>,
    nodes: Query<SceneSnapshotQuery>,
    mut history: ResMut<SceneHistory>,
    mut document: ResMut<SceneDocument>,
    mut selection: ResMut<Selection>,
    mut saved_selections: ResMut<WorkspaceSelections>,
    mut mode: ResMut<WorkspaceMode>,
    mut root_menu: ResMut<SceneRootMenuState>,
    mut commands: Commands,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    history.pending = None;
    let entry = match button.0 {
        HistoryAction::Undo => history.undo.pop(),
        HistoryAction::Redo => history.redo.pop(),
    };
    let Some(entry) = entry else {
        return;
    };
    let snapshot = match button.0 {
        HistoryAction::Undo => &entry.before,
        HistoryAction::Redo => &entry.after,
    };
    restore_scene_snapshot(
        snapshot,
        &nodes,
        &mut commands,
        &mut selection,
        &mut saved_selections,
        &mut mode,
        &mut root_menu,
    );
    history.update_dirty(snapshot, &mut document);
    match button.0 {
        HistoryAction::Undo => history.redo.push(entry),
        HistoryAction::Redo => history.undo.push(entry),
    }
    history.bump_revision();
}

fn restore_scene_snapshot(
    snapshot: &SceneSnapshot,
    nodes: &Query<SceneSnapshotQuery>,
    commands: &mut Commands,
    selection: &mut Selection,
    saved_selections: &mut WorkspaceSelections,
    mode: &mut WorkspaceMode,
    root_menu: &mut SceneRootMenuState,
) {
    for node in nodes.iter() {
        commands.entity(node.entity).despawn();
    }
    let mut entities = HashMap::new();
    for node in &snapshot.nodes {
        let entity = spawn_entity(
            commands,
            node.kind,
            node.name.clone(),
            node.id,
            node.parent,
            node.order,
            node.space,
        );
        for component in node.components.iter().copied() {
            insert_builtin_component(commands, entity, component);
        }
        commands.entity(entity).insert((
            AddedEntityComponents(node.components.clone()),
            node.custom_components.clone(),
            node.entity_script.clone(),
            node.systems.clone(),
        ));
        if let Some(transform) = node.transform {
            commands.entity(entity).insert(transform);
        }
        if let Some(visibility) = node.visibility {
            commands.entity(entity).insert(visibility);
        }
        if let Some(ui_layout) = node.ui_layout {
            commands.entity(entity).insert(ui_layout);
        }
        if let Some(ui_content) = node.ui_content.clone() {
            commands.entity(entity).insert(ui_content);
        }
        if let Some(sprite) = node.sprite.clone() {
            commands.entity(entity).insert(sprite);
        }
        if let Some(model) = node.model.clone() {
            commands.entity(entity).insert(model);
        }
        if let Some(animation_player) = node.animation_player.clone() {
            commands.entity(entity).insert(animation_player);
        }
        if let Some(collision) = node.collision_rect {
            commands.entity(entity).insert(collision);
        }
        if node.root {
            commands.entity(entity).insert(SceneRoot);
        }
        entities.insert(node.id, entity);
    }
    *mode = snapshot.mode;
    selection.0 = snapshot.selected.and_then(|id| entities.get(&id).copied());
    saved_selections.set(WorkspaceMode::TwoD, None);
    saved_selections.set(WorkspaceMode::ThreeD, None);
    saved_selections.set(snapshot.mode, selection.0);
    root_menu.open = false;
    root_menu.context_open = false;
}

fn process_history_request(
    nodes: Query<SceneSnapshotQuery>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: ResMut<SceneHistory>,
    mut document: ResMut<SceneDocument>,
) {
    let Some(request) = history.request.take() else {
        return;
    };
    match request {
        HistoryRequest::Clear => {
            history.undo.clear();
            history.redo.clear();
            history.pending = None;
            history.saved = None;
        }
        HistoryRequest::Reset { saved } => {
            let current = capture_scene_snapshot(&nodes, &selection, *mode);
            history.undo.clear();
            history.redo.clear();
            history.pending = None;
            history.saved = saved.then_some(current.clone());
            history.update_dirty(&current, &mut document);
        }
        HistoryRequest::MarkSaved => {
            let current = capture_scene_snapshot(&nodes, &selection, *mode);
            history.saved = Some(current);
            document.dirty = false;
            document.bump_revision();
        }
    }
    history.bump_revision();
}

fn finalize_pending_action(
    nodes: Query<SceneSnapshotQuery>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: ResMut<SceneHistory>,
    mut document: ResMut<SceneDocument>,
) {
    let Some(pending) = history.pending.take() else {
        return;
    };
    let after = capture_scene_snapshot(&nodes, &selection, *mode);
    history.record(pending.label, pending.before, after, &mut document);
}

#[derive(Default)]
struct GizmoHistoryState {
    active: bool,
    before: Option<SceneSnapshot>,
}

fn track_gizmo_history(
    gizmo: Res<TransformGizmoState>,
    nodes: Query<SceneSnapshotQuery>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: ResMut<SceneHistory>,
    mut document: ResMut<SceneDocument>,
    mut state: Local<GizmoHistoryState>,
) {
    if gizmo.active && !state.active {
        let mut before = capture_scene_snapshot(&nodes, &selection, *mode);
        if let Some(entity) = gizmo.entity
            && let Ok(node) = nodes.get(entity)
            && let Some(snapshot_node) = before.nodes.iter_mut().find(|item| item.id == *node.id)
        {
            snapshot_node.transform = Some(gizmo.start_transform.clone());
        }
        state.before = Some(before);
    } else if !gizmo.active
        && state.active
        && let Some(before) = state.before.take()
    {
        let after = capture_scene_snapshot(&nodes, &selection, *mode);
        history.record("Transform Entity".into(), before, after, &mut document);
    }
    state.active = gizmo.active;
}

fn sync_history_chrome(
    history: Res<SceneHistory>,
    mut buttons: Query<(Entity, &HistoryActionButton, Has<InteractionDisabled>)>,
    mut labels: Query<&mut Text, With<HistoryStatusLabel>>,
    mut commands: Commands,
) {
    if !history.is_changed() {
        return;
    }
    for (entity, button, disabled) in &mut buttons {
        let should_disable = match button.0 {
            HistoryAction::Undo => !history.can_undo(),
            HistoryAction::Redo => !history.can_redo(),
        };
        if should_disable && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !should_disable && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
    let status = history
        .undo_label()
        .map(|label| format!("Undo: {label}"))
        .or_else(|| history.redo_label().map(|label| format!("Redo: {label}")))
        .unwrap_or_else(|| "No history".into());
    for mut label in &mut labels {
        label.0 = status.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        let mut app = App::new();
        app.init_resource::<SceneDocument>()
            .init_resource::<Selection>()
            .init_resource::<WorkspaceSelections>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneRootMenuState>()
            .init_resource::<TransformGizmoState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(SceneHistoryPlugin);
        app
    }

    fn snapshot(name: &str, x: f32) -> SceneSnapshot {
        let id = SceneNodeId::parse("00000000000000000000000000000001").unwrap();
        SceneSnapshot {
            nodes: vec![SceneNodeSnapshot {
                id,
                parent: None,
                order: 0,
                name: name.into(),
                kind: EntityKind::Empty2D,
                components: Vec::new(),
                custom_components: EntityCustomComponents::default(),
                entity_script: EntityScriptBinding::default(),
                systems: EntitySystemBindings::default(),
                space: SceneSpace::TwoD,
                transform: Some(Transform::from_xyz(x, 0.0, 0.0)),
                visibility: Some(Visibility::Visible),
                ui_layout: None,
                ui_content: None,
                sprite: None,
                model: None,
                animation_player: None,
                collision_rect: None,
                root: true,
            }],
            selected: Some(id),
            mode: WorkspaceMode::TwoD,
        }
    }

    #[test]
    fn recording_clears_redo_and_tracks_labels() {
        let mut history = SceneHistory::default();
        let mut document = SceneDocument::default();
        history.redo.push(HistoryEntry {
            label: "Old".into(),
            before: snapshot("Root", 0.0),
            after: snapshot("Root", 1.0),
        });

        history.record(
            "Move Entity".into(),
            snapshot("Root", 0.0),
            snapshot("Root", 2.0),
            &mut document,
        );

        assert_eq!(history.undo_label(), Some("Move Entity"));
        assert!(!history.can_redo());
        assert!(document.dirty);
    }

    #[test]
    fn saved_snapshot_controls_dirty_state() {
        let saved = snapshot("Root", 0.0);
        let changed = snapshot("Root", 3.0);
        let history = SceneHistory {
            saved: Some(saved.clone()),
            ..default()
        };
        let mut document = SceneDocument::default();

        history.update_dirty(&changed, &mut document);
        assert!(document.dirty);
        history.update_dirty(&saved, &mut document);
        assert!(!document.dirty);
    }

    #[test]
    fn selection_changes_do_not_make_scene_dirty() {
        let mut saved = snapshot("Root", 0.0);
        let child_id = SceneNodeId::parse("00000000000000000000000000000002").unwrap();
        saved.nodes.push(SceneNodeSnapshot {
            id: child_id,
            parent: Some(saved.nodes[0].id),
            order: 0,
            name: "Child".into(),
            kind: EntityKind::Empty2D,
            components: Vec::new(),
            custom_components: EntityCustomComponents::default(),
            entity_script: EntityScriptBinding::default(),
            systems: EntitySystemBindings::default(),
            space: SceneSpace::TwoD,
            transform: Some(Transform::default()),
            visibility: Some(Visibility::Visible),
            ui_layout: None,
            ui_content: None,
            sprite: None,
            model: None,
            animation_player: None,
            collision_rect: None,
            root: false,
        });
        let mut changed_selection = saved.clone();
        changed_selection.selected = Some(child_id);
        let history = SceneHistory {
            saved: Some(saved),
            ..default()
        };
        let mut document = SceneDocument::default();

        history.update_dirty(&changed_selection, &mut document);
        assert!(!document.dirty);
    }

    #[test]
    fn undo_and_redo_restore_scene_transform_and_dirty_state() {
        let mut app = test_app();
        let id = SceneNodeId::parse("00000000000000000000000000000001").unwrap();
        let root = app
            .world_mut()
            .spawn((
                SceneRoot,
                EditableObject {
                    name: "Root".into(),
                },
                id,
                SceneParentId(None),
                SceneSiblingOrder(0),
                SceneSpace::TwoD,
                Transform::default(),
                Visibility::Visible,
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(root);
        app.world_mut().resource_mut::<SceneDocument>().open = true;
        app.world_mut()
            .resource_mut::<SceneHistory>()
            .request_reset(true);
        app.update();

        app.world_mut()
            .resource_mut::<SceneHistory>()
            .begin("Edit Transform", snapshot("Root", 0.0));
        app.world_mut()
            .entity_mut(root)
            .get_mut::<Transform>()
            .unwrap()
            .translation
            .x = 4.0;
        app.update();
        assert!(app.world().resource::<SceneHistory>().can_undo());

        let button = app
            .world_mut()
            .spawn(HistoryActionButton(HistoryAction::Undo))
            .id();
        let redo_button = app
            .world_mut()
            .spawn(HistoryActionButton(HistoryAction::Redo))
            .id();
        app.world_mut().trigger(Activate { entity: button });
        app.world_mut().flush();
        let restored = app
            .world_mut()
            .query::<(&SceneNodeId, &Transform)>()
            .iter(app.world())
            .find(|(node_id, _)| **node_id == id)
            .unwrap()
            .1;
        assert_eq!(restored.translation.x, 0.0);
        assert!(!app.world().resource::<SceneDocument>().dirty);

        app.world_mut().trigger(Activate {
            entity: redo_button,
        });
        app.world_mut().flush();
        let restored = app
            .world_mut()
            .query::<(&SceneNodeId, &Transform)>()
            .iter(app.world())
            .find(|(node_id, _)| **node_id == id)
            .unwrap()
            .1;
        assert_eq!(restored.translation.x, 4.0);
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn hierarchy_add_node_is_recorded_and_undoable() {
        let mut app = test_app();
        app.add_plugins(crate::hierarchy::HierarchyPlugin);
        let option = app
            .world_mut()
            .spawn(crate::hierarchy::SceneRootOption(
                crate::hierarchy::RootNodeKind::TwoD,
            ))
            .id();
        app.world_mut().trigger(Activate { entity: option });
        let create = app
            .world_mut()
            .spawn(crate::hierarchy::SceneEntityPickerCreate)
            .id();
        app.world_mut().trigger(Activate { entity: create });
        app.world_mut().flush();
        app.update();
        let root = app.world().resource::<Selection>().0.unwrap();
        let root_id = *app.world().entity(root).get::<SceneNodeId>().unwrap();

        let add = app
            .world_mut()
            .spawn(crate::hierarchy::SceneEntityOption(EntityKind::Empty))
            .id();
        app.world_mut().trigger(Activate { entity: add });
        app.world_mut().trigger(Activate { entity: create });
        app.world_mut().flush();
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&EditableObject>()
                .iter(app.world())
                .count(),
            2
        );
        assert!(app.world().resource::<SceneHistory>().can_undo());

        let undo = app
            .world_mut()
            .spawn(HistoryActionButton(HistoryAction::Undo))
            .id();
        app.world_mut().trigger(Activate { entity: undo });
        app.world_mut().flush();

        assert_eq!(
            app.world_mut()
                .query::<&EditableObject>()
                .iter(app.world())
                .count(),
            1
        );
        let restored_root = app.world().resource::<Selection>().0.unwrap();
        assert!(app.world().entity(restored_root).contains::<SceneRoot>());
        assert_eq!(
            *app.world()
                .entity(restored_root)
                .get::<SceneNodeId>()
                .unwrap(),
            root_id
        );
    }
}
