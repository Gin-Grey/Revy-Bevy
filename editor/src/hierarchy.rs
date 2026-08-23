use std::collections::{HashMap, HashSet};

use bevy::{
    input_focus::{AutoFocus, InputFocus},
    picking::{Pickable, pointer::PointerButton},
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui::RelativeCursorPosition,
    ui_widgets::{Activate, SelectAllOnFocus},
};
use uuid::Uuid;

use crate::entities::{
    AddedEntityComponents, BuiltinComponent, EntityCustomComponents, EntityKind,
    EntityScriptBinding, EntitySystemBindings, SceneCollisionRect2D, SceneModel3D, SceneSprite2D,
    SceneUiContent, SceneUiLayout, insert_builtin_component, spawn_entity,
};
use crate::filesystem::{
    FileSystemState, InstantiateModelRequest, model_resource_path_from_filesystem,
};
use crate::scene::SceneDocument;
use crate::selection::{EditableObject, Selection, SelectionSet, select_entity};
use crate::ui::theme;
use crate::undo::{SceneHistory, SceneSnapshotQuery, capture_scene_snapshot};
use crate::workspace::{EditorViewMode, SceneSpace, WorkspaceMode, WorkspaceSelections};

/// Parent UI node that holds live scene-tree rows.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneTreeList;

/// A clickable hierarchy row bound to a scene entity.
#[derive(Component)]
pub struct SceneTreeRow {
    pub target: Entity,
}

/// Expand/collapse control displayed before a hierarchy node.
#[derive(Component, Clone, Copy, Debug)]
struct SceneTreeToggle {
    target: SceneNodeId,
}

/// Editor-only expansion state keyed by stable scene IDs, so rebuilding the
/// hierarchy UI or switching workspaces does not lose collapsed branches.
#[derive(Resource, Debug, Default)]
struct SceneTreeExpansionState {
    collapsed: HashSet<SceneNodeId>,
}

impl SceneTreeExpansionState {
    fn is_collapsed(&self, id: SceneNodeId) -> bool {
        self.collapsed.contains(&id)
    }

    fn toggle(&mut self, id: SceneNodeId) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
    }

    fn expand(&mut self, id: SceneNodeId) {
        self.collapsed.remove(&id);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SceneTreeDropPlacement {
    Before,
    #[default]
    Child,
    After,
}

#[derive(Resource, Debug)]
struct SceneTreeDragState {
    source: Option<Entity>,
    hovered_row: Option<Entity>,
    placement: SceneTreeDropPlacement,
    auto_expand_target: Option<SceneNodeId>,
    auto_expand_timer: Timer,
}

impl Default for SceneTreeDragState {
    fn default() -> Self {
        Self {
            source: None,
            hovered_row: None,
            placement: SceneTreeDropPlacement::Child,
            auto_expand_target: None,
            auto_expand_timer: Timer::from_seconds(0.55, TimerMode::Once),
        }
    }
}

impl SceneTreeDragState {
    fn arm_auto_expand(&mut self, target: SceneNodeId) {
        if self.auto_expand_target != Some(target) {
            self.auto_expand_target = Some(target);
            self.auto_expand_timer.reset();
        }
    }

    fn clear_auto_expand(&mut self) {
        self.auto_expand_target = None;
        self.auto_expand_timer.reset();
    }

    fn clear(&mut self) {
        self.source = None;
        self.hovered_row = None;
        self.clear_auto_expand();
    }
}

#[derive(Clone, Copy, Debug)]
struct HierarchyNodeState {
    entity: Entity,
    id: SceneNodeId,
    parent: Option<SceneNodeId>,
    order: u32,
    space: SceneSpace,
    kind: EntityKind,
    root: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HierarchyNodeUpdate {
    entity: Entity,
    parent: Option<SceneNodeId>,
    order: u32,
}

/// A selectable entity preset in the add-entity panel.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneEntityOption(pub EntityKind);

/// Context menu opened by right-clicking the Scene hierarchy.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneContextMenu;

/// Context-menu entry that opens the entity-system picker.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneContextAddEntity;

/// Closes the entity-system picker without creating anything.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneEntityPickerCancel;

/// Prevents the Scene hierarchy observer from being attached twice.
#[derive(Component, Clone, Copy, Default)]
struct SceneTreeInteractionReady;

/// Root entity created from the Scene panel.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneRoot;

/// Persistent identity stored in scene files. It never depends on an ECS entity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneNodeId(pub Uuid);

impl SceneNodeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|_| format!("invalid scene entity ID: {value}"))
    }
}

impl Default for SceneNodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SceneNodeId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0.simple())
    }
}

/// Stable parent reference. `None` is reserved for the document root.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneParentId(pub Option<SceneNodeId>);

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneSiblingOrder(pub u32);

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SceneNodeAction {
    #[default]
    Duplicate,
    MakeRoot,
    Rename,
    Delete,
}

#[derive(Component, Clone, Copy, Default)]
pub struct SceneNodeActionButton(pub SceneNodeAction);

#[derive(Component, Clone, Copy, Default)]
struct SceneNodeRenameInput;

#[derive(Resource, Debug, Default)]
struct RenameDialogState {
    open: bool,
    target: Option<SceneNodeId>,
    name: String,
    error: String,
    revision: u64,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneRootOption(pub RootNodeKind);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RootNodeKind {
    #[default]
    TwoD,
    ThreeD,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneEntityChoice {
    Root(RootNodeKind),
    Entity(EntityKind),
}

#[derive(Resource, Debug)]
pub(crate) struct SceneEntityPickerState {
    pub(crate) selected: Option<SceneEntityChoice>,
    pub(crate) search: String,
    pub(crate) recent: Vec<EntityKind>,
}

impl Default for SceneEntityPickerState {
    fn default() -> Self {
        Self {
            selected: None,
            search: String::new(),
            recent: vec![
                EntityKind::Empty2D,
                EntityKind::Empty3D,
                EntityKind::Sprite2D,
                EntityKind::Camera3D,
            ],
        }
    }
}

#[derive(Component, Clone, Copy, Default)]
pub struct SceneEntityPickerCreate;

#[derive(Resource, Debug)]
pub(crate) struct SceneRootMenuState {
    /// The entity-system picker is visible.
    pub(crate) open: bool,
    /// The right-click Scene context menu is visible.
    pub(crate) context_open: bool,
    /// Screen-space position of the context menu.
    pub(crate) context_position: Vec2,
}

impl Default for SceneRootMenuState {
    fn default() -> Self {
        Self {
            open: false,
            context_open: false,
            context_position: Vec2::ZERO,
        }
    }
}

impl SceneRootMenuState {
    pub(crate) fn any_open(&self) -> bool {
        self.open || self.context_open
    }
}

#[derive(Resource, Debug, Default)]
struct SceneContextMenuPointerState {
    armed: bool,
    layout_grace: bool,
}

impl SceneContextMenuPointerState {
    fn arm(&mut self) {
        self.armed = true;
        self.layout_grace = true;
    }

    fn reset(&mut self) {
        self.armed = false;
        self.layout_grace = false;
    }
}

#[derive(Clone, Debug)]
struct SceneClipboardNode {
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
    ui_layout: Option<SceneUiLayout>,
    ui_content: Option<SceneUiContent>,
    sprite: Option<SceneSprite2D>,
    model: Option<SceneModel3D>,
    collision_rect: Option<SceneCollisionRect2D>,
}

#[derive(Resource, Clone, Debug, Default)]
struct SceneClipboard {
    roots: Vec<SceneNodeId>,
    nodes: Vec<SceneClipboardNode>,
}

#[derive(bevy::ecs::query::QueryData)]
struct SceneBatchNode {
    entity: Entity,
    object: &'static EditableObject,
    id: &'static SceneNodeId,
    parent: &'static SceneParentId,
    order: &'static SceneSiblingOrder,
    space: &'static SceneSpace,
    transform: Option<&'static Transform>,
    kind: Option<&'static EntityKind>,
    components: Option<&'static AddedEntityComponents>,
    custom_components: Option<&'static EntityCustomComponents>,
    entity_script: Option<&'static EntityScriptBinding>,
    systems: Option<&'static EntitySystemBindings>,
    ui_layout: Option<&'static SceneUiLayout>,
    ui_content: Option<&'static SceneUiContent>,
    sprite: Option<&'static SceneSprite2D>,
    model: Option<&'static SceneModel3D>,
    collision_rect: Option<&'static SceneCollisionRect2D>,
    root: Option<&'static SceneRoot>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SceneBatchShortcut {
    Copy,
    Cut,
    Paste,
    Duplicate,
    Delete,
    Nudge(Vec2),
}

pub struct HierarchyPlugin;

impl Plugin for HierarchyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneRootMenuState>()
            .init_resource::<SceneEntityPickerState>()
            .init_resource::<SceneContextMenuPointerState>()
            .init_resource::<SceneTreeExpansionState>()
            .init_resource::<SceneTreeDragState>()
            .init_resource::<RenameDialogState>()
            .init_resource::<SceneClipboard>()
            .init_resource::<EditorViewMode>()
            .init_resource::<SelectionSet>()
            .init_resource::<FileSystemState>()
            .add_message::<InstantiateModelRequest>()
            .add_observer(handle_scene_root_action)
            .add_observer(handle_scene_node_action)
            .add_systems(
                Update,
                (
                    (
                        crate::entities::ensure_entity_defaults,
                        crate::entities::clear_model3d_placeholders,
                        crate::entities::sync_model3d_assets,
                    )
                        .chain(),
                    attach_scene_tree_interaction,
                    handle_scene_node_shortcuts,
                    handle_scene_batch_shortcuts,
                    handle_instantiate_model_requests,
                    handle_scene_tree_keyboard_navigation,
                    auto_expand_scene_tree_drag_target,
                    rebuild_scene_tree,
                    highlight_scene_tree_rows,
                    sync_scene_root_menu,
                    close_scene_context_menu_on_cursor_exit,
                    close_scene_menus_on_escape,
                    sync_rename_input,
                    handle_inline_rename_keyboard,
                ),
            );
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_instantiate_model_requests(
    mut requests: MessageReader<InstantiateModelRequest>,
    roots: Query<(Entity, &SceneNodeId, &SceneSpace), With<SceneRoot>>,
    objects: Query<(
        Entity,
        &EditableObject,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
        &SceneSpace,
    )>,
    history_nodes: Query<SceneSnapshotQuery>,
    mut selection: ResMut<Selection>,
    mut selection_set: ResMut<SelectionSet>,
    mut saved_selections: ResMut<WorkspaceSelections>,
    mut mode: ResMut<WorkspaceMode>,
    mut view: ResMut<EditorViewMode>,
    mut document: Option<ResMut<SceneDocument>>,
    mut history: Option<ResMut<SceneHistory>>,
    mut filesystem: ResMut<FileSystemState>,
    mut commands: Commands,
) {
    for request in requests.read() {
        let relative = request.relative_path.to_string_lossy().replace('\\', "/");
        let resource_path = match model_resource_path_from_filesystem(&relative) {
            Ok(path) => path,
            Err(error) => {
                filesystem.status = format!("FBX import failed: {error}");
                filesystem.revision = filesystem.revision.wrapping_add(1);
                continue;
            }
        };

        let existing_root = roots.iter().next();
        if existing_root.is_some_and(|(_, _, space)| *space != SceneSpace::ThreeD) {
            filesystem.status = "FBX models can only be instantiated in a 3D scene.".into();
            filesystem.revision = filesystem.revision.wrapping_add(1);
            continue;
        }
        if let Some(history) = history.as_deref_mut() {
            history.begin(
                "Instantiate FBX",
                capture_scene_snapshot(&history_nodes, &selection, *mode),
            );
        }

        let (root_entity, root_id) = if let Some((entity, id, _)) = existing_root {
            (entity, *id)
        } else {
            let id = SceneNodeId::new();
            let root = spawn_entity(
                &mut commands,
                EntityKind::Empty3D,
                EntityKind::Empty3D.default_name().into(),
                id,
                None,
                0,
                SceneSpace::ThreeD,
            );
            commands.entity(root).insert(SceneRoot);
            (root, id)
        };

        let parent_id = selection
            .0
            .and_then(|entity| objects.get(entity).ok())
            .filter(|(_, _, _, _, _, space)| **space == SceneSpace::ThreeD)
            .map(|(_, _, id, _, _, _)| *id)
            .unwrap_or(root_id);
        let next_order = objects
            .iter()
            .filter(|(_, _, _, parent, _, _)| parent.0 == Some(parent_id))
            .map(|(_, _, _, _, order, _)| order.0)
            .max()
            .map_or(0, |order| order.saturating_add(1));
        let base_name = request
            .relative_path
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("FBXModel");
        let name = unique_name(
            base_name,
            objects.iter().map(|(_, object, _, _, _, _)| &object.name),
        );
        let model_entity = spawn_entity(
            &mut commands,
            EntityKind::Mesh3D,
            name,
            SceneNodeId::new(),
            Some(parent_id),
            next_order,
            SceneSpace::ThreeD,
        );
        commands.entity(model_entity).insert(SceneModel3D {
            resource_path: resource_path.clone(),
        });

        *mode = WorkspaceMode::ThreeD;
        *view = EditorViewMode::ThreeD;
        selection_set.select_only(&mut selection, model_entity);
        saved_selections.set(WorkspaceMode::ThreeD, Some(model_entity));
        if let Some(document) = document.as_deref_mut() {
            if existing_root.is_none() {
                document.path = None;
                document.name = "Untitled Level".into();
            }
            mark_document_changed(Some(document));
        }
        filesystem.selected = Some(relative);
        filesystem.status = format!("Instantiated {resource_path}");
        filesystem.revision = filesystem.revision.wrapping_add(1);
        let _ = root_entity;
    }
}

fn attach_scene_tree_interaction(
    mut commands: Commands,
    lists: Query<Entity, (With<SceneTreeList>, Without<SceneTreeInteractionReady>)>,
) {
    for entity in &lists {
        commands
            .entity(entity)
            .insert(SceneTreeInteractionReady)
            .observe(on_scene_tree_click);
    }
}

fn on_scene_tree_click(
    click: On<Pointer<Click>>,
    rows: Query<&SceneTreeRow>,
    mut menu: ResMut<SceneRootMenuState>,
    mut menu_pointer: ResMut<SceneContextMenuPointerState>,
) {
    // Row clicks bubble to the tree. The row observer owns those so that the
    // blank Scene workspace and an entity row have one unambiguous handler.
    if rows.get(click.entity).is_ok() {
        return;
    }
    match click.button {
        PointerButton::Secondary => {
            open_scene_context_menu(
                &mut menu,
                &mut menu_pointer,
                click.pointer_location.position,
            );
        }
        PointerButton::Primary => {
            menu.context_open = false;
            menu_pointer.reset();
        }
        _ => {}
    }
}

fn open_scene_context_menu(
    menu: &mut SceneRootMenuState,
    pointer: &mut SceneContextMenuPointerState,
    position: Vec2,
) {
    menu.open = false;
    menu.context_open = true;
    menu.context_position = position;
    pointer.arm();
}

fn close_scene_menus_on_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<SceneRootMenuState>,
) {
    if keyboard.just_pressed(KeyCode::Escape) && menu.any_open() {
        menu.open = false;
        menu.context_open = false;
    }
}

fn handle_scene_node_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    rename: Res<RenameDialogState>,
    buttons: Query<(Entity, &SceneNodeActionButton)>,
    mut commands: Commands,
) {
    if rename.open
        || focus
            .as_deref()
            .and_then(InputFocus::get)
            .is_some_and(|entity| editable_text.get(entity).is_ok())
    {
        return;
    }
    let action = if keyboard.just_pressed(KeyCode::F2) {
        Some(SceneNodeAction::Rename)
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

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_scene_batch_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    rename: Res<RenameDialogState>,
    mode: Res<WorkspaceMode>,
    mut clipboard: ResMut<SceneClipboard>,
    mut selection: ResMut<Selection>,
    mut selection_set: ResMut<SelectionSet>,
    mut saved_selections: ResMut<WorkspaceSelections>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut commands: Commands,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<SceneBatchNode>,
        Query<&mut Transform>,
        Query<&mut SceneUiLayout>,
    )>,
) {
    if rename.open
        || focus
            .as_deref()
            .and_then(InputFocus::get)
            .is_some_and(|entity| editable_text.get(entity).is_ok())
    {
        return;
    }

    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    let shortcut = if control && keyboard.just_pressed(KeyCode::KeyC) {
        Some(SceneBatchShortcut::Copy)
    } else if control && keyboard.just_pressed(KeyCode::KeyX) {
        Some(SceneBatchShortcut::Cut)
    } else if control && keyboard.just_pressed(KeyCode::KeyV) {
        Some(SceneBatchShortcut::Paste)
    } else if control && keyboard.just_pressed(KeyCode::KeyD) {
        Some(SceneBatchShortcut::Duplicate)
    } else if keyboard.just_pressed(KeyCode::Delete) {
        Some(SceneBatchShortcut::Delete)
    } else if !control && !alt && *mode == WorkspaceMode::TwoD {
        let amount = if shift { 10.0 } else { 1.0 };
        if keyboard.just_pressed(KeyCode::ArrowLeft) {
            Some(SceneBatchShortcut::Nudge(Vec2::new(-amount, 0.0)))
        } else if keyboard.just_pressed(KeyCode::ArrowRight) {
            Some(SceneBatchShortcut::Nudge(Vec2::new(amount, 0.0)))
        } else if keyboard.just_pressed(KeyCode::ArrowUp) {
            Some(SceneBatchShortcut::Nudge(Vec2::new(0.0, -amount)))
        } else if keyboard.just_pressed(KeyCode::ArrowDown) {
            Some(SceneBatchShortcut::Nudge(Vec2::new(0.0, amount)))
        } else {
            None
        }
    } else {
        None
    };
    let Some(shortcut) = shortcut else { return };

    match shortcut {
        SceneBatchShortcut::Copy => {
            let next = {
                let objects = nodes.p1();
                selected_clipboard(&selection, &selection_set, &objects)
            };
            if let Some(next) = next {
                *clipboard = next;
            }
        }
        SceneBatchShortcut::Cut | SceneBatchShortcut::Delete => {
            let (next_clipboard, plan) = {
                let objects = nodes.p1();
                let roots = selected_top_level_entities(&selection, &selection_set, &objects);
                let copied = (shortcut == SceneBatchShortcut::Cut)
                    .then(|| capture_clipboard(&roots, &objects));
                (copied, deletion_plan(&roots, &objects))
            };
            let Some(plan) = plan else { return };
            let before = {
                let history_nodes = nodes.p0();
                capture_scene_snapshot(&history_nodes, &selection, *mode)
            };
            if let Some(history) = history.as_deref_mut() {
                history.begin(
                    if shortcut == SceneBatchShortcut::Cut {
                        "Cut Entities"
                    } else {
                        "Delete Entities"
                    },
                    before,
                );
            }
            if let Some(Some(next)) = next_clipboard {
                *clipboard = next;
            }
            for entity in plan.entities {
                commands.entity(entity).despawn();
            }
            set_batch_selection(
                plan.fallback.into_iter(),
                &mut selection,
                &mut selection_set,
                &mut saved_selections,
                *mode,
            );
            mark_document_changed(document.as_deref_mut());
        }
        SceneBatchShortcut::Paste | SceneBatchShortcut::Duplicate => {
            let source = if shortcut == SceneBatchShortcut::Paste {
                (!clipboard.nodes.is_empty()).then(|| clipboard.clone())
            } else {
                let objects = nodes.p1();
                selected_clipboard(&selection, &selection_set, &objects)
            };
            let Some(source) = source else { return };
            let before = {
                let history_nodes = nodes.p0();
                capture_scene_snapshot(&history_nodes, &selection, *mode)
            };
            let pasted = {
                let objects = nodes.p1();
                paste_clipboard(&source, &objects, &mut commands)
            };
            if pasted.is_empty() {
                return;
            }
            if let Some(history) = history.as_deref_mut() {
                history.begin(
                    if shortcut == SceneBatchShortcut::Paste {
                        "Paste Entities"
                    } else {
                        "Duplicate Entities"
                    },
                    before,
                );
            }
            set_batch_selection(
                pasted,
                &mut selection,
                &mut selection_set,
                &mut saved_selections,
                *mode,
            );
            mark_document_changed(document.as_deref_mut());
        }
        SceneBatchShortcut::Nudge(screen_delta) => {
            let targets = {
                let objects = nodes.p1();
                selected_top_level_entities(&selection, &selection_set, &objects)
                    .into_iter()
                    .filter_map(|entity| {
                        objects
                            .get(entity)
                            .ok()
                            .filter(|node| *node.space == SceneSpace::TwoD)
                            .map(|node| (entity, node.ui_layout.is_some()))
                    })
                    .collect::<Vec<_>>()
            };
            if targets.is_empty() {
                return;
            }
            let before = {
                let history_nodes = nodes.p0();
                capture_scene_snapshot(&history_nodes, &selection, *mode)
            };
            if let Some(history) = history.as_deref_mut() {
                history.begin("Move Entities", before);
            }
            let mut changed = false;
            for (entity, ui) in targets {
                if ui {
                    let mut layouts = nodes.p3();
                    if let Ok(mut layout) = layouts.get_mut(entity) {
                        apply_ui_nudge(&mut layout, screen_delta);
                        changed = true;
                    }
                } else {
                    let mut transforms = nodes.p2();
                    if let Ok(mut transform) = transforms.get_mut(entity) {
                        transform.translation.x += screen_delta.x;
                        transform.translation.y -= screen_delta.y;
                        changed = true;
                    }
                }
            }
            if changed {
                mark_document_changed(document.as_deref_mut());
            }
        }
    }
}

fn set_batch_selection(
    entities: impl IntoIterator<Item = Entity>,
    selection: &mut Selection,
    selection_set: &mut SelectionSet,
    saved_selections: &mut WorkspaceSelections,
    mode: WorkspaceMode,
) {
    selection_set.select_many(selection, entities);
    saved_selections.set(mode, selection.0);
}

fn apply_ui_nudge(layout: &mut SceneUiLayout, delta: Vec2) {
    layout.offset.0 += delta.x;
    layout.offset.1 += delta.y;
    if layout.anchor_max.0 > layout.anchor_min.0 {
        layout.margin.2 -= delta.x;
    }
    if layout.anchor_max.1 > layout.anchor_min.1 {
        layout.margin.3 -= delta.y;
    }
}

fn selected_clipboard(
    selection: &Selection,
    selection_set: &SelectionSet,
    objects: &Query<SceneBatchNode>,
) -> Option<SceneClipboard> {
    let roots = selected_top_level_entities(selection, selection_set, objects);
    capture_clipboard(&roots, objects)
}

fn selected_top_level_entities(
    selection: &Selection,
    selection_set: &SelectionSet,
    objects: &Query<SceneBatchNode>,
) -> Vec<Entity> {
    let selected: Vec<_> = selection_set
        .entities(selection)
        .filter(|entity| objects.get(*entity).is_ok_and(|node| node.root.is_none()))
        .collect();
    let selected_ids: HashSet<_> = selected
        .iter()
        .filter_map(|entity| objects.get(*entity).ok().map(|node| *node.id))
        .collect();
    let parents: HashMap<_, _> = objects
        .iter()
        .map(|node| (*node.id, node.parent.0))
        .collect();
    selected
        .into_iter()
        .filter(|entity| {
            let mut parent = objects.get(*entity).ok().and_then(|node| node.parent.0);
            while let Some(parent_id) = parent {
                if selected_ids.contains(&parent_id) {
                    return false;
                }
                parent = parents.get(&parent_id).copied().flatten();
            }
            true
        })
        .collect()
}

fn capture_clipboard(roots: &[Entity], objects: &Query<SceneBatchNode>) -> Option<SceneClipboard> {
    let root_ids: Vec<_> = roots
        .iter()
        .filter_map(|entity| objects.get(*entity).ok().map(|node| *node.id))
        .collect();
    if root_ids.is_empty() {
        return None;
    }
    let mut included: HashSet<_> = root_ids.iter().copied().collect();
    loop {
        let before = included.len();
        for node in objects.iter() {
            if node
                .parent
                .0
                .is_some_and(|parent| included.contains(&parent))
            {
                included.insert(*node.id);
            }
        }
        if included.len() == before {
            break;
        }
    }
    let nodes = objects
        .iter()
        .filter(|node| included.contains(node.id))
        .map(|node| SceneClipboardNode {
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
            transform: node.transform.copied(),
            ui_layout: node.ui_layout.copied(),
            ui_content: node.ui_content.cloned(),
            sprite: node.sprite.cloned(),
            model: node.model.cloned(),
            collision_rect: node.collision_rect.copied(),
        })
        .collect();
    Some(SceneClipboard {
        roots: root_ids,
        nodes,
    })
}

struct SceneDeletionPlan {
    entities: Vec<Entity>,
    fallback: Option<Entity>,
}

fn deletion_plan(roots: &[Entity], objects: &Query<SceneBatchNode>) -> Option<SceneDeletionPlan> {
    let root_ids: Vec<_> = roots
        .iter()
        .filter_map(|entity| objects.get(*entity).ok().map(|node| *node.id))
        .collect();
    let mut deleted: HashSet<_> = root_ids.iter().copied().collect();
    if deleted.is_empty() {
        return None;
    }
    loop {
        let before = deleted.len();
        for node in objects.iter() {
            if node
                .parent
                .0
                .is_some_and(|parent| deleted.contains(&parent))
            {
                deleted.insert(*node.id);
            }
        }
        if deleted.len() == before {
            break;
        }
    }
    let preferred_parent = roots
        .last()
        .and_then(|entity| objects.get(*entity).ok())
        .and_then(|node| node.parent.0);
    let fallback = preferred_parent
        .and_then(|parent| {
            objects
                .iter()
                .find(|node| *node.id == parent && !deleted.contains(node.id))
                .map(|node| node.entity)
        })
        .or_else(|| {
            objects
                .iter()
                .find(|node| node.root.is_some())
                .map(|node| node.entity)
        });
    Some(SceneDeletionPlan {
        entities: objects
            .iter()
            .filter(|node| deleted.contains(node.id))
            .map(|node| node.entity)
            .collect(),
        fallback,
    })
}

fn paste_clipboard(
    clipboard: &SceneClipboard,
    objects: &Query<SceneBatchNode>,
    commands: &mut Commands,
) -> Vec<Entity> {
    let id_map: HashMap<_, _> = clipboard
        .nodes
        .iter()
        .map(|node| (node.id, SceneNodeId::new()))
        .collect();
    let mut root_parents = HashMap::new();
    for root_id in &clipboard.roots {
        let Some(root) = clipboard.nodes.iter().find(|node| node.id == *root_id) else {
            return Vec::new();
        };
        let parent = root
            .parent
            .filter(|parent| {
                objects.iter().any(|node| {
                    *node.id == *parent
                        && *node.space == root.space
                        && (!root.kind.is_ui()
                            || node.root.is_some()
                            || node.kind.is_some_and(|kind| kind.is_ui()))
                })
            })
            .or_else(|| {
                objects
                    .iter()
                    .find(|node| node.root.is_some() && *node.space == root.space)
                    .map(|node| *node.id)
            });
        let Some(parent) = parent else {
            return Vec::new();
        };
        root_parents.insert(*root_id, parent);
    }

    let mut next_orders = HashMap::new();
    let mut root_orders = HashMap::new();
    for root_id in &clipboard.roots {
        let parent = root_parents[root_id];
        let next = next_orders.entry(parent).or_insert_with(|| {
            objects
                .iter()
                .filter(|node| node.parent.0 == Some(parent))
                .map(|node| node.order.0)
                .max()
                .map_or(0, |order| order.saturating_add(1))
        });
        root_orders.insert(*root_id, *next);
        *next = next.saturating_add(1);
    }

    let mut names: Vec<_> = objects
        .iter()
        .map(|node| node.object.name.clone())
        .collect();
    let mut root_names = HashMap::new();
    for root_id in &clipboard.roots {
        let root = clipboard
            .nodes
            .iter()
            .find(|node| node.id == *root_id)
            .unwrap();
        let name = unique_name(&root.name, names.iter());
        names.push(name.clone());
        root_names.insert(*root_id, name);
    }

    let roots: HashSet<_> = clipboard.roots.iter().copied().collect();
    let mut spawned_roots = HashMap::new();
    for node in &clipboard.nodes {
        let is_root = roots.contains(&node.id);
        let new_entity = spawn_entity(
            commands,
            node.kind,
            root_names
                .get(&node.id)
                .cloned()
                .unwrap_or_else(|| node.name.clone()),
            id_map[&node.id],
            if is_root {
                Some(root_parents[&node.id])
            } else {
                node.parent.and_then(|parent| id_map.get(&parent).copied())
            },
            if is_root {
                root_orders[&node.id]
            } else {
                node.order
            },
            node.space,
        );
        for component in node.components.iter().copied() {
            insert_builtin_component(commands, new_entity, component);
        }
        commands.entity(new_entity).insert((
            AddedEntityComponents(node.components.clone()),
            node.custom_components.clone(),
            node.entity_script.clone(),
            node.systems.clone(),
        ));
        if let Some(transform) = node.transform {
            commands
                .entity(new_entity)
                .insert((transform, Visibility::Visible));
        }
        if let Some(layout) = node.ui_layout {
            commands.entity(new_entity).insert(layout);
        }
        if let Some(content) = node.ui_content.clone() {
            commands.entity(new_entity).insert(content);
        }
        if let Some(sprite) = node.sprite.clone() {
            commands.entity(new_entity).insert(sprite);
        }
        if let Some(model) = node.model.clone() {
            commands.entity(new_entity).insert(model);
        }
        if let Some(collision) = node.collision_rect {
            commands.entity(new_entity).insert(collision);
        }
        if is_root {
            spawned_roots.insert(node.id, new_entity);
        }
    }
    clipboard
        .roots
        .iter()
        .filter_map(|root| spawned_roots.get(root).copied())
        .collect()
}

#[allow(clippy::type_complexity)]
fn handle_scene_tree_keyboard_navigation(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    rename: Res<RenameDialogState>,
    objects: Query<(
        Entity,
        &EditableObject,
        &EntityKind,
        &SceneSpace,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
    )>,
    mode: Res<WorkspaceMode>,
    mut expansion: ResMut<SceneTreeExpansionState>,
    mut selection: ResMut<Selection>,
    mut saved_selections: ResMut<WorkspaceSelections>,
) {
    if rename.open
        || focus
            .as_deref()
            .and_then(InputFocus::get)
            .is_some_and(|entity| editable_text.get(entity).is_ok())
        || keyboard.pressed(KeyCode::ControlLeft)
        || keyboard.pressed(KeyCode::ControlRight)
        || (!keyboard.pressed(KeyCode::AltLeft) && !keyboard.pressed(KeyCode::AltRight))
    {
        return;
    }
    let action = if keyboard.just_pressed(KeyCode::ArrowUp) {
        Some(KeyCode::ArrowUp)
    } else if keyboard.just_pressed(KeyCode::ArrowDown) {
        Some(KeyCode::ArrowDown)
    } else if keyboard.just_pressed(KeyCode::ArrowLeft) {
        Some(KeyCode::ArrowLeft)
    } else if keyboard.just_pressed(KeyCode::ArrowRight) {
        Some(KeyCode::ArrowRight)
    } else {
        None
    };
    let Some(action) = action else {
        return;
    };

    let entries: Vec<_> = objects
        .iter()
        .filter(|(_, _, _, space, _, _, _)| match *mode {
            WorkspaceMode::TwoD => **space == SceneSpace::TwoD,
            WorkspaceMode::ThreeD => **space == SceneSpace::ThreeD,
        })
        .map(|(entity, object, kind, _, id, parent, order)| TreeEntry {
            entity,
            name: object.name.clone(),
            kind: *kind,
            id: *id,
            parent: parent.0,
            order: order.0,
        })
        .collect();
    let flattened = flatten_tree(&entries, &expansion.collapsed);
    if flattened.is_empty() {
        return;
    }

    let selected_index: usize = selection
        .0
        .and_then(|selected| {
            flattened
                .iter()
                .position(|(entry, _, _)| entry.entity == selected)
        })
        .unwrap_or(0);
    let (selected_entry, selected_depth, has_children) = flattened[selected_index];
    let next_selection = match action {
        KeyCode::ArrowUp => Some(flattened[selected_index.saturating_sub(1)].0.entity),
        KeyCode::ArrowDown => Some(
            flattened[(selected_index + 1).min(flattened.len().saturating_sub(1))]
                .0
                .entity,
        ),
        KeyCode::ArrowLeft => {
            if has_children && !expansion.is_collapsed(selected_entry.id) {
                expansion.collapsed.insert(selected_entry.id);
                None
            } else {
                selected_entry.parent.and_then(|parent_id| {
                    entries
                        .iter()
                        .find(|entry| entry.id == parent_id)
                        .map(|entry| entry.entity)
                })
            }
        }
        KeyCode::ArrowRight => {
            if has_children && expansion.is_collapsed(selected_entry.id) {
                expansion.expand(selected_entry.id);
                None
            } else {
                flattened
                    .get(selected_index + 1)
                    .filter(|(_, depth, _)| *depth == selected_depth.saturating_add(1))
                    .map(|(entry, _, _)| entry.entity)
            }
        }
        _ => None,
    };
    if let Some(entity) = next_selection {
        select_entity(entity, &mut selection);
        saved_selections.set(*mode, Some(entity));
    }
}

fn handle_scene_root_action(
    activate: On<Activate>,
    picker_controls: Query<(
        Has<SceneContextAddEntity>,
        Has<SceneEntityPickerCancel>,
        Has<SceneEntityPickerCreate>,
    )>,
    options: Query<&SceneRootOption>,
    entity_options: Query<&SceneEntityOption>,
    mut menu: ResMut<SceneRootMenuState>,
    mut picker: ResMut<SceneEntityPickerState>,
    mut mode: ResMut<WorkspaceMode>,
    mut view: ResMut<EditorViewMode>,
    mut selection: ResMut<Selection>,
    mut saved_selections: ResMut<WorkspaceSelections>,
    roots: Query<(Entity, &SceneNodeId), With<SceneRoot>>,
    objects: Query<(
        Entity,
        &EditableObject,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
        &SceneSpace,
        Option<&EntityKind>,
    )>,
    all_objects: Query<Entity, With<EditableObject>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut history: Option<ResMut<SceneHistory>>,
    history_nodes: Query<SceneSnapshotQuery>,
    mut commands: Commands,
) {
    let (is_context_add, is_picker_cancel, is_picker_create) =
        picker_controls.get(activate.entity).unwrap_or_default();
    if is_context_add {
        menu.context_open = false;
        menu.open = true;
        picker.selected = None;
        picker.search.clear();
        return;
    }
    if is_picker_cancel {
        menu.open = false;
        return;
    }

    if let Ok(option) = entity_options.get(activate.entity) {
        picker.selected = Some(SceneEntityChoice::Entity(option.0));
        return;
    }
    if let Ok(option) = options.get(activate.entity) {
        picker.selected = Some(SceneEntityChoice::Root(option.0));
        return;
    }
    if !is_picker_create {
        return;
    }

    let Some(choice) = picker.selected else {
        return;
    };
    if let SceneEntityChoice::Entity(kind) = choice {
        let Some((root_entity, _)) = roots.iter().next() else {
            return;
        };
        let selected_space = selection
            .0
            .and_then(|entity| objects.get(entity).ok())
            .map(|(_, _, _, _, _, space, _)| *space);
        let inherited_space = selected_space.unwrap_or(match *mode {
            WorkspaceMode::TwoD => SceneSpace::TwoD,
            WorkspaceMode::ThreeD => SceneSpace::ThreeD,
        });
        let entity_space = kind.default_space().unwrap_or(inherited_space);
        let requested_parent = selection
            .0
            .filter(|entity| {
                objects
                    .get(*entity)
                    .is_ok_and(|(_, _, _, _, _, space, _)| *space == entity_space)
            })
            .unwrap_or(root_entity);
        let parent_entity = if kind.is_ui()
            && requested_parent != root_entity
            && objects
                .get(requested_parent)
                .ok()
                .and_then(|(_, _, _, _, _, _, kind)| kind.copied())
                .is_none_or(|parent_kind| !parent_kind.is_ui())
        {
            root_entity
        } else {
            requested_parent
        };
        let Ok((_, _, parent_id, _, _, _, _)) = objects.get(parent_entity) else {
            return;
        };
        if let Some(history) = history.as_deref_mut() {
            history.begin(
                "Add Entity",
                capture_scene_snapshot(&history_nodes, &selection, *mode),
            );
        }
        let next_order = objects
            .iter()
            .filter(|(_, _, _, parent, _, _, _)| parent.0 == Some(*parent_id))
            .map(|(_, _, _, _, order, _, _)| order.0)
            .max()
            .map_or(0, |order| order.saturating_add(1));
        let name = unique_name(
            kind.default_name(),
            objects
                .iter()
                .map(|(_, object, _, _, _, _, _)| &object.name),
        );
        let child = spawn_entity(
            &mut commands,
            kind,
            name,
            SceneNodeId::new(),
            Some(*parent_id),
            next_order,
            entity_space,
        );
        let next_mode = match entity_space {
            SceneSpace::TwoD => WorkspaceMode::TwoD,
            SceneSpace::ThreeD => WorkspaceMode::ThreeD,
        };
        *mode = next_mode;
        *view = match next_mode {
            WorkspaceMode::TwoD => EditorViewMode::TwoD,
            WorkspaceMode::ThreeD => EditorViewMode::ThreeD,
        };
        selection.0 = Some(child);
        saved_selections.set(next_mode, Some(child));
        picker.recent.retain(|recent| *recent != kind);
        picker.recent.insert(0, kind);
        picker.recent.truncate(8);
        menu.open = false;
        menu.context_open = false;
        mark_document_changed(document.as_deref_mut());
        return;
    }

    let SceneEntityChoice::Root(root_kind) = choice else {
        unreachable!();
    };

    // A scene has one document root. Child-node creation will be added later;
    // avoid silently producing a second root while the document model is empty.
    if roots.iter().next().is_some() {
        menu.open = false;
        menu.context_open = false;
        return;
    }

    for entity in &all_objects {
        commands.entity(entity).despawn();
    }

    let (space, kind, next_mode) = match root_kind {
        RootNodeKind::TwoD => (SceneSpace::TwoD, EntityKind::Empty2D, WorkspaceMode::TwoD),
        RootNodeKind::ThreeD => (
            SceneSpace::ThreeD,
            EntityKind::Empty3D,
            WorkspaceMode::ThreeD,
        ),
    };
    let root = spawn_entity(
        &mut commands,
        kind,
        kind.default_name().into(),
        SceneNodeId::new(),
        None,
        0,
        space,
    );
    commands.entity(root).insert(SceneRoot);

    *mode = next_mode;
    *view = match next_mode {
        WorkspaceMode::TwoD => EditorViewMode::TwoD,
        WorkspaceMode::ThreeD => EditorViewMode::ThreeD,
    };
    selection.0 = Some(root);
    saved_selections.set(next_mode, Some(root));
    menu.open = false;
    menu.context_open = false;
    if let Some(document) = document.as_deref_mut() {
        document.open = true;
        document.dirty = true;
        document.path = None;
        document.name = "Untitled Level".into();
        document.bump_revision();
    }
    if let Some(history) = history.as_deref_mut() {
        history.request_reset(false);
    }
}

fn unique_name<'a>(base: &str, existing: impl Iterator<Item = &'a String>) -> String {
    let existing: HashSet<_> = existing.map(String::as_str).collect();
    if !existing.contains(base) {
        return base.to_owned();
    }
    (2..)
        .map(|suffix| format!("{base}{suffix}"))
        .find(|candidate| !existing.contains(candidate.as_str()))
        .unwrap()
}

fn mark_document_changed(document: Option<&mut SceneDocument>) {
    if let Some(document) = document {
        document.open = true;
        document.dirty = true;
        document.bump_revision();
    }
}

#[allow(clippy::type_complexity)]
fn handle_scene_node_action(
    activate: On<Activate>,
    action_buttons: Query<&SceneNodeActionButton>,
    mut rename: ResMut<RenameDialogState>,
    mut root_menu: ResMut<SceneRootMenuState>,
    mut selection: ResMut<Selection>,
    mut saved_selections: ResMut<WorkspaceSelections>,
    mode: Res<WorkspaceMode>,
    mut document: Option<ResMut<SceneDocument>>,
    mut history: Option<ResMut<SceneHistory>>,
    history_nodes: Query<SceneSnapshotQuery>,
    objects: Query<(
        Entity,
        &EditableObject,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
        &SceneSpace,
        Option<&Transform>,
        Option<&EntityKind>,
        Option<&AddedEntityComponents>,
        Option<&EntitySystemBindings>,
        Option<&SceneRoot>,
    )>,
    resources: Query<(
        Option<&EntityCustomComponents>,
        Option<&EntityScriptBinding>,
        Option<&SceneUiLayout>,
        Option<&SceneUiContent>,
        Option<&SceneSprite2D>,
        Option<&SceneModel3D>,
        Option<&SceneCollisionRect2D>,
    )>,
    mut commands: Commands,
) {
    let Ok(button) = action_buttons.get(activate.entity) else {
        return;
    };
    root_menu.context_open = false;
    let Some(selected_entity) = selection.0 else {
        return;
    };
    let Ok((_, selected_object, selected_id, selected_parent, selected_order, _, _, _, _, _, root)) =
        objects.get(selected_entity)
    else {
        return;
    };

    match button.0 {
        SceneNodeAction::MakeRoot => {
            if root.is_some() || selected_parent.0.is_none() {
                return;
            }

            let Some((old_root_entity, _, old_root_id, ..)) = objects
                .iter()
                .find(|(_, _, _, _, _, _, _, _, _, _, root)| root.is_some())
            else {
                return;
            };

            // Follow the selected node's ancestor chain up to the current root.
            // Reversing every edge keeps all entities connected while making the
            // selected entity the document's one and only root.
            let mut path = vec![(selected_entity, *selected_id)];
            let mut next_parent = selected_parent.0;
            let mut seen = HashSet::from([*selected_id]);
            while let Some(parent_id) = next_parent {
                if !seen.insert(parent_id) {
                    return;
                }
                let Some((parent_entity, _, id, parent, ..)) = objects
                    .iter()
                    .find(|(_, _, id, _, _, _, _, _, _, _, _)| **id == parent_id)
                else {
                    return;
                };
                path.push((parent_entity, *id));
                if parent_entity == old_root_entity {
                    break;
                }
                next_parent = parent.0;
            }
            if path.last().map(|(_, id)| *id) != Some(*old_root_id) {
                return;
            }

            if let Some(history) = history.as_deref_mut() {
                history.begin(
                    "Set Scene Root",
                    capture_scene_snapshot(&history_nodes, &selection, *mode),
                );
            }

            commands.entity(selected_entity).insert((
                SceneRoot,
                SceneParentId(None),
                SceneSiblingOrder(0),
            ));
            commands.entity(old_root_entity).remove::<SceneRoot>();

            for edge in path.windows(2) {
                let (_, new_parent_id) = edge[0];
                let (ancestor_entity, _) = edge[1];
                let next_order = objects
                    .iter()
                    .filter(|(_, _, _, parent, _, _, _, _, _, _, _)| {
                        parent.0 == Some(new_parent_id)
                    })
                    .map(|(_, _, _, _, order, _, _, _, _, _, _)| order.0)
                    .max()
                    .map_or(0, |order| order.saturating_add(1));
                commands.entity(ancestor_entity).insert((
                    SceneParentId(Some(new_parent_id)),
                    SceneSiblingOrder(next_order),
                ));
            }
            mark_document_changed(document.as_deref_mut());
        }
        SceneNodeAction::Rename => {
            rename.open = true;
            rename.target = Some(*selected_id);
            rename.name = selected_object.name.clone();
            rename.error.clear();
            rename.revision = rename.revision.wrapping_add(1);
        }
        SceneNodeAction::Delete => {
            if root.is_some() {
                return;
            }
            if let Some(history) = history.as_deref_mut() {
                history.begin(
                    "Delete Entity",
                    capture_scene_snapshot(&history_nodes, &selection, *mode),
                );
            }
            let subtree = subtree_ids(*selected_id, &objects);
            for (entity, _, id, _, _, _, _, _, _, _, _) in &objects {
                if subtree.contains(id) {
                    commands.entity(entity).despawn();
                }
            }
            let parent_entity = selected_parent.0.and_then(|parent_id| {
                objects
                    .iter()
                    .find_map(|(entity, _, id, _, _, _, _, _, _, _, _)| {
                        (*id == parent_id).then_some(entity)
                    })
            });
            selection.0 = parent_entity;
            saved_selections.set(*mode, parent_entity);
            mark_document_changed(document.as_deref_mut());
        }
        SceneNodeAction::Duplicate => {
            if root.is_some() {
                return;
            }
            if let Some(history) = history.as_deref_mut() {
                history.begin(
                    "Duplicate Entity",
                    capture_scene_snapshot(&history_nodes, &selection, *mode),
                );
            }
            let subtree = subtree_ids(*selected_id, &objects);
            let id_map: HashMap<_, _> = subtree
                .iter()
                .copied()
                .map(|old_id| (old_id, SceneNodeId::new()))
                .collect();
            let next_order = objects
                .iter()
                .filter(|(_, _, _, parent, _, _, _, _, _, _, _)| parent.0 == selected_parent.0)
                .map(|(_, _, _, _, order, _, _, _, _, _, _)| order.0)
                .max()
                .map_or(selected_order.0, |order| order.saturating_add(1));
            let duplicate_name = unique_name(
                &selected_object.name,
                objects
                    .iter()
                    .map(|(_, object, _, _, _, _, _, _, _, _, _)| &object.name),
            );
            let mut duplicate_root = None;
            for (
                entity,
                object,
                id,
                parent,
                order,
                space,
                transform,
                kind,
                components,
                systems,
                _,
            ) in &objects
            {
                if !subtree.contains(id) {
                    continue;
                }
                let new_id = id_map[id];
                let new_parent = if entity == selected_entity {
                    selected_parent.0
                } else {
                    parent
                        .0
                        .and_then(|old_parent| id_map.get(&old_parent).copied())
                };
                let entity_kind = kind.copied().unwrap_or(match space {
                    SceneSpace::TwoD => EntityKind::Empty2D,
                    SceneSpace::ThreeD => EntityKind::Empty3D,
                });
                let new_entity = spawn_entity(
                    &mut commands,
                    entity_kind,
                    if entity == selected_entity {
                        duplicate_name.clone()
                    } else {
                        object.name.clone()
                    },
                    new_id,
                    new_parent,
                    if entity == selected_entity {
                        next_order
                    } else {
                        order.0
                    },
                    *space,
                );
                let components = components
                    .map(|components| components.0.clone())
                    .unwrap_or_default();
                for component in components.iter().copied() {
                    insert_builtin_component(&mut commands, new_entity, component);
                }
                commands.entity(new_entity).insert((
                    AddedEntityComponents(components),
                    systems.cloned().unwrap_or_default(),
                ));
                if let Some(transform) = transform {
                    commands
                        .entity(new_entity)
                        .insert((*transform, Visibility::Visible));
                }
                if let Ok((
                    custom_components,
                    entity_script,
                    layout,
                    content,
                    sprite,
                    model,
                    collision,
                )) = resources.get(entity)
                {
                    commands.entity(new_entity).insert((
                        custom_components.cloned().unwrap_or_default(),
                        entity_script.cloned().unwrap_or_default(),
                    ));
                    if let Some(layout) = layout {
                        commands.entity(new_entity).insert(*layout);
                    }
                    if let Some(content) = content {
                        commands.entity(new_entity).insert(content.clone());
                    }
                    if let Some(sprite) = sprite {
                        commands.entity(new_entity).insert(sprite.clone());
                    }
                    if let Some(model) = model {
                        commands.entity(new_entity).insert(model.clone());
                    }
                    if let Some(collision) = collision {
                        commands.entity(new_entity).insert(*collision);
                    }
                }
                if entity == selected_entity {
                    duplicate_root = Some(new_entity);
                }
            }
            selection.0 = duplicate_root;
            saved_selections.set(*mode, duplicate_root);
            mark_document_changed(document.as_deref_mut());
        }
    }
}

#[allow(clippy::type_complexity)]
fn subtree_ids(
    root: SceneNodeId,
    objects: &Query<(
        Entity,
        &EditableObject,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
        &SceneSpace,
        Option<&Transform>,
        Option<&EntityKind>,
        Option<&AddedEntityComponents>,
        Option<&EntitySystemBindings>,
        Option<&SceneRoot>,
    )>,
) -> HashSet<SceneNodeId> {
    let mut result = HashSet::from([root]);
    loop {
        let before = result.len();
        for (_, _, id, parent, _, _, _, _, _, _, _) in objects.iter() {
            if parent
                .0
                .is_some_and(|parent_id| result.contains(&parent_id))
            {
                result.insert(*id);
            }
        }
        if result.len() == before {
            return result;
        }
    }
}

fn close_rename_dialog(state: &mut RenameDialogState) {
    state.open = false;
    state.target = None;
    state.error.clear();
    state.revision = state.revision.wrapping_add(1);
}

fn sync_rename_input(
    inputs: Query<&EditableText, (With<SceneNodeRenameInput>, Changed<EditableText>)>,
    mut state: ResMut<RenameDialogState>,
) {
    for input in &inputs {
        state.name = input.value().to_string();
        state.error.clear();
    }
}

/// Commit or cancel the active inline Scene Tree rename.
///
/// The text widget owns cursor movement and editing. This system only handles
/// the two editor-level commands that finish the operation, keeping the row in
/// place instead of opening a modal dialog.
#[allow(clippy::type_complexity)]
fn handle_inline_rename_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    inputs: Query<(Entity, &EditableText), With<SceneNodeRenameInput>>,
    mut rename: ResMut<RenameDialogState>,
    selection: ResMut<Selection>,
    mode: Res<WorkspaceMode>,
    mut document: Option<ResMut<SceneDocument>>,
    mut history: Option<ResMut<SceneHistory>>,
    history_nodes: Query<SceneSnapshotQuery>,
    objects: Query<(Entity, &EditableObject, &SceneNodeId)>,
    mut commands: Commands,
) {
    if !rename.open {
        return;
    }
    let Some((input_entity, input)) = inputs.iter().next() else {
        return;
    };
    if focus
        .as_deref()
        .and_then(InputFocus::get)
        .is_some_and(|focused| focused != input_entity)
    {
        return;
    }

    if keyboard.just_pressed(KeyCode::Escape) {
        close_rename_dialog(&mut rename);
        return;
    }
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }

    rename.name = input.value().to_string();
    let name = rename.name.trim().to_owned();
    if name.is_empty() {
        rename.error = "Entity name cannot be empty.".into();
        return;
    }
    let Some(target_id) = rename.target else {
        close_rename_dialog(&mut rename);
        return;
    };
    let Some((entity, _, _)) = objects.iter().find(|(_, _, id)| **id == target_id) else {
        close_rename_dialog(&mut rename);
        return;
    };

    if let Some(history) = history.as_deref_mut() {
        history.begin(
            "Rename Entity",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    commands.entity(entity).insert(EditableObject { name });
    close_rename_dialog(&mut rename);
    mark_document_changed(document.as_deref_mut());
}

fn sync_scene_root_menu(
    menu: Res<SceneRootMenuState>,
    mut contexts: Query<&mut Node, With<SceneContextMenu>>,
) {
    if !menu.is_changed() {
        return;
    }

    for mut node in &mut contexts {
        node.display = if menu.context_open {
            Display::Flex
        } else {
            Display::None
        };
        node.left = Val::Px((menu.context_position.x - 2.0).max(0.0));
        node.top = Val::Px((menu.context_position.y - 2.0).max(0.0));
    }
}

fn close_scene_context_menu_on_cursor_exit(
    mut menu: ResMut<SceneRootMenuState>,
    mut pointer: ResMut<SceneContextMenuPointerState>,
    contexts: Query<&RelativeCursorPosition, With<SceneContextMenu>>,
) {
    if !menu.context_open {
        pointer.reset();
        return;
    }

    if pointer.layout_grace {
        pointer.layout_grace = false;
        return;
    }

    let cursor_over = contexts.iter().any(RelativeCursorPosition::cursor_over);
    if !cursor_over && pointer.armed {
        pointer.reset();
        menu.context_open = false;
    }
}

fn rebuild_scene_tree(
    mut commands: Commands,
    all_objects: Query<(
        Entity,
        &EditableObject,
        &EntityKind,
        &SceneSpace,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
    )>,
    asset_server: Option<Res<AssetServer>>,
    list: Query<Entity, With<SceneTreeList>>,
    rows: Query<&SceneTreeRow>,
    selection: Res<Selection>,
    selection_set: Res<SelectionSet>,
    mode: Res<WorkspaceMode>,
    expansion: Res<SceneTreeExpansionState>,
    rename: Res<RenameDialogState>,
    mut cache: Local<(WorkspaceMode, u64, u64)>,
) {
    let Ok(list_entity) = list.single() else {
        return;
    };

    let entries: Vec<_> = all_objects
        .iter()
        .filter(|(_, _, _, space, _, _, _)| match *mode {
            WorkspaceMode::TwoD => **space == SceneSpace::TwoD,
            WorkspaceMode::ThreeD => **space == SceneSpace::ThreeD,
        })
        .map(|(entity, object, kind, _, id, parent, order)| TreeEntry {
            entity,
            name: object.name.clone(),
            kind: *kind,
            id: *id,
            parent: parent.0,
            order: order.0,
        })
        .collect();

    let mode_now = *mode;
    let signature = tree_signature(&entries, &expansion.collapsed);
    let flattened = flatten_tree(&entries, &expansion.collapsed);
    if mode_now == cache.0
        && signature == cache.1
        && rename.revision == cache.2
        && rows.iter().count() == flattened.len()
    {
        return;
    }

    commands.entity(list_entity).despawn_related::<Children>();

    for (entry, depth, has_children) in flattened {
        spawn_tree_row(
            &mut commands,
            list_entity,
            entry,
            depth,
            has_children,
            expansion.is_collapsed(entry.id),
            selection_set.contains(&selection, entry.entity),
            asset_server.as_deref(),
            rename.open && rename.target == Some(entry.id),
        );
    }

    *cache = (mode_now, signature, rename.revision);
}

#[derive(Clone, Debug)]
struct TreeEntry {
    entity: Entity,
    name: String,
    kind: EntityKind,
    id: SceneNodeId,
    parent: Option<SceneNodeId>,
    order: u32,
}

fn tree_signature(entries: &[TreeEntry], collapsed: &HashSet<SceneNodeId>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for entry in entries {
        entry.entity.hash(&mut hasher);
        entry.name.hash(&mut hasher);
        entry.kind.hash(&mut hasher);
        entry.id.hash(&mut hasher);
        entry.parent.hash(&mut hasher);
        entry.order.hash(&mut hasher);
        collapsed.contains(&entry.id).hash(&mut hasher);
    }
    hasher.finish()
}

fn flatten_tree<'a>(
    entries: &'a [TreeEntry],
    collapsed: &HashSet<SceneNodeId>,
) -> Vec<(&'a TreeEntry, usize, bool)> {
    let entry_ids: HashSet<_> = entries.iter().map(|entry| entry.id).collect();
    let mut children: HashMap<Option<SceneNodeId>, Vec<&TreeEntry>> = HashMap::new();
    for entry in entries {
        // Workspace filtering can omit a document root that belongs to the
        // other spatial mode. Its children remain valid and are displayed as
        // logical top-level entries in the active workspace.
        let visible_parent = entry.parent.filter(|parent| entry_ids.contains(parent));
        children.entry(visible_parent).or_default().push(entry);
    }
    for siblings in children.values_mut() {
        siblings.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.name.cmp(&right.name))
        });
    }
    let mut flattened = Vec::with_capacity(entries.len());
    let mut visited = HashSet::new();
    flatten_children(None, 0, &children, collapsed, &mut visited, &mut flattened);
    flattened
}

fn flatten_children<'a>(
    parent: Option<SceneNodeId>,
    depth: usize,
    children: &HashMap<Option<SceneNodeId>, Vec<&'a TreeEntry>>,
    collapsed: &HashSet<SceneNodeId>,
    visited: &mut HashSet<SceneNodeId>,
    flattened: &mut Vec<(&'a TreeEntry, usize, bool)>,
) {
    let Some(siblings) = children.get(&parent) else {
        return;
    };
    for entry in siblings {
        if !visited.insert(entry.id) {
            continue;
        }
        let has_children = children.contains_key(&Some(entry.id));
        flattened.push((entry, depth, has_children));
        if collapsed.contains(&entry.id) {
            continue;
        }
        flatten_children(
            Some(entry.id),
            depth.saturating_add(1),
            children,
            collapsed,
            visited,
            flattened,
        );
    }
}

fn spawn_tree_row(
    commands: &mut Commands,
    list_entity: Entity,
    entry: &TreeEntry,
    depth: usize,
    has_children: bool,
    collapsed: bool,
    selected: bool,
    asset_server: Option<&AssetServer>,
    renaming: bool,
) {
    let branch = if collapsed { ">" } else { "v" };
    let row_entity = commands
        .spawn((
            SceneTreeRow {
                target: entry.entity,
            },
            ChildOf(list_entity),
            Button,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(24.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::new(
                    Val::Px(7.0 + depth as f32 * 16.0),
                    Val::Px(5.0),
                    Val::Px(3.0),
                    Val::Px(3.0),
                ),
                ..default()
            },
            BackgroundColor(row_bg(selected)),
            BorderColor::DEFAULT,
        ))
        .id();
    commands
        .entity(row_entity)
        .with_children(|row| {
            let mut toggle = row.spawn((
                Node {
                    width: Val::Px(10.0),
                    min_width: Val::Px(10.0),
                    height: Val::Px(18.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                if has_children {
                    Pickable::default()
                } else {
                    Pickable::IGNORE
                },
            ));
            if has_children {
                toggle
                    .insert(SceneTreeToggle { target: entry.id })
                    .observe(on_scene_tree_toggle_click);
            }
            toggle.with_children(|toggle| {
                toggle.spawn((
                    Text::new(if has_children { branch } else { "" }),
                    Pickable::IGNORE,
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.62, 0.65, 0.69)),
                ));
            });
            if let Some(asset_server) = asset_server {
                row.spawn((
                    Node {
                        width: Val::Px(17.0),
                        min_width: Val::Px(17.0),
                        height: Val::Px(17.0),
                        ..default()
                    },
                    ImageNode::new(asset_server.load(entry.kind.icon_path()))
                        .with_color(entry.kind.icon_color()),
                    Pickable::IGNORE,
                ));
            }
            if renaming {
                row.spawn((
                    SceneNodeRenameInput,
                    AutoFocus,
                    SelectAllOnFocus,
                    EditableText::new(&entry.name),
                    TextCursorStyle::default(),
                    TextFont {
                        font_size: FontSize::Px(11.5),
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.94, 0.97)),
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        height: Val::Px(20.0),
                        padding: UiRect::horizontal(Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::accent()),
                ));
            } else {
                row.spawn((
                    Text::new(entry.name.clone()),
                    Pickable::IGNORE,
                    TextFont {
                        font_size: FontSize::Px(11.5),
                        ..default()
                    },
                    TextColor(Color::srgb(0.88, 0.90, 0.92)),
                ));
                row.spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                });
            }
            row.spawn((
                Text::new("o"),
                Pickable::IGNORE,
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::text_muted()),
            ));
        })
        .observe(on_tree_row_click)
        .observe(on_tree_row_drag_start)
        .observe(on_tree_row_drag_over)
        .observe(on_tree_row_drag_leave)
        .observe(on_tree_row_drop)
        .observe(on_tree_row_drag_end);
}

fn on_scene_tree_toggle_click(
    mut click: On<Pointer<Click>>,
    toggles: Query<&SceneTreeToggle>,
    mut expansion: ResMut<SceneTreeExpansionState>,
    mut menu: ResMut<SceneRootMenuState>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(toggle) = toggles.get(click.entity) else {
        return;
    };
    expansion.toggle(toggle.target);
    menu.context_open = false;
    click.propagate(false);
}

fn scene_tree_row_target(
    mut entity: Entity,
    rows: &Query<&SceneTreeRow>,
    parents: &Query<&ChildOf>,
) -> Option<Entity> {
    for _ in 0..16 {
        if let Ok(row) = rows.get(entity) {
            return Some(row.target);
        }
        entity = parents.get(entity).ok()?.parent();
    }
    None
}

fn collect_hierarchy_nodes(
    objects: &Query<(
        Entity,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
        &SceneSpace,
        &EntityKind,
        Has<SceneRoot>,
    )>,
) -> Vec<HierarchyNodeState> {
    objects
        .iter()
        .map(
            |(entity, id, parent, order, space, kind, root)| HierarchyNodeState {
                entity,
                id: *id,
                parent: parent.0,
                order: order.0,
                space: *space,
                kind: *kind,
                root,
            },
        )
        .collect()
}

fn scene_tree_drop_placement(
    pointer_position: Vec2,
    node: &ComputedNode,
    target: &ComputedUiRenderTargetInfo,
    transform: &UiGlobalTransform,
    ui_scale: f32,
    target_is_root: bool,
) -> SceneTreeDropPlacement {
    if target_is_root {
        return SceneTreeDropPlacement::Child;
    }
    let point = pointer_position * target.scale_factor() / ui_scale.max(f32::EPSILON);
    let Some(normalized) = node.normalize_point(*transform, point) else {
        return SceneTreeDropPlacement::Child;
    };
    if normalized.y < -0.20 {
        SceneTreeDropPlacement::Before
    } else if normalized.y > 0.20 {
        SceneTreeDropPlacement::After
    } else {
        SceneTreeDropPlacement::Child
    }
}

fn auto_expand_scene_tree_drag_target(
    time: Option<Res<Time>>,
    mut drag: ResMut<SceneTreeDragState>,
    mut expansion: ResMut<SceneTreeExpansionState>,
) {
    let (Some(time), Some(target)) = (time.as_deref(), drag.auto_expand_target) else {
        return;
    };
    drag.auto_expand_timer.tick(time.delta());
    if drag.auto_expand_timer.just_finished() {
        expansion.expand(target);
        drag.clear_auto_expand();
    }
}

fn plan_hierarchy_drop(
    nodes: &[HierarchyNodeState],
    source_entity: Entity,
    target_entity: Entity,
    placement: SceneTreeDropPlacement,
) -> Option<Vec<HierarchyNodeUpdate>> {
    let source_index = nodes.iter().position(|node| node.entity == source_entity)?;
    let target_index = nodes.iter().position(|node| node.entity == target_entity)?;
    let source = nodes[source_index];
    let target = nodes[target_index];
    if source.root || source.entity == target.entity || source.space != target.space {
        return None;
    }

    let placement = if target.root {
        SceneTreeDropPlacement::Child
    } else {
        placement
    };
    let destination_parent = match placement {
        SceneTreeDropPlacement::Child => Some(target.id),
        SceneTreeDropPlacement::Before | SceneTreeDropPlacement::After => target.parent,
    }?;
    let destination_parent_node = nodes.iter().find(|node| node.id == destination_parent)?;

    // UI layout inheritance is only well-defined below the scene root or
    // another UI node. Preserve that invariant while allowing every other
    // valid hierarchy move.
    if source.kind.is_ui() && !destination_parent_node.root && !destination_parent_node.kind.is_ui()
    {
        return None;
    }

    // Walking from the requested parent towards the root must never encounter
    // the source node, otherwise this drop would create a hierarchy cycle.
    let mut ancestor = Some(destination_parent);
    let mut visited = HashSet::new();
    while let Some(id) = ancestor {
        if id == source.id || !visited.insert(id) {
            return None;
        }
        ancestor = nodes.iter().find(|node| node.id == id)?.parent;
    }

    let old_parent = source.parent;
    let mut destination_siblings: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            (index != source_index && node.parent == Some(destination_parent)).then_some(index)
        })
        .collect();
    destination_siblings.sort_by_key(|index| (nodes[*index].order, *index));
    let insertion_index = match placement {
        SceneTreeDropPlacement::Child => destination_siblings.len(),
        SceneTreeDropPlacement::Before => destination_siblings
            .iter()
            .position(|index| *index == target_index)?,
        SceneTreeDropPlacement::After => destination_siblings
            .iter()
            .position(|index| *index == target_index)?
            .saturating_add(1),
    };
    destination_siblings.insert(insertion_index, source_index);

    let mut updates = HashMap::<Entity, HierarchyNodeUpdate>::new();
    for (order, index) in destination_siblings.into_iter().enumerate() {
        let node = nodes[index];
        updates.insert(
            node.entity,
            HierarchyNodeUpdate {
                entity: node.entity,
                parent: Some(destination_parent),
                order: order as u32,
            },
        );
    }

    if old_parent != Some(destination_parent) {
        if let Some(old_parent) = old_parent {
            let mut old_siblings: Vec<usize> = nodes
                .iter()
                .enumerate()
                .filter_map(|(index, node)| {
                    (index != source_index && node.parent == Some(old_parent)).then_some(index)
                })
                .collect();
            old_siblings.sort_by_key(|index| (nodes[*index].order, *index));
            for (order, index) in old_siblings.into_iter().enumerate() {
                let node = nodes[index];
                updates.insert(
                    node.entity,
                    HierarchyNodeUpdate {
                        entity: node.entity,
                        parent: Some(old_parent),
                        order: order as u32,
                    },
                );
            }
        }
    }

    Some(
        updates
            .into_values()
            .filter(|update| {
                nodes
                    .iter()
                    .find(|node| node.entity == update.entity)
                    .is_some_and(|node| node.parent != update.parent || node.order != update.order)
            })
            .collect(),
    )
}

fn on_tree_row_drag_start(
    mut drag: On<Pointer<DragStart>>,
    rows: Query<&SceneTreeRow>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<SceneRoot>>,
    mut selection: ResMut<Selection>,
    mut menu: ResMut<SceneRootMenuState>,
    mut state: ResMut<SceneTreeDragState>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(source) = scene_tree_row_target(drag.entity, &rows, &parents) else {
        return;
    };
    if roots.contains(source) {
        state.clear();
        return;
    }
    select_entity(source, &mut selection);
    menu.context_open = false;
    state.source = Some(source);
    state.hovered_row = None;
    state.placement = SceneTreeDropPlacement::Child;
    state.clear_auto_expand();
    drag.propagate(false);
}

#[allow(clippy::type_complexity)]
fn on_tree_row_drag_over(
    mut drag: On<Pointer<DragOver>>,
    rows: Query<&SceneTreeRow>,
    parents: Query<&ChildOf>,
    row_geometry: Query<(
        &SceneTreeRow,
        &ComputedNode,
        &ComputedUiRenderTargetInfo,
        &UiGlobalTransform,
    )>,
    objects: Query<(
        Entity,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
        &SceneSpace,
        &EntityKind,
        Has<SceneRoot>,
    )>,
    ui_scale: Option<Res<UiScale>>,
    expansion: Res<SceneTreeExpansionState>,
    mut state: ResMut<SceneTreeDragState>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let source = state
        .source
        .or_else(|| scene_tree_row_target(drag.dragged, &rows, &parents));
    let Some(source) = source else {
        return;
    };
    let Ok((target_row, node, render_target, transform)) = row_geometry.get(drag.entity) else {
        return;
    };
    let nodes = collect_hierarchy_nodes(&objects);
    let target_is_root = nodes
        .iter()
        .find(|entry| entry.entity == target_row.target)
        .is_some_and(|entry| entry.root);
    let placement = scene_tree_drop_placement(
        drag.pointer_location.position,
        node,
        render_target,
        transform,
        ui_scale.as_deref().map_or(1.0, |scale| scale.0),
        target_is_root,
    );
    if plan_hierarchy_drop(&nodes, source, target_row.target, placement).is_some() {
        state.source = Some(source);
        state.hovered_row = Some(drag.entity);
        state.placement = placement;
        if placement == SceneTreeDropPlacement::Child {
            if let Some(target_id) = nodes
                .iter()
                .find(|entry| entry.entity == target_row.target)
                .map(|entry| entry.id)
                .filter(|id| expansion.is_collapsed(*id))
            {
                state.arm_auto_expand(target_id);
            } else {
                state.clear_auto_expand();
            }
        } else {
            state.clear_auto_expand();
        }
    } else if state.hovered_row == Some(drag.entity) {
        state.hovered_row = None;
        state.clear_auto_expand();
    }
    drag.propagate(false);
}

fn on_tree_row_drag_leave(mut drag: On<Pointer<DragLeave>>, mut state: ResMut<SceneTreeDragState>) {
    if drag.button == PointerButton::Primary && state.hovered_row == Some(drag.entity) {
        state.hovered_row = None;
        state.clear_auto_expand();
    }
    drag.propagate(false);
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn on_tree_row_drop(
    mut drop: On<Pointer<DragDrop>>,
    rows: Query<&SceneTreeRow>,
    parents: Query<&ChildOf>,
    objects: Query<(
        Entity,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
        &SceneSpace,
        &EntityKind,
        Has<SceneRoot>,
    )>,
    history_nodes: Query<SceneSnapshotQuery>,
    mode: Res<WorkspaceMode>,
    mut selection: ResMut<Selection>,
    mut saved_selections: ResMut<WorkspaceSelections>,
    mut document: Option<ResMut<SceneDocument>>,
    mut history: Option<ResMut<SceneHistory>>,
    mut expansion: ResMut<SceneTreeExpansionState>,
    mut state: ResMut<SceneTreeDragState>,
    mut commands: Commands,
) {
    if drop.button != PointerButton::Primary {
        return;
    }
    let source = state
        .source
        .or_else(|| scene_tree_row_target(drop.dropped, &rows, &parents));
    let target = rows.get(drop.entity).ok().map(|row| row.target);
    let (Some(source), Some(target)) = (source, target) else {
        state.clear();
        return;
    };
    let nodes = collect_hierarchy_nodes(&objects);
    let placement = if state.hovered_row == Some(drop.entity) {
        state.placement
    } else {
        SceneTreeDropPlacement::Child
    };
    let Some(updates) = plan_hierarchy_drop(&nodes, source, target, placement) else {
        state.clear();
        return;
    };
    if placement == SceneTreeDropPlacement::Child {
        if let Some(target_id) = nodes
            .iter()
            .find(|entry| entry.entity == target)
            .map(|entry| entry.id)
        {
            expansion.expand(target_id);
        }
    }
    if !updates.is_empty() {
        if let Some(history) = history.as_deref_mut() {
            history.begin(
                "Move Entity",
                capture_scene_snapshot(&history_nodes, &selection, *mode),
            );
        }
        for update in updates {
            commands.entity(update.entity).insert((
                SceneParentId(update.parent),
                SceneSiblingOrder(update.order),
            ));
        }
        select_entity(source, &mut selection);
        saved_selections.set(*mode, Some(source));
        mark_document_changed(document.as_deref_mut());
    }
    state.clear();
    drop.propagate(false);
}

fn on_tree_row_drag_end(mut drag: On<Pointer<DragEnd>>, mut state: ResMut<SceneTreeDragState>) {
    if drag.button == PointerButton::Primary {
        state.clear();
    }
    drag.propagate(false);
}

fn on_tree_row_click(
    click: On<Pointer<Click>>,
    mut selection: ResMut<Selection>,
    mut selection_set: ResMut<SelectionSet>,
    keyboard: Res<ButtonInput<KeyCode>>,
    rows: Query<&SceneTreeRow>,
    mut menu: ResMut<SceneRootMenuState>,
    mut menu_pointer: ResMut<SceneContextMenuPointerState>,
) {
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    match click.button {
        PointerButton::Primary => {
            menu.context_open = false;
            menu_pointer.reset();
            let shift =
                keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
            let control =
                keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
            selection_set.select_from_click(&mut selection, row.target, shift, control);
        }
        PointerButton::Secondary => {
            selection_set.select_only(&mut selection, row.target);
            open_scene_context_menu(
                &mut menu,
                &mut menu_pointer,
                click.pointer_location.position,
            );
        }
        _ => {}
    }
}

fn highlight_scene_tree_rows(
    selection: Res<Selection>,
    selection_set: Res<SelectionSet>,
    drag: Res<SceneTreeDragState>,
    mut rows: Query<(
        Entity,
        &SceneTreeRow,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    if !selection.is_changed() && !selection_set.is_changed() && !drag.is_changed() {
        return;
    }
    for (entity, row, mut bg, mut border) in &mut rows {
        let placement = (drag.hovered_row == Some(entity)).then_some(drag.placement);
        *bg = BackgroundColor(if placement == Some(SceneTreeDropPlacement::Child) {
            Color::srgba(0.10, 0.48, 0.70, 0.42)
        } else {
            row_bg(selection_set.contains(&selection, row.target))
        });
        *border = match placement {
            Some(SceneTreeDropPlacement::Before) => BorderColor {
                top: theme::accent(),
                ..BorderColor::DEFAULT
            },
            Some(SceneTreeDropPlacement::Child) => BorderColor::all(theme::accent()),
            Some(SceneTreeDropPlacement::After) => BorderColor {
                bottom: theme::accent(),
                ..BorderColor::DEFAULT
            },
            None => BorderColor::DEFAULT,
        };
    }
}

fn row_bg(selected: bool) -> Color {
    if selected {
        theme::bg_selected()
    } else {
        Color::NONE
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            events::{Click, Pointer},
            pointer::{Location, PointerId},
        },
    };

    use super::*;

    fn hierarchy_app() -> App {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<WorkspaceSelections>()
            .init_resource::<SceneDocument>()
            .add_plugins(HierarchyPlugin);
        app
    }

    #[test]
    fn collapsed_tree_hides_descendants_and_restores_them() {
        let root_id = SceneNodeId::new();
        let parent_id = SceneNodeId::new();
        let child_id = SceneNodeId::new();
        let sibling_id = SceneNodeId::new();
        let entries = vec![
            TreeEntry {
                entity: Entity::from_raw_u32(1).unwrap(),
                name: "Root".into(),
                kind: EntityKind::Empty2D,
                id: root_id,
                parent: None,
                order: 0,
            },
            TreeEntry {
                entity: Entity::from_raw_u32(2).unwrap(),
                name: "Parent".into(),
                kind: EntityKind::Empty2D,
                id: parent_id,
                parent: Some(root_id),
                order: 0,
            },
            TreeEntry {
                entity: Entity::from_raw_u32(3).unwrap(),
                name: "Child".into(),
                kind: EntityKind::Sprite2D,
                id: child_id,
                parent: Some(parent_id),
                order: 0,
            },
            TreeEntry {
                entity: Entity::from_raw_u32(4).unwrap(),
                name: "Sibling".into(),
                kind: EntityKind::Camera2D,
                id: sibling_id,
                parent: Some(root_id),
                order: 1,
            },
        ];
        let mut expansion = SceneTreeExpansionState::default();
        expansion.toggle(parent_id);

        let collapsed = flatten_tree(&entries, &expansion.collapsed);
        assert_eq!(
            collapsed
                .iter()
                .map(|(entry, _, _)| entry.id)
                .collect::<Vec<_>>(),
            vec![root_id, parent_id, sibling_id]
        );
        assert!(collapsed[1].2);

        expansion.toggle(parent_id);
        let expanded = flatten_tree(&entries, &expansion.collapsed);
        assert_eq!(
            expanded
                .iter()
                .map(|(entry, _, _)| entry.id)
                .collect::<Vec<_>>(),
            vec![root_id, parent_id, child_id, sibling_id]
        );
    }

    #[test]
    fn arrow_keys_collapse_expand_and_navigate_parent_child() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<RenameDialogState>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneTreeExpansionState>()
            .init_resource::<Selection>()
            .init_resource::<WorkspaceSelections>()
            .add_systems(Update, handle_scene_tree_keyboard_navigation);

        let root_id = SceneNodeId::new();
        let parent_id = SceneNodeId::new();
        let child_id = SceneNodeId::new();
        let root = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Root".into(),
                },
                EntityKind::Empty2D,
                SceneSpace::TwoD,
                root_id,
                SceneParentId(None),
                SceneSiblingOrder(0),
            ))
            .id();
        let parent = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Parent".into(),
                },
                EntityKind::Empty2D,
                SceneSpace::TwoD,
                parent_id,
                SceneParentId(Some(root_id)),
                SceneSiblingOrder(0),
            ))
            .id();
        let child = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Child".into(),
                },
                EntityKind::Sprite2D,
                SceneSpace::TwoD,
                child_id,
                SceneParentId(Some(parent_id)),
                SceneSiblingOrder(0),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(parent);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::AltLeft);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowLeft);
        app.update();
        assert!(
            app.world()
                .resource::<SceneTreeExpansionState>()
                .is_collapsed(parent_id)
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(KeyCode::ArrowLeft);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        app.update();
        assert!(
            !app.world()
                .resource::<SceneTreeExpansionState>()
                .is_collapsed(parent_id)
        );
        assert_eq!(app.world().resource::<Selection>().0, Some(parent));

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(KeyCode::ArrowRight);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        app.update();
        assert_eq!(app.world().resource::<Selection>().0, Some(child));

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(KeyCode::ArrowRight);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowLeft);
        app.update();
        assert_eq!(app.world().resource::<Selection>().0, Some(parent));
        assert_ne!(root, parent);
    }

    fn create_root(app: &mut App) -> Entity {
        let option = app
            .world_mut()
            .spawn(SceneRootOption(RootNodeKind::TwoD))
            .id();
        app.world_mut().trigger(Activate { entity: option });
        confirm_picker(app);
        app.world_mut().flush();
        app.world().resource::<Selection>().0.unwrap()
    }

    fn add_empty(app: &mut App) -> Entity {
        add_kind(app, EntityKind::Empty)
    }

    fn add_kind(app: &mut App, kind: EntityKind) -> Entity {
        let option = app.world_mut().spawn(SceneEntityOption(kind)).id();
        app.world_mut().trigger(Activate { entity: option });
        confirm_picker(app);
        app.world_mut().flush();
        app.world().resource::<Selection>().0.unwrap()
    }

    fn confirm_picker(app: &mut App) {
        let create = app.world_mut().spawn(SceneEntityPickerCreate).id();
        app.world_mut().trigger(Activate { entity: create });
    }

    fn set_multi_selection(app: &mut App, entities: impl IntoIterator<Item = Entity>) {
        let entities: Vec<_> = entities.into_iter().collect();
        app.world_mut()
            .resource_scope(|world, mut selection_set: Mut<SelectionSet>| {
                let mut selection = world.resource_mut::<Selection>();
                selection_set.select_many(&mut selection, entities);
            });
    }

    #[test]
    fn scene_context_add_opens_entity_system_picker() {
        let mut app = hierarchy_app();
        app.world_mut()
            .resource_mut::<SceneRootMenuState>()
            .context_open = true;
        let button = app.world_mut().spawn(SceneContextAddEntity).id();

        app.world_mut().trigger(Activate { entity: button });

        let menu = app.world().resource::<SceneRootMenuState>();
        assert!(menu.open);
        assert!(!menu.context_open);
    }

    #[test]
    fn right_click_on_scene_workspace_opens_context_menu() {
        let mut app = hierarchy_app();
        let list = app.world_mut().spawn(SceneTreeList).id();
        app.world_mut()
            .commands()
            .entity(list)
            .observe(on_scene_tree_click);
        app.world_mut().flush();
        let position = Vec2::new(48.0, 96.0);
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 320,
                height: 480,
            },
            position,
        };

        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location,
            Click {
                button: PointerButton::Secondary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                duration: Duration::from_millis(10),
                count: 1,
            },
            list,
        ));
        app.world_mut().flush();

        let menu = app.world().resource::<SceneRootMenuState>();
        assert!(menu.context_open);
        assert!(!menu.open);
        assert_eq!(menu.context_position, position);
    }

    #[test]
    fn scene_context_menu_closes_when_pointer_moves_directly_outside() {
        let mut app = App::new();
        app.init_resource::<SceneRootMenuState>()
            .init_resource::<SceneContextMenuPointerState>()
            .add_systems(Update, close_scene_context_menu_on_cursor_exit);
        let context = app
            .world_mut()
            .spawn((
                SceneContextMenu,
                RelativeCursorPosition {
                    cursor_over: false,
                    normalized: None,
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<SceneRootMenuState>()
            .context_open = true;
        app.world_mut()
            .resource_mut::<SceneContextMenuPointerState>()
            .arm();
        app.update();
        assert!(app.world().resource::<SceneRootMenuState>().context_open);

        app.update();

        assert!(!app.world().resource::<SceneRootMenuState>().context_open);
        assert!(
            app.world()
                .entity(context)
                .contains::<RelativeCursorPosition>()
        );
    }

    #[test]
    fn creating_two_d_root_switches_workspace_and_selects_root() {
        let mut app = hierarchy_app();
        let old_object = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Demo Quad".into(),
                },
                SceneSpace::TwoD,
            ))
            .id();
        let option = app
            .world_mut()
            .spawn(SceneRootOption(RootNodeKind::TwoD))
            .id();

        app.world_mut().trigger(Activate { entity: option });
        confirm_picker(&mut app);
        app.world_mut().flush();

        let selection = app.world().resource::<Selection>().0;
        assert_eq!(
            app.world().resource::<WorkspaceMode>().clone(),
            WorkspaceMode::TwoD
        );
        assert!(selection.is_some());
        assert_ne!(selection, Some(old_object));
        assert!(
            app.world()
                .entity(selection.unwrap())
                .contains::<SceneRoot>()
        );
        let world = app.world_mut();
        let object_count = world.query::<&EditableObject>().iter(world).count();
        assert_eq!(object_count, 1);
        let document = world.resource::<SceneDocument>();
        assert!(document.open);
        assert!(document.dirty);
        assert_eq!(document.name, "Untitled Level");
    }

    #[test]
    fn stable_node_ids_are_unique() {
        assert_ne!(SceneNodeId::new(), SceneNodeId::new());
    }

    #[test]
    fn f2_shortcut_enters_inline_rename_state() {
        let mut app = hierarchy_app();
        let entity = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Player".into(),
                },
                EntityKind::Empty2D,
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(entity);
        app.world_mut()
            .spawn(SceneNodeActionButton(SceneNodeAction::Rename));

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F2);
        app.update();
        let rename = app.world().resource::<RenameDialogState>();
        assert!(rename.open);
        assert_eq!(
            rename.target,
            app.world().entity(entity).get::<SceneNodeId>().copied()
        );
        assert_eq!(rename.name, "Player");
    }

    #[test]
    fn enter_commits_inline_rename_and_marks_document_dirty() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<RenameDialogState>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<Selection>()
            .init_resource::<SceneDocument>()
            .add_systems(Update, handle_inline_rename_keyboard);
        let id = SceneNodeId::new();
        let entity = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Before".into(),
                },
                id,
            ))
            .id();
        app.world_mut()
            .spawn((SceneNodeRenameInput, EditableText::new("After")));
        {
            let mut rename = app.world_mut().resource_mut::<RenameDialogState>();
            rename.open = true;
            rename.target = Some(id);
            rename.name = "After".into();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();

        assert_eq!(
            app.world()
                .entity(entity)
                .get::<EditableObject>()
                .unwrap()
                .name,
            "After"
        );
        assert!(!app.world().resource::<RenameDialogState>().open);
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn escape_cancels_inline_rename_without_changing_name() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<RenameDialogState>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<Selection>()
            .add_systems(Update, handle_inline_rename_keyboard);
        let id = SceneNodeId::new();
        let entity = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Before".into(),
                },
                id,
            ))
            .id();
        app.world_mut()
            .spawn((SceneNodeRenameInput, EditableText::new("After")));
        {
            let mut rename = app.world_mut().resource_mut::<RenameDialogState>();
            rename.open = true;
            rename.target = Some(id);
            rename.name = "After".into();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();

        assert_eq!(
            app.world()
                .entity(entity)
                .get::<EditableObject>()
                .unwrap()
                .name,
            "Before"
        );
        assert!(!app.world().resource::<RenameDialogState>().open);
    }

    #[test]
    fn entity_picker_creates_child_under_selected_node() {
        let mut app = hierarchy_app();
        let root = create_root(&mut app);
        let root_id = *app.world().entity(root).get::<SceneNodeId>().unwrap();
        let child = add_empty(&mut app);
        assert_ne!(child, root);
        assert_ne!(
            *app.world().entity(child).get::<SceneNodeId>().unwrap(),
            root_id
        );
        assert_eq!(
            app.world().entity(child).get::<SceneParentId>().unwrap().0,
            Some(root_id)
        );
    }

    #[test]
    fn entity_picker_creates_three_d_nodes_in_a_two_d_document() {
        let mut app = hierarchy_app();
        let root = create_root(&mut app);
        let root_id = *app.world().entity(root).get::<SceneNodeId>().unwrap();

        let mesh = add_kind(&mut app, EntityKind::Mesh3D);
        assert_eq!(
            *app.world().entity(mesh).get::<SceneSpace>().unwrap(),
            SceneSpace::ThreeD
        );
        assert_eq!(
            app.world().entity(mesh).get::<SceneParentId>().unwrap().0,
            Some(root_id)
        );
        assert_eq!(
            *app.world().resource::<WorkspaceMode>(),
            WorkspaceMode::ThreeD
        );
        assert_eq!(
            *app.world().resource::<EditorViewMode>(),
            EditorViewMode::ThreeD
        );

        let entry = TreeEntry {
            entity: mesh,
            name: "Mesh3D".into(),
            kind: EntityKind::Mesh3D,
            id: *app.world().entity(mesh).get::<SceneNodeId>().unwrap(),
            parent: Some(root_id),
            order: 0,
        };
        assert_eq!(flatten_tree(&[entry], &HashSet::new()).len(), 1);
    }

    #[test]
    fn ui_nodes_use_root_or_ui_parents_for_layout_inheritance() {
        let mut app = hierarchy_app();
        let root = create_root(&mut app);
        let root_id = *app.world().entity(root).get::<SceneNodeId>().unwrap();
        let sprite = add_kind(&mut app, EntityKind::Sprite2D);

        app.world_mut().resource_mut::<Selection>().0 = Some(sprite);
        let panel = add_kind(&mut app, EntityKind::Panel);
        let panel_id = *app.world().entity(panel).get::<SceneNodeId>().unwrap();
        assert_eq!(
            app.world().entity(panel).get::<SceneParentId>().unwrap().0,
            Some(root_id)
        );

        let button = add_kind(&mut app, EntityKind::Button);
        assert_eq!(
            app.world().entity(button).get::<SceneParentId>().unwrap().0,
            Some(panel_id)
        );
    }

    #[test]
    fn selected_descendant_can_become_the_only_scene_root() {
        let mut app = hierarchy_app();
        let old_root = create_root(&mut app);
        let parent = add_empty(&mut app);
        let new_root = add_empty(&mut app);
        let old_root_id = *app.world().entity(old_root).get::<SceneNodeId>().unwrap();
        let parent_id = *app.world().entity(parent).get::<SceneNodeId>().unwrap();
        let new_root_id = *app.world().entity(new_root).get::<SceneNodeId>().unwrap();
        let action = app
            .world_mut()
            .spawn(SceneNodeActionButton(SceneNodeAction::MakeRoot))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.world_mut().flush();

        assert!(app.world().entity(new_root).contains::<SceneRoot>());
        assert!(!app.world().entity(old_root).contains::<SceneRoot>());
        assert_eq!(
            app.world()
                .entity(new_root)
                .get::<SceneParentId>()
                .unwrap()
                .0,
            None
        );
        assert_eq!(
            app.world().entity(parent).get::<SceneParentId>().unwrap().0,
            Some(new_root_id)
        );
        assert_eq!(
            app.world()
                .entity(old_root)
                .get::<SceneParentId>()
                .unwrap()
                .0,
            Some(parent_id)
        );
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<SceneRoot>>()
                .iter(world)
                .count(),
            1
        );
        assert_ne!(old_root_id, new_root_id);
    }

    #[test]
    fn deleting_parent_removes_the_complete_subtree() {
        let mut app = hierarchy_app();
        let root = create_root(&mut app);
        let parent = add_empty(&mut app);
        add_empty(&mut app);
        let delete = app
            .world_mut()
            .spawn(SceneNodeActionButton(SceneNodeAction::Delete))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(parent);

        app.world_mut().trigger(Activate { entity: delete });
        app.world_mut().flush();

        let world = app.world_mut();
        assert_eq!(world.query::<&EditableObject>().iter(world).count(), 1);
        assert_eq!(world.resource::<Selection>().0, Some(root));
    }

    #[test]
    fn duplicating_node_assigns_a_new_stable_id() {
        let mut app = hierarchy_app();
        create_root(&mut app);
        let original = add_empty(&mut app);
        let original_id = *app.world().entity(original).get::<SceneNodeId>().unwrap();
        let duplicate = app
            .world_mut()
            .spawn(SceneNodeActionButton(SceneNodeAction::Duplicate))
            .id();

        app.world_mut().trigger(Activate { entity: duplicate });
        app.world_mut().flush();

        let copy = app.world().resource::<Selection>().0.unwrap();
        assert_ne!(copy, original);
        assert_ne!(
            *app.world().entity(copy).get::<SceneNodeId>().unwrap(),
            original_id
        );
    }

    #[test]
    fn keyboard_duplicate_preserves_multi_selection_and_assigns_new_ids() {
        let mut app = hierarchy_app();
        let root = create_root(&mut app);
        let root_id = *app.world().entity(root).get::<SceneNodeId>().unwrap();
        let first = add_kind(&mut app, EntityKind::Sprite2D);
        app.world_mut().resource_mut::<Selection>().0 = Some(root);
        let second = add_kind(&mut app, EntityKind::Image);
        let original_ids = HashSet::from([
            *app.world().entity(first).get::<SceneNodeId>().unwrap(),
            *app.world().entity(second).get::<SceneNodeId>().unwrap(),
        ]);
        set_multi_selection(&mut app, [first, second]);

        let keyboard = &mut app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keyboard.press(KeyCode::ControlLeft);
        keyboard.press(KeyCode::KeyD);
        app.update();
        app.world_mut().flush();

        let selection = *app.world().resource::<Selection>();
        let copies: Vec<_> = app
            .world()
            .resource::<SelectionSet>()
            .entities(&selection)
            .collect();
        assert_eq!(copies.len(), 2);
        for entity in copies {
            let copy = app.world().entity(entity);
            assert!(!original_ids.contains(copy.get::<SceneNodeId>().unwrap()));
            assert_eq!(copy.get::<SceneParentId>().unwrap().0, Some(root_id));
        }
        let world = app.world_mut();
        assert_eq!(world.query::<&EditableObject>().iter(world).count(), 5);
    }

    #[test]
    fn keyboard_copy_paste_preserves_subtree_with_new_ids() {
        let mut app = hierarchy_app();
        let root = create_root(&mut app);
        let root_id = *app.world().entity(root).get::<SceneNodeId>().unwrap();
        let parent = add_empty(&mut app);
        let child = add_kind(&mut app, EntityKind::Sprite2D);
        let original_ids = HashSet::from([
            *app.world().entity(parent).get::<SceneNodeId>().unwrap(),
            *app.world().entity(child).get::<SceneNodeId>().unwrap(),
        ]);
        set_multi_selection(&mut app, [parent]);

        {
            let keyboard = &mut app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keyboard.press(KeyCode::ControlLeft);
            keyboard.press(KeyCode::KeyC);
        }
        app.update();
        {
            let keyboard = &mut app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keyboard.reset(KeyCode::KeyC);
            keyboard.press(KeyCode::KeyV);
        }
        app.update();
        app.world_mut().flush();

        let pasted_parent = app.world().resource::<Selection>().0.unwrap();
        let pasted_parent_id = *app
            .world()
            .entity(pasted_parent)
            .get::<SceneNodeId>()
            .unwrap();
        assert!(!original_ids.contains(&pasted_parent_id));
        assert_eq!(
            app.world()
                .entity(pasted_parent)
                .get::<SceneParentId>()
                .unwrap()
                .0,
            Some(root_id)
        );

        let pasted_children: Vec<_> = {
            let world = app.world_mut();
            world
                .query::<(Entity, &SceneNodeId, &SceneParentId)>()
                .iter(world)
                .filter(|(_, _, parent)| parent.0 == Some(pasted_parent_id))
                .map(|(entity, id, _)| (entity, *id))
                .collect()
        };
        assert_eq!(pasted_children.len(), 1);
        assert!(!original_ids.contains(&pasted_children[0].1));
        assert_ne!(pasted_children[0].0, child);

        let world = app.world_mut();
        assert_eq!(world.query::<&EditableObject>().iter(world).count(), 5);
    }

    #[test]
    fn keyboard_delete_filters_selected_descendants() {
        let mut app = hierarchy_app();
        let root = create_root(&mut app);
        let parent = add_empty(&mut app);
        let child = add_kind(&mut app, EntityKind::Sprite2D);
        set_multi_selection(&mut app, [parent, child]);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Delete);
        app.update();
        app.world_mut().flush();

        let world = app.world_mut();
        assert_eq!(world.query::<&EditableObject>().iter(world).count(), 1);
        assert_eq!(world.resource::<Selection>().0, Some(root));
        assert_eq!(
            world
                .resource::<SelectionSet>()
                .entities(&*world.resource::<Selection>())
                .collect::<Vec<_>>(),
            vec![root]
        );
    }

    #[test]
    fn keyboard_nudge_moves_sprite_and_ui_in_the_same_screen_direction() {
        let mut app = hierarchy_app();
        let root = create_root(&mut app);
        let sprite = add_kind(&mut app, EntityKind::Sprite2D);
        app.world_mut().resource_mut::<Selection>().0 = Some(root);
        let image = add_kind(&mut app, EntityKind::Image);
        set_multi_selection(&mut app, [sprite, image]);

        {
            let keyboard = &mut app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keyboard.press(KeyCode::ShiftLeft);
            keyboard.press(KeyCode::ArrowRight);
        }
        app.update();
        {
            let keyboard = &mut app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keyboard.reset(KeyCode::ArrowRight);
            keyboard.press(KeyCode::ArrowUp);
        }
        app.update();

        let sprite_transform = app.world().entity(sprite).get::<Transform>().unwrap();
        assert_eq!(
            sprite_transform.translation.truncate(),
            Vec2::new(10.0, 10.0)
        );
        let ui_layout = app.world().entity(image).get::<SceneUiLayout>().unwrap();
        assert_eq!(ui_layout.offset, (10.0, -10.0));
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    fn hierarchy_node(
        raw_entity: u32,
        id: SceneNodeId,
        parent: Option<SceneNodeId>,
        order: u32,
        kind: EntityKind,
        root: bool,
    ) -> HierarchyNodeState {
        HierarchyNodeState {
            entity: Entity::from_raw_u32(raw_entity).unwrap(),
            id,
            parent,
            order,
            space: SceneSpace::TwoD,
            kind,
            root,
        }
    }

    fn planned_update(
        updates: &[HierarchyNodeUpdate],
        entity: Entity,
    ) -> Option<HierarchyNodeUpdate> {
        updates
            .iter()
            .find(|update| update.entity == entity)
            .copied()
    }

    #[test]
    fn hierarchy_drop_can_reparent_a_node_as_a_child() {
        let root_id = SceneNodeId::new();
        let parent_id = SceneNodeId::new();
        let sibling_id = SceneNodeId::new();
        let child_id = SceneNodeId::new();
        let root = hierarchy_node(1, root_id, None, 0, EntityKind::Empty2D, true);
        let parent = hierarchy_node(2, parent_id, Some(root_id), 0, EntityKind::Empty2D, false);
        let sibling = hierarchy_node(3, sibling_id, Some(root_id), 1, EntityKind::Sprite2D, false);
        let child = hierarchy_node(4, child_id, Some(parent_id), 0, EntityKind::Sprite2D, false);
        let nodes = [root, parent, sibling, child];

        let updates = plan_hierarchy_drop(
            &nodes,
            sibling.entity,
            parent.entity,
            SceneTreeDropPlacement::Child,
        )
        .unwrap();

        assert_eq!(
            planned_update(&updates, sibling.entity),
            Some(HierarchyNodeUpdate {
                entity: sibling.entity,
                parent: Some(parent_id),
                order: 1,
            })
        );
    }

    #[test]
    fn hierarchy_drop_can_reorder_siblings_before_and_after() {
        let root_id = SceneNodeId::new();
        let first_id = SceneNodeId::new();
        let second_id = SceneNodeId::new();
        let third_id = SceneNodeId::new();
        let root = hierarchy_node(1, root_id, None, 0, EntityKind::Empty2D, true);
        let first = hierarchy_node(2, first_id, Some(root_id), 0, EntityKind::Empty2D, false);
        let second = hierarchy_node(3, second_id, Some(root_id), 1, EntityKind::Sprite2D, false);
        let third = hierarchy_node(4, third_id, Some(root_id), 2, EntityKind::Camera2D, false);
        let nodes = [root, first, second, third];

        let updates = plan_hierarchy_drop(
            &nodes,
            third.entity,
            first.entity,
            SceneTreeDropPlacement::Before,
        )
        .unwrap();

        assert_eq!(planned_update(&updates, third.entity).unwrap().order, 0);
        assert_eq!(planned_update(&updates, first.entity).unwrap().order, 1);
        assert_eq!(planned_update(&updates, second.entity).unwrap().order, 2);

        let after_updates = plan_hierarchy_drop(
            &nodes,
            first.entity,
            second.entity,
            SceneTreeDropPlacement::After,
        )
        .unwrap();
        assert_eq!(
            planned_update(&after_updates, second.entity).unwrap().order,
            0
        );
        assert_eq!(
            planned_update(&after_updates, first.entity).unwrap().order,
            1
        );
    }

    #[test]
    fn hierarchy_drop_rejects_root_moves_and_parent_cycles() {
        let root_id = SceneNodeId::new();
        let parent_id = SceneNodeId::new();
        let child_id = SceneNodeId::new();
        let root = hierarchy_node(1, root_id, None, 0, EntityKind::Empty2D, true);
        let parent = hierarchy_node(2, parent_id, Some(root_id), 0, EntityKind::Empty2D, false);
        let child = hierarchy_node(3, child_id, Some(parent_id), 0, EntityKind::Sprite2D, false);
        let nodes = [root, parent, child];

        assert!(
            plan_hierarchy_drop(
                &nodes,
                root.entity,
                parent.entity,
                SceneTreeDropPlacement::Child,
            )
            .is_none()
        );
        assert!(
            plan_hierarchy_drop(
                &nodes,
                parent.entity,
                child.entity,
                SceneTreeDropPlacement::Child,
            )
            .is_none()
        );
    }

    #[test]
    fn hierarchy_drop_keeps_ui_below_root_or_ui_nodes() {
        let root_id = SceneNodeId::new();
        let sprite_id = SceneNodeId::new();
        let image_id = SceneNodeId::new();
        let root = hierarchy_node(1, root_id, None, 0, EntityKind::Empty2D, true);
        let sprite = hierarchy_node(2, sprite_id, Some(root_id), 0, EntityKind::Sprite2D, false);
        let image = hierarchy_node(3, image_id, Some(root_id), 1, EntityKind::Image, false);
        let nodes = [root, sprite, image];

        assert!(
            plan_hierarchy_drop(
                &nodes,
                image.entity,
                sprite.entity,
                SceneTreeDropPlacement::Child,
            )
            .is_none()
        );
        assert!(
            plan_hierarchy_drop(
                &nodes,
                image.entity,
                root.entity,
                SceneTreeDropPlacement::Child,
            )
            .is_some()
        );
    }
}
