//! 编辑器场景文档、序列化和 Save As 流程。
//!
//! `SceneDocument` 只保存文档级状态；实体事实位于编辑世界中。保存时通过
//! `SceneSaveQuery` 收集实体，再调用 arisna_engine 的共享 BSN 转换函数。

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
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
    filesystem::{FileSystemState, RefreshFileSystem},
    hierarchy::{SceneNodeId, SceneParentId, SceneRoot, SceneRootMenuState, SceneSiblingOrder},
    selection::EditableObject,
    ui::{
        components::{SceneLevelLabel, SceneSaveDialogHost, SceneTabBar, SceneTabLabel},
        theme,
    },
    undo::SceneHistory,
    workspace::{EditorViewMode, SceneSpace, WorkspaceMode, WorkspaceSelections},
};
use arisna_engine::{
    ProjectRoot, SceneFile as SceneFileV2, SceneNodeData as SceneNodeFile, scene_file_from_bsn,
    scene_file_to_bsn,
};
use bevy::{
    picking::Pickable,
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui_widgets::{Activate, Button as WidgetButton},
};

const SCENE_EXTENSION: &str = ".bsn";

#[derive(Component, Clone, Copy, Default)]
pub struct SaveSceneButton;

#[derive(Component, Clone, Copy, Default)]
pub struct CloseSceneButton;

#[derive(Message, Clone, Debug)]
pub struct OpenSceneRequest {
    pub relative_path: PathBuf,
}

#[derive(Component, Clone, Copy, Default)]
struct SceneSaveFolderInput;

#[derive(Component, Clone, Copy, Default)]
struct SceneSaveNameInput;

#[derive(Component, Clone, Copy)]
struct SceneDialogAction(pub DialogAction);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogAction {
    Browse,
    Confirm,
    Cancel,
}

#[derive(Resource, Debug, Default)]
pub struct SceneDocument {
    pub open: bool,
    pub dirty: bool,
    pub path: Option<PathBuf>,
    pub name: String,
    dialog: SaveDialogState,
    revision: u64,
}

#[derive(Debug, Default)]
struct SaveDialogState {
    open: bool,
    folder: String,
    filename: String,
    error: String,
}

pub struct SceneDocumentPlugin;

#[derive(bevy::ecs::query::QueryData)]
/// 保存场景所需的只读投影。
///
/// 新增可持久化组件时，应同时扩展此查询、`serialize_scene`、加载路径和
/// round-trip 测试，避免 Inspector 看得到但保存后丢失。
pub(crate) struct SceneSaveQuery {
    pub entity: Entity,
    pub object: &'static EditableObject,
    pub space: &'static SceneSpace,
    pub transform: Option<&'static Transform>,
    pub preview_transform: Option<&'static AnimationPreviewOriginalTransform>,
    pub kind: Option<&'static EntityKind>,
    pub components: Option<&'static AddedEntityComponents>,
    pub custom_components: Option<&'static EntityCustomComponents>,
    pub entity_script: Option<&'static EntityScriptBinding>,
    pub systems: Option<&'static EntitySystemBindings>,
    pub ui_layout: Option<&'static SceneUiLayout>,
    pub preview_ui_layout: Option<&'static AnimationPreviewOriginalUiLayout>,
    pub ui_content: Option<&'static SceneUiContent>,
    pub sprite: Option<&'static SceneSprite2D>,
    pub preview_sprite_frame: Option<&'static AnimationPreviewOriginalSpriteFrame>,
    pub model: Option<&'static SceneModel3D>,
    pub animation_player: Option<&'static SceneAnimationPlayer>,
    pub collision_rect: Option<&'static SceneCollisionRect2D>,
    pub id: &'static SceneNodeId,
    pub parent: &'static SceneParentId,
    pub order: &'static SceneSiblingOrder,
    pub root: Option<&'static SceneRoot>,
}

impl Plugin for SceneDocumentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneDocument>()
            .add_message::<OpenSceneRequest>()
            .add_observer(handle_scene_document_action)
            .add_systems(
                Update,
                (
                    restore_last_scene,
                    handle_open_scene_requests,
                    handle_scene_shortcuts,
                    sync_dialog_inputs,
                    rebuild_scene_dialog,
                    sync_scene_chrome,
                )
                    .chain(),
            );
    }
}

impl SceneDocument {
    pub(crate) fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn save_dialog_open(&self) -> bool {
        self.dialog.open
    }
}

fn handle_scene_document_action(
    activate: On<Activate>,
    save_buttons: Query<(), With<SaveSceneButton>>,
    close_buttons: Query<(), With<CloseSceneButton>>,
    actions: Query<&SceneDialogAction>,
    folder_inputs: Query<&EditableText, With<SceneSaveFolderInput>>,
    name_inputs: Query<&EditableText, With<SceneSaveNameInput>>,
    project: Res<ProjectRoot>,
    mut document: ResMut<SceneDocument>,
    objects: Query<SceneSaveQuery>,
    filesystem: Option<ResMut<FileSystemState>>,
    mut refresh: MessageWriter<RefreshFileSystem>,
    mut modes: (ResMut<WorkspaceMode>, ResMut<EditorViewMode>),
    mut selection: ResMut<crate::selection::Selection>,
    mut workspace_selections: ResMut<WorkspaceSelections>,
    mut root_menu: ResMut<SceneRootMenuState>,
    mut history: Option<ResMut<SceneHistory>>,
    mut commands: Commands,
) {
    if save_buttons.get(activate.entity).is_ok() {
        save_or_open(
            &project,
            &mut document,
            &objects,
            filesystem,
            &mut refresh,
            history.as_deref_mut(),
        );
        return;
    }
    if close_buttons.get(activate.entity).is_ok() {
        for node in &objects {
            commands.entity(node.entity).despawn();
        }
        *modes.0 = WorkspaceMode::TwoD;
        *modes.1 = EditorViewMode::TwoD;
        selection.0 = None;
        workspace_selections.set(WorkspaceMode::TwoD, None);
        workspace_selections.set(WorkspaceMode::ThreeD, None);
        root_menu.open = false;
        root_menu.context_open = false;
        document.open = false;
        document.dirty = false;
        document.path = None;
        document.name.clear();
        document.dialog.open = false;
        document.dialog.error.clear();
        document.revision = document.revision.wrapping_add(1);
        persist_last_scene(&project, None);
        if let Some(history) = history.as_deref_mut() {
            history.request_clear();
        }
        return;
    }
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };

    match action.0 {
        DialogAction::Cancel => {
            document.dialog.open = false;
            document.dialog.error.clear();
            document.revision = document.revision.wrapping_add(1);
        }
        DialogAction::Browse => {
            let initial = PathBuf::from(document.dialog.folder.trim());
            if let Some(path) = pick_folder("Choose a folder for the scene", &initial) {
                document.dialog.folder = path.to_string_lossy().into_owned();
                document.dialog.error.clear();
                document.revision = document.revision.wrapping_add(1);
            }
        }
        DialogAction::Confirm => {
            if let Some(value) = folder_inputs.iter().next() {
                document.dialog.folder = value.value().to_string();
            }
            if let Some(value) = name_inputs.iter().next() {
                document.dialog.filename = value.value().to_string();
            }
            let result = save_scene(&mut document, &objects);
            if let Err(error) = result {
                document.dialog.error = error;
                document.revision = document.revision.wrapping_add(1);
            } else {
                if let Some(history) = history.as_deref_mut() {
                    history.request_mark_saved();
                }
                persist_last_scene(&project, document.path.as_deref());
                refresh.write(RefreshFileSystem);
                if let Some(mut state) = filesystem {
                    state.revision = state.revision.wrapping_add(1);
                    state.status = "Scene saved".into();
                }
            }
        }
    }
}

fn handle_scene_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    project: Res<ProjectRoot>,
    mut document: ResMut<SceneDocument>,
    objects: Query<SceneSaveQuery>,
    filesystem: Option<ResMut<FileSystemState>>,
    mut refresh: MessageWriter<RefreshFileSystem>,
    mut history: Option<ResMut<SceneHistory>>,
) {
    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if control && keyboard.just_pressed(KeyCode::KeyS) {
        save_or_open(
            &project,
            &mut document,
            &objects,
            filesystem,
            &mut refresh,
            history.as_deref_mut(),
        );
    }
    if document.dialog.open && keyboard.just_pressed(KeyCode::Escape) {
        document.dialog.open = false;
        document.dialog.error.clear();
        document.revision = document.revision.wrapping_add(1);
    }
}

fn restore_last_scene(
    project: Res<ProjectRoot>,
    mut writer: MessageWriter<OpenSceneRequest>,
    mut attempted: Local<bool>,
) {
    if *attempted {
        return;
    }
    *attempted = true;
    let Some(relative_path) = read_last_scene(&project) else {
        return;
    };
    if !relative_path.is_empty() {
        writer.write(OpenSceneRequest {
            relative_path: PathBuf::from(relative_path),
        });
    }
}

fn handle_open_scene_requests(
    mut events: MessageReader<OpenSceneRequest>,
    project: Res<ProjectRoot>,
    mut document: ResMut<SceneDocument>,
    mut mode: ResMut<WorkspaceMode>,
    mut view: ResMut<EditorViewMode>,
    mut selection: ResMut<crate::selection::Selection>,
    mut workspace_selections: ResMut<WorkspaceSelections>,
    mut root_menu: ResMut<SceneRootMenuState>,
    existing: Query<Entity, With<EditableObject>>,
    mut filesystem: ResMut<FileSystemState>,
    mut history: Option<ResMut<SceneHistory>>,
    mut commands: Commands,
) {
    let requests: Vec<_> = events.read().cloned().collect();
    for request in requests {
        let relative = request.relative_path.to_string_lossy().replace('\\', "/");
        let Some(path) = project
            .resolve_existing(&relative)
            .filter(|path| path.is_file())
        else {
            filesystem.status = format!("Scene not found: {relative}");
            filesystem.revision = filesystem.revision.wrapping_add(1);
            continue;
        };

        let loaded = match load_scene_file(&path) {
            Ok(scene) => scene,
            Err(error) => {
                filesystem.status = format!("Open scene failed: {error}");
                filesystem.revision = filesystem.revision.wrapping_add(1);
                continue;
            }
        };

        for entity in &existing {
            commands.entity(entity).despawn();
        }

        let next_mode = loaded.mode;
        let mut root_entity = None;
        for node in loaded.nodes {
            let is_root = node.id == loaded.root_id;
            let entity = spawn_entity(
                &mut commands,
                node.kind,
                node.name,
                node.id,
                node.parent,
                node.order,
                node.space,
            );
            for component in node.components.iter().copied() {
                insert_builtin_component(&mut commands, entity, component);
            }
            commands
                .entity(entity)
                .insert(AddedEntityComponents(node.components.clone()));
            commands
                .entity(entity)
                .insert(EntityCustomComponents(node.custom_components.clone()));
            commands
                .entity(entity)
                .insert(EntityScriptBinding(node.entity_script.clone()));
            commands
                .entity(entity)
                .insert(EntitySystemBindings(node.systems.clone()));
            if let Some(layout) = node.ui_layout {
                commands.entity(entity).insert(layout);
            }
            if let Some(content) = node.ui_content {
                commands.entity(entity).insert(content);
            }
            if let Some(sprite) = node.sprite {
                commands.entity(entity).insert(sprite);
            }
            if let Some(model) = node.model {
                commands.entity(entity).insert(model);
            }
            if let Some(animation_player) = node.animation_player {
                commands.entity(entity).insert(animation_player);
            }
            if let Some(collision) = node.collision_rect {
                commands.entity(entity).insert(collision);
            }
            if node.kind.is_spatial() || !node.components.is_empty() {
                commands
                    .entity(entity)
                    .insert((node.transform, Visibility::Visible));
            }
            if is_root {
                commands.entity(entity).insert(SceneRoot);
                root_entity = Some(entity);
            }
        }

        let Some(root_entity) = root_entity else {
            filesystem.status = "Open scene failed: root entity is missing".into();
            filesystem.revision = filesystem.revision.wrapping_add(1);
            continue;
        };

        *mode = next_mode;
        *view = match next_mode {
            WorkspaceMode::TwoD => EditorViewMode::TwoD,
            WorkspaceMode::ThreeD => EditorViewMode::ThreeD,
        };
        selection.0 = Some(root_entity);
        workspace_selections.set(next_mode, Some(root_entity));
        root_menu.open = false;
        root_menu.context_open = false;
        document.open = true;
        document.dirty = false;
        document.path = Some(path.clone());
        document.name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled Level")
            .to_owned();
        document.dialog.open = false;
        document.dialog.error.clear();
        document.revision = document.revision.wrapping_add(1);
        if let Some(history) = history.as_deref_mut() {
            history.request_reset(true);
        }
        persist_last_scene(&project, Some(&path));
        filesystem.selected = Some(relative);
        filesystem.status = format!("Opened {}", path.display());
        filesystem.revision = filesystem.revision.wrapping_add(1);
    }
}

#[derive(Debug)]
struct LoadedScene {
    root_id: SceneNodeId,
    mode: WorkspaceMode,
    nodes: Vec<LoadedNode>,
}

#[derive(Debug)]
struct LoadedNode {
    id: SceneNodeId,
    parent: Option<SceneNodeId>,
    order: u32,
    name: String,
    kind: EntityKind,
    components: Vec<BuiltinComponent>,
    custom_components: Vec<arisna_engine::SceneCustomComponent>,
    entity_script: Option<arisna_engine::SceneEntityScript>,
    systems: Vec<arisna_engine::SceneSystemBinding>,
    ui_layout: Option<SceneUiLayout>,
    ui_content: Option<SceneUiContent>,
    sprite: Option<SceneSprite2D>,
    model: Option<SceneModel3D>,
    animation_player: Option<SceneAnimationPlayer>,
    collision_rect: Option<SceneCollisionRect2D>,
    space: SceneSpace,
    transform: Transform,
}

fn load_scene_file(path: &Path) -> Result<LoadedScene, String> {
    let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bsn"))
    {
        return load_scene_v2_file(scene_file_from_bsn(&source)?);
    }
    let version = source
        .lines()
        .find_map(|line| unsigned_value(line, "format_version:"))
        .unwrap_or(1);
    match version {
        1 => load_legacy_scene(&source),
        2 => load_scene_v2(&source),
        _ => Err(format!("unsupported scene format version: {version}")),
    }
}

fn load_scene_v2(source: &str) -> Result<LoadedScene, String> {
    let file: SceneFileV2 = if source.contains("format_version:") {
        ron::from_str(source).map_err(|error| format!("invalid legacy scene: {error}"))?
    } else {
        scene_file_from_bsn(source)?
    };
    load_scene_v2_file(file)
}

fn load_scene_v2_file(file: SceneFileV2) -> Result<LoadedScene, String> {
    if file.format_version != 2 {
        return Err(format!(
            "unsupported scene format version: {}",
            file.format_version
        ));
    }
    let root_id = SceneNodeId::parse(&file.root)?;
    let mut ids = HashSet::new();
    let mut nodes = Vec::with_capacity(file.entities.len());
    for node in file.entities {
        let id = SceneNodeId::parse(&node.id)?;
        if !ids.insert(id) {
            return Err(format!("duplicate scene entity ID: {id}"));
        }
        let parent = node.parent.as_deref().map(SceneNodeId::parse).transpose()?;
        nodes.push(LoadedNode {
            id,
            parent,
            order: node.order,
            name: node.name,
            kind: EntityKind::from_scene_kind(&node.kind)?,
            components: node
                .components
                .iter()
                .map(|component| BuiltinComponent::from_scene_name(component))
                .collect::<Result<_, _>>()?,
            custom_components: node.custom_components,
            entity_script: node.entity_script,
            systems: node.systems,
            ui_layout: node.ui_layout,
            ui_content: node.ui_content.or_else(|| {
                EntityKind::from_scene_kind(&node.kind)
                    .ok()?
                    .default_ui_content()
            }),
            sprite: node.sprite.or_else(|| {
                let kind = EntityKind::from_scene_kind(&node.kind).ok()?;
                (kind == EntityKind::Sprite2D
                    || node
                        .components
                        .iter()
                        .any(|component| component == "sprite"))
                .then(SceneSprite2D::default)
            }),
            model: node.model,
            animation_player: node.animation_player,
            collision_rect: node.collision_rect,
            space: parse_node_space(&node.kind, node.space.as_deref())?,
            transform: Transform {
                translation: Vec3::new(node.translation.0, node.translation.1, node.translation.2),
                rotation: Quat::from_xyzw(
                    node.rotation.0,
                    node.rotation.1,
                    node.rotation.2,
                    node.rotation.3,
                ),
                scale: Vec3::new(node.scale.0, node.scale.1, node.scale.2),
            },
        });
    }
    validate_loaded_scene(root_id, &nodes)?;
    let mode = nodes
        .iter()
        .find(|node| node.id == root_id)
        .map(|node| workspace_mode(node.space))
        .ok_or_else(|| "root entity is missing".to_string())?;
    Ok(LoadedScene {
        root_id,
        mode,
        nodes,
    })
}

fn load_legacy_scene(source: &str) -> Result<LoadedScene, String> {
    let root_name = source
        .lines()
        .find_map(|line| quoted_value(line, "root:"))
        .ok_or_else(|| "root field is missing".to_string())?;
    let mut nodes = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("(name:") {
            continue;
        }
        let name =
            quoted_value(trimmed, "name:").ok_or_else(|| "entity name is missing".to_string())?;
        let kind = quoted_value(trimmed, "kind:")
            .ok_or_else(|| format!("entity {name} kind is missing"))?;
        let translation = tuple_values(trimmed, "translation:", 3)?;
        let rotation = tuple_values(trimmed, "rotation:", 4)?;
        let scale = tuple_values(trimmed, "scale:", 3)?;
        let entity_kind = EntityKind::from_scene_kind(&kind)?;
        nodes.push(LoadedNode {
            id: SceneNodeId::new(),
            parent: None,
            order: nodes.len() as u32,
            name,
            kind: entity_kind,
            components: Vec::new(),
            custom_components: Vec::new(),
            entity_script: None,
            systems: Vec::new(),
            ui_layout: entity_kind.default_ui_layout(),
            ui_content: entity_kind.default_ui_content(),
            sprite: (entity_kind == EntityKind::Sprite2D).then(SceneSprite2D::default),
            model: None,
            animation_player: None,
            collision_rect: None,
            space: parse_node_space(&kind, None)?,
            transform: Transform {
                translation: Vec3::new(translation[0], translation[1], translation[2]),
                rotation: Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]),
                scale: Vec3::new(scale[0], scale[1], scale[2]),
            },
        });
    }
    if nodes.is_empty() {
        return Err("scene contains no entities".into());
    }
    let root_index = nodes
        .iter()
        .position(|node| node.name == root_name)
        .ok_or_else(|| "root entity is missing".to_string())?;
    let root_id = nodes[root_index].id;
    for (index, node) in nodes.iter_mut().enumerate() {
        node.parent = (index != root_index).then_some(root_id);
    }
    let mode = workspace_mode(nodes[root_index].space);
    Ok(LoadedScene {
        root_id,
        mode,
        nodes,
    })
}

fn validate_loaded_scene(root_id: SceneNodeId, nodes: &[LoadedNode]) -> Result<(), String> {
    let ids: HashSet<_> = nodes.iter().map(|node| node.id).collect();
    let root = nodes
        .iter()
        .find(|node| node.id == root_id)
        .ok_or_else(|| "root entity is missing".to_string())?;
    if root.parent.is_some() {
        return Err("root entity cannot have a parent".into());
    }
    for node in nodes {
        if node.id != root_id && node.parent.is_none() {
            return Err(format!("entity {} has no parent", node.name));
        }
        if let Some(parent) = node.parent {
            if parent == node.id {
                return Err(format!("entity {} cannot parent itself", node.name));
            }
            if !ids.contains(&parent) {
                return Err(format!("entity {} references a missing parent", node.name));
            }
        }
    }
    let parents: HashMap<_, _> = nodes.iter().map(|node| (node.id, node.parent)).collect();
    for node in nodes {
        let mut seen = HashSet::new();
        let mut current = Some(node.id);
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(format!("scene hierarchy contains a cycle at {}", node.name));
            }
            current = parents.get(&id).copied().flatten();
        }
        if !seen.contains(&root_id) {
            return Err(format!("entity {} is not connected to the root", node.name));
        }
    }
    Ok(())
}

fn parse_node_space(kind: &str, explicit_space: Option<&str>) -> Result<SceneSpace, String> {
    if let Some(space) = explicit_space {
        return match space {
            "2d" => Ok(SceneSpace::TwoD),
            "3d" => Ok(SceneSpace::ThreeD),
            _ => Err(format!("unsupported entity space: {space}")),
        };
    }
    match kind {
        "2d" | "empty2d" | "collision_rect2d" | "sprite2d" | "camera2d" | "empty_ui" | "panel"
        | "text" | "button" | "image" => Ok(SceneSpace::TwoD),
        "3d"
        | "empty3d"
        | "mesh3d"
        | "camera3d"
        | "directional_light3d"
        | "point_light3d"
        | "spot_light3d" => Ok(SceneSpace::ThreeD),
        "empty" | "animation_player" => {
            Err(format!("logical {kind} entity requires an explicit space"))
        }
        _ => Err(format!("unsupported scene kind: {kind}")),
    }
}

fn workspace_mode(space: SceneSpace) -> WorkspaceMode {
    match space {
        SceneSpace::TwoD => WorkspaceMode::TwoD,
        SceneSpace::ThreeD => WorkspaceMode::ThreeD,
    }
}

fn unsigned_value(line: &str, key: &str) -> Option<u32> {
    let start = line.find(key)? + key.len();
    line[start..].trim().trim_end_matches(',').parse().ok()
}

fn quoted_value(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let value = &line[start..];
    let first = value.find('"')? + 1;
    let rest = &value[first..];
    let end = rest.find('"')?;
    Some(rest[..end].replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn tuple_values(line: &str, key: &str, expected: usize) -> Result<Vec<f32>, String> {
    let start = line
        .find(key)
        .and_then(|index| line[index..].find('(').map(|offset| index + offset + 1))
        .ok_or_else(|| format!("{key} tuple is missing"))?;
    let end = line[start..]
        .find(')')
        .map(|offset| start + offset)
        .ok_or_else(|| format!("{key} tuple is incomplete"))?;
    let values: Result<Vec<_>, _> = line[start..end]
        .split(',')
        .map(|value| value.trim().parse::<f32>())
        .collect();
    let values = values.map_err(|error| format!("invalid {key} value: {error}"))?;
    if values.len() != expected {
        return Err(format!("{key} expects {expected} values"));
    }
    Ok(values)
}

pub(crate) fn persist_last_scene(project: &ProjectRoot, path: Option<&Path>) {
    let state_dir = project.root.join(".arisna");
    if fs::create_dir_all(&state_dir).is_err() {
        return;
    }
    let relative = path
        .and_then(|path| path.strip_prefix(&project.root).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let content = format!("(last_scene: \"{}\")\n", ron_string(&relative));
    let _ = fs::write(state_dir.join("editor_state.ron"), content);
}

fn read_last_scene(project: &ProjectRoot) -> Option<String> {
    let source = fs::read_to_string(project.root.join(".arisna/editor_state.ron")).ok()?;
    source
        .lines()
        .find_map(|line| quoted_value(line, "last_scene:"))
}

fn sync_dialog_inputs(
    folder: Query<&EditableText, (With<SceneSaveFolderInput>, Changed<EditableText>)>,
    name: Query<&EditableText, (With<SceneSaveNameInput>, Changed<EditableText>)>,
    mut document: ResMut<SceneDocument>,
) {
    for value in &folder {
        document.dialog.folder = value.value().to_string();
    }
    for value in &name {
        document.dialog.filename = value.value().to_string();
    }
}

pub(crate) fn request_save(project: &ProjectRoot, document: &mut SceneDocument) {
    document.dialog.open = true;
    document.dialog.folder = project
        .root
        .join("assets")
        .join("scenes")
        .to_string_lossy()
        .into_owned();
    if document.dialog.filename.trim().is_empty() {
        document.dialog.filename = "untitled.bsn".into();
    }
    document.dialog.error.clear();
    document.revision = document.revision.wrapping_add(1);
}

fn save_or_open(
    project: &ProjectRoot,
    document: &mut SceneDocument,
    objects: &Query<SceneSaveQuery>,
    mut filesystem: Option<ResMut<FileSystemState>>,
    refresh: &mut MessageWriter<RefreshFileSystem>,
    mut history: Option<&mut SceneHistory>,
) {
    if document.path.is_some() {
        match save_scene(document, objects) {
            Ok(()) => {
                if let Some(history) = history.as_deref_mut() {
                    history.request_mark_saved();
                }
                persist_last_scene(project, document.path.as_deref());
                refresh.write(RefreshFileSystem);
                if let Some(state) = filesystem.as_deref_mut() {
                    state.revision = state.revision.wrapping_add(1);
                    state.status = "Scene saved".into();
                }
            }
            Err(error) => {
                document.dialog.open = true;
                document.dialog.error = error;
                document.revision = document.revision.wrapping_add(1);
            }
        }
    } else {
        request_save(project, document);
    }
}

pub(crate) fn save_scene(
    document: &mut SceneDocument,
    objects: &Query<SceneSaveQuery>,
) -> Result<(), String> {
    // 场景根使用稳定 SceneNodeId，不使用本次运行临时分配的 Bevy Entity。
    let Some(root_id) = objects.iter().find_map(|node| node.root.map(|_| *node.id)) else {
        return Err("Create a 2D or 3D scene root before saving.".into());
    };
    let content = serialize_scene(root_id, objects)?;
    if let Some(path) = document.path.clone() {
        atomic_write_scene(&path, content.as_bytes())?;
        document.dirty = false;
        document.dialog.error.clear();
        document.revision = document.revision.wrapping_add(1);
        return Ok(());
    }

    let folder_text = document.dialog.folder.trim();
    if folder_text.is_empty() {
        return Err("Choose a folder for this scene.".into());
    }
    let folder = PathBuf::from(folder_text);
    if !folder.is_absolute() {
        return Err("Choose an absolute scene folder or use Browse.".into());
    }
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create folder: {error}"))?;

    let filename = normalize_scene_filename(&document.dialog.filename)?;
    let path = folder.join(filename);
    atomic_write_scene(&path, content.as_bytes())?;

    document.path = Some(path.clone());
    document.name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("untitled.bsn")
        .to_owned();
    document.dirty = false;
    document.dialog.open = false;
    document.dialog.error.clear();
    document.revision = document.revision.wrapping_add(1);
    Ok(())
}

fn atomic_write_scene(path: &Path, content: &[u8]) -> Result<(), String> {
    // 先在目标目录写完整临时文件，再原子替换旧文件。保存中途失败时，
    // 用户原有场景仍保持可读取状态。
    let parent = path
        .parent()
        .ok_or_else(|| "Scene path has no parent folder.".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Scene file name is not valid UTF-8.".to_string())?;
    let temporary = parent.join(format!(".{file_name}.arisna-save-{}", std::process::id()));
    fs::write(&temporary, content)
        .map_err(|error| format!("Could not write scene temporary file: {error}"))?;

    let result = replace_scene_file(&temporary, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| format!("Could not save scene: {error}"))
}

#[cfg(not(windows))]
fn replace_scene_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(windows)]
fn replace_scene_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Both paths are in the scene folder, so replacement stays on the project's drive.
    let success = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if success == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn normalize_scene_filename(value: &str) -> Result<String, String> {
    let mut filename = value.trim().to_owned();
    if filename.is_empty() {
        return Err("Enter a scene file name.".into());
    }
    if filename.contains(['/', '\\']) || filename == "." || filename == ".." {
        return Err("Use a file name, not a folder path.".into());
    }
    if !filename.to_ascii_lowercase().ends_with(SCENE_EXTENSION) {
        filename.push_str(SCENE_EXTENSION);
    }
    Ok(filename)
}

fn serialize_scene(
    root_id: SceneNodeId,
    objects: &Query<SceneSaveQuery>,
) -> Result<String, String> {
    // 这里是编辑世界到磁盘契约的唯一出口；排序保证同一场景重复保存稳定。
    let mut entries = Vec::new();
    for node in objects.iter() {
        let kind = node.kind.copied().unwrap_or(match node.space {
            SceneSpace::TwoD => EntityKind::Empty2D,
            SceneSpace::ThreeD => EntityKind::Empty3D,
        });
        let transform = node
            .preview_transform
            .map(|original| original.0)
            .or(node.transform.copied())
            .unwrap_or_default();
        let rotation = transform.rotation;
        entries.push(SceneNodeFile {
            id: node.id.to_string(),
            parent: node.parent.0.map(|parent| parent.to_string()),
            order: node.order.0,
            name: node.object.name.clone(),
            kind: kind.scene_kind().to_owned(),
            components: node
                .components
                .map(|components| {
                    components
                        .0
                        .iter()
                        .map(|component| component.scene_name().to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            custom_components: node
                .custom_components
                .map(|components| components.0.clone())
                .unwrap_or_default(),
            entity_script: node.entity_script.and_then(|binding| binding.0.clone()),
            systems: node
                .systems
                .map(|bindings| bindings.0.clone())
                .unwrap_or_default(),
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
            space: Some(match node.space {
                SceneSpace::TwoD => "2d".into(),
                SceneSpace::ThreeD => "3d".into(),
            }),
            translation: (
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ),
            rotation: (rotation.x, rotation.y, rotation.z, rotation.w),
            scale: (transform.scale.x, transform.scale.y, transform.scale.z),
        });
    }
    if entries.is_empty() {
        return Err("The scene has no editable entities.".into());
    }
    entries.sort_by(|left, right| {
        left.parent
            .cmp(&right.parent)
            .then_with(|| left.order.cmp(&right.order))
            .then_with(|| left.id.cmp(&right.id))
    });
    let file = SceneFileV2 {
        format_version: 2,
        root: root_id.to_string(),
        entities: entries,
    };
    serialize_scene_file(&file)
}

fn serialize_scene_file(file: &SceneFileV2) -> Result<String, String> {
    scene_file_to_bsn(file).map_err(|error| format!("Could not serialize scene: {error}"))
}

fn ron_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn sync_scene_chrome(
    document: Res<SceneDocument>,
    mut tabs: Query<&mut Node, (With<SceneTabBar>, Without<SceneSaveDialogHost>)>,
    mut tab_labels: Query<&mut Text, (With<SceneTabLabel>, Without<SceneLevelLabel>)>,
    mut level_labels: Query<&mut Text, (With<SceneLevelLabel>, Without<SceneTabLabel>)>,
    mut dialog_hosts: Query<
        (&mut Node, &mut BackgroundColor),
        (With<SceneSaveDialogHost>, Without<SceneTabBar>),
    >,
) {
    if !document.is_changed() {
        return;
    }
    for mut node in &mut tabs {
        node.display = if document.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    let label = if document.name.is_empty() {
        "Untitled Level"
    } else {
        &document.name
    };
    let label = if document.dirty {
        format!("{label} *")
    } else {
        label.to_owned()
    };
    for mut text in &mut tab_labels {
        text.0 = label.clone();
    }
    for mut text in &mut level_labels {
        text.0 = format!("Level: {label}");
    }
    for (mut node, mut background) in &mut dialog_hosts {
        node.display = if document.dialog.open {
            Display::Flex
        } else {
            Display::None
        };
        *background = BackgroundColor(if document.dialog.open {
            Color::srgba(0.02, 0.025, 0.035, 0.72)
        } else {
            Color::NONE
        });
    }
}

fn rebuild_scene_dialog(
    mut commands: Commands,
    document: Res<SceneDocument>,
    hosts: Query<Entity, With<SceneSaveDialogHost>>,
    mut revision: Local<u64>,
) {
    if document.revision == *revision {
        return;
    }
    let Ok(host) = hosts.single() else { return };
    *revision = document.revision;
    commands.entity(host).despawn_related::<Children>();
    if !document.dialog.open {
        return;
    }
    commands.entity(host).with_children(|root| {
        root.spawn((
            Node {
                width: Val::Px(620.0),
                max_width: Val::Percent(90.0),
                padding: UiRect::all(Val::Px(20.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
            BorderColor::all(theme::border()),
            Pickable::default(),
        ))
        .with_children(|panel| {
            dialog_text(panel, "Save Scene As", 18.0, theme::text_primary());
            dialog_text(
                panel,
                "Choose where to save this scene resource.",
                11.0,
                theme::text_muted(),
            );
            dialog_field_row(
                panel,
                "Folder",
                SceneSaveFolderInput,
                &document.dialog.folder,
                true,
            );
            dialog_field_row(
                panel,
                "File name",
                SceneSaveNameInput,
                &document.dialog.filename,
                false,
            );
            dialog_text(panel, "Revy Scene (*.bsn)", 10.5, theme::text_muted());
            if !document.dialog.error.is_empty() {
                dialog_text(panel, &document.dialog.error, 11.0, theme::warning());
            }
            panel.spawn(Node {
                flex_grow: 1.0,
                min_height: Val::Px(5.0),
                ..default()
            });
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::FlexEnd,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|buttons| {
                    dialog_button(buttons, DialogAction::Cancel, "Cancel", false);
                    dialog_button(buttons, DialogAction::Confirm, "Save", true);
                });
        });
    });
}

fn dialog_field_row<M: Component + Clone>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    marker: M,
    value: &str,
    browse: bool,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(38.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            dialog_text(row, label, 11.0, theme::text_muted());
            row.spawn((
                marker,
                EditableText::new(value),
                TextCursorStyle::default(),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    height: Val::Px(36.0),
                    padding: UiRect::axes(Val::Px(9.0), Val::Px(7.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ));
            if browse {
                dialog_button(row, DialogAction::Browse, "Browse", false);
            }
        });
}

fn dialog_button(
    parent: &mut ChildSpawnerCommands,
    action: DialogAction,
    label: &str,
    primary: bool,
) {
    let color = if primary {
        theme::accent()
    } else {
        theme::bg_field()
    };
    parent.spawn((
        Button,
        WidgetButton,
        SceneDialogAction(action),
        Node {
            min_width: Val::Px(86.0),
            height: Val::Px(34.0),
            padding: UiRect::horizontal(Val::Px(13.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(color),
        BorderColor::all(theme::border_soft()),
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(11.5),
                ..default()
            },
            TextColor(theme::text_primary())
        )],
    ));
}

fn dialog_text(parent: &mut ChildSpawnerCommands, value: &str, size: f32, color: Color) {
    parent.spawn((
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    ));
}

#[cfg(windows)]
fn pick_folder(title: &str, initial: &Path) -> Option<PathBuf> {
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description='{}'; $d.SelectedPath='{}'; if($d.ShowDialog() -eq 'OK'){{ $d.SelectedPath }}",
        title.replace('\'', "''"),
        initial.to_string_lossy().replace('\'', "''"),
    );
    Command::new("powershell")
        .args(["-NoProfile", "-STA", "-Command", &script])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!path.is_empty()).then(|| PathBuf::from(path))
        })
}

#[cfg(not(windows))]
fn pick_folder(_title: &str, _initial: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::SystemState;

    const ROOT_ID: &str = "00000000000000000000000000000001";
    const CHILD_ID: &str = "00000000000000000000000000000002";

    fn scene_node(id: &str, parent: Option<&str>, name: &str) -> SceneNodeFile {
        SceneNodeFile {
            id: id.into(),
            parent: parent.map(str::to_owned),
            order: 0,
            name: name.into(),
            kind: "2d".into(),
            space: None,
            components: Vec::new(),
            custom_components: Vec::new(),
            entity_script: None,
            systems: Vec::new(),
            ui_layout: None,
            ui_content: None,
            sprite: None,
            model: None,
            animation_player: None,
            collision_rect: None,
            translation: (0.0, 0.0, 0.0),
            rotation: (0.0, 0.0, 0.0, 1.0),
            scale: (1.0, 1.0, 1.0),
        }
    }

    fn scene_source(nodes: Vec<SceneNodeFile>) -> String {
        serialize_scene_file(&SceneFileV2 {
            format_version: 2,
            root: ROOT_ID.into(),
            entities: nodes,
        })
        .unwrap()
    }

    #[test]
    fn normalizes_scene_extension() {
        assert_eq!(normalize_scene_filename("main"), Ok("main.bsn".into()));
        assert_eq!(normalize_scene_filename("main.bsn"), Ok("main.bsn".into()));
        assert!(normalize_scene_filename("folder/main").is_err());
    }

    #[test]
    fn atomic_scene_write_replaces_existing_content() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-temp")
            .join(format!("atomic-scene-save-{unique}"));
        let path = folder.join("main.bsn");
        fs::create_dir_all(&folder).unwrap();
        fs::write(&path, "old").unwrap();

        atomic_write_scene(&path, b"new scene").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new scene");
        assert_eq!(
            fs::read_dir(&folder).unwrap().count(),
            1,
            "the temporary save file should be removed"
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn loads_saved_two_d_scene_data() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("arisna_scene_load_{unique}.scn.ron"));
        fs::write(
            &path,
            "(\n  root: \"Node2D\",\n  entities: [\n    (name: \"Node2D\", kind: \"2d\", translation: (1.0, 2.0, 0.0), rotation: (0.0, 0.0, 0.0, 1.0), scale: (1.0, 1.0, 1.0)),\n  ],\n)\n",
        )
        .unwrap();

        let loaded = load_scene_file(&path).unwrap();
        assert_eq!(loaded.nodes[0].id, loaded.root_id);
        assert_eq!(loaded.mode, WorkspaceMode::TwoD);
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(
            loaded.nodes[0].transform.translation,
            Vec3::new(1.0, 2.0, 0.0)
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn v2_scene_preserves_stable_ids_and_parent_order() {
        let source = scene_source(vec![
            scene_node(ROOT_ID, None, "Root"),
            SceneNodeFile {
                order: 7,
                ..scene_node(CHILD_ID, Some(ROOT_ID), "Child")
            },
        ]);

        let loaded = load_scene_v2(&source).unwrap();
        assert_eq!(loaded.root_id.to_string(), ROOT_ID);
        assert_eq!(loaded.nodes[1].id.to_string(), CHILD_ID);
        assert_eq!(loaded.nodes[1].parent, Some(loaded.root_id));
        assert_eq!(loaded.nodes[1].order, 7);
        assert!(source.contains("SceneNodeData {"));
        assert!(source.contains("Children ["));
        assert!(source.contains("parent: Some("));
    }

    #[test]
    fn ecs_scene_round_trip_preserves_hierarchy() {
        let mut world = World::new();
        let root_id = SceneNodeId::parse(ROOT_ID).unwrap();
        let child_id = SceneNodeId::parse(CHILD_ID).unwrap();
        world.spawn((
            SceneRoot,
            EditableObject {
                name: "Root".into(),
            },
            SceneSpace::TwoD,
            Transform::default(),
            root_id,
            SceneParentId(None),
            SceneSiblingOrder(0),
        ));
        world.spawn((
            EditableObject {
                name: "Child".into(),
            },
            AddedEntityComponents(vec![BuiltinComponent::Sprite]),
            EntityCustomComponents(vec![arisna_engine::SceneCustomComponent {
                type_path: "sample_game::MovementState".into(),
                source_path: "res://src/movement.rs".into(),
                fields: vec![arisna_engine::SceneCustomField {
                    name: "speed".into(),
                    type_name: "f32".into(),
                    value: "12.5".into(),
                }],
            }]),
            SceneSpace::TwoD,
            Transform::from_xyz(3.0, 4.0, 0.0),
            child_id,
            SceneParentId(Some(root_id)),
            SceneSiblingOrder(2),
        ));
        let mut query_state: SystemState<Query<SceneSaveQuery>> = SystemState::new(&mut world);
        let query = query_state.get(&world).unwrap();

        let source = serialize_scene(root_id, &query).unwrap();
        let loaded = load_scene_v2(&source).unwrap();
        let child = loaded
            .nodes
            .iter()
            .find(|node| node.id == child_id)
            .unwrap();
        assert_eq!(child.parent, Some(root_id));
        assert_eq!(child.order, 2);
        assert_eq!(child.transform.translation, Vec3::new(3.0, 4.0, 0.0));
        assert_eq!(child.components, vec![BuiltinComponent::Sprite]);
        assert_eq!(child.custom_components[0].fields[0].value, "12.5");
        assert!(source.contains("sample_game::MovementState"));
    }

    #[test]
    fn animation_player_entity_round_trip_stays_in_scene_bsn() {
        let mut world = World::new();
        let root_id = SceneNodeId::parse(ROOT_ID).unwrap();
        let child_id = SceneNodeId::parse(CHILD_ID).unwrap();
        world.spawn((
            SceneRoot,
            EditableObject {
                name: "Root".into(),
            },
            EntityKind::Empty2D,
            SceneSpace::TwoD,
            Transform::default(),
            root_id,
            SceneParentId(None),
            SceneSiblingOrder(0),
        ));
        let player = SceneAnimationPlayer {
            autoplay: "Idle".into(),
            speed: 1.25,
            clips: vec![arisna_engine::SceneAnimationClip {
                name: "Idle".into(),
                length: 0.8,
                looped: true,
                tracks: vec![arisna_engine::SceneAnimationTrack {
                    target_node: root_id.to_string(),
                    property: "transform.position".into(),
                    kind: arisna_engine::SceneAnimationTrackKind::Transform,
                    keys: vec![arisna_engine::SceneAnimationKey {
                        time: 0.4,
                        value: arisna_engine::format_animation_transform(&Transform::from_xyz(
                            32.0, 48.0, 0.0,
                        )),
                    }],
                }],
            }],
        };
        world.spawn((
            EditableObject {
                name: "AnimationPlayer".into(),
            },
            EntityKind::AnimationPlayer,
            SceneSpace::TwoD,
            player.clone(),
            child_id,
            SceneParentId(Some(root_id)),
            SceneSiblingOrder(0),
        ));
        let mut query_state: SystemState<Query<SceneSaveQuery>> = SystemState::new(&mut world);
        let query = query_state.get(&world).unwrap();

        let source = serialize_scene(root_id, &query).unwrap();
        let loaded = load_scene_v2(&source).unwrap();
        let animation_player = loaded
            .nodes
            .iter()
            .find(|node| node.id == child_id)
            .unwrap();

        assert_eq!(animation_player.kind, EntityKind::AnimationPlayer);
        assert_eq!(animation_player.animation_player, Some(player));
        assert!(source.contains("kind: \"animation_player\""));
        assert!(source.contains("animation_player: Some("));
    }

    #[test]
    fn scene_save_uses_static_values_while_animation_preview_is_active() {
        let mut world = World::new();
        let root_id = SceneNodeId::parse(ROOT_ID).unwrap();
        let child_id = SceneNodeId::parse(CHILD_ID).unwrap();
        let sprite_id = SceneNodeId::new();
        let static_transform = Transform::from_xyz(10.0, 20.0, 0.0);
        world.spawn((
            SceneRoot,
            EditableObject {
                name: "Root".into(),
            },
            EntityKind::Empty2D,
            SceneSpace::TwoD,
            Transform::from_xyz(80.0, 90.0, 0.0),
            AnimationPreviewOriginalTransform(static_transform),
            root_id,
            SceneParentId(None),
            SceneSiblingOrder(0),
        ));
        let mut static_layout = SceneUiLayout::sized(120.0, 40.0);
        static_layout.offset = (12.0, 24.0);
        let mut preview_layout = static_layout;
        preview_layout.offset = (200.0, 300.0);
        world.spawn((
            EditableObject {
                name: "AnimatedButton".into(),
            },
            EntityKind::Button,
            AddedEntityComponents::default(),
            preview_layout,
            AnimationPreviewOriginalUiLayout(static_layout),
            EntityKind::Button.default_ui_content().unwrap(),
            SceneSpace::TwoD,
            child_id,
            SceneParentId(Some(root_id)),
            SceneSiblingOrder(0),
        ));
        world.spawn((
            EditableObject {
                name: "AnimatedSprite".into(),
            },
            EntityKind::Sprite2D,
            SceneSprite2D {
                hframes: 4,
                vframes: 2,
                frame: 6,
                ..default()
            },
            AnimationPreviewOriginalSpriteFrame(1),
            SceneSpace::TwoD,
            Transform::default(),
            sprite_id,
            SceneParentId(Some(root_id)),
            SceneSiblingOrder(1),
        ));
        let mut query_state: SystemState<Query<SceneSaveQuery>> = SystemState::new(&mut world);
        let query = query_state.get(&world).unwrap();

        let source = serialize_scene(root_id, &query).unwrap();
        let loaded = load_scene_v2(&source).unwrap();
        let root = loaded.nodes.iter().find(|node| node.id == root_id).unwrap();
        let button = loaded
            .nodes
            .iter()
            .find(|node| node.id == child_id)
            .unwrap();
        let sprite = loaded
            .nodes
            .iter()
            .find(|node| node.id == sprite_id)
            .unwrap();
        assert_eq!(root.transform, static_transform);
        assert_eq!(button.ui_layout, Some(static_layout));
        assert_eq!(sprite.sprite.as_ref().unwrap().frame, 1);
    }

    #[test]
    fn ui_entity_round_trip_preserves_kind_and_layout() {
        let mut world = World::new();
        let root_id = SceneNodeId::parse(ROOT_ID).unwrap();
        let child_id = SceneNodeId::parse(CHILD_ID).unwrap();
        world.spawn((
            SceneRoot,
            EditableObject {
                name: "Root".into(),
            },
            EntityKind::Empty2D,
            SceneSpace::TwoD,
            Transform::default(),
            root_id,
            SceneParentId(None),
            SceneSiblingOrder(0),
        ));
        let mut layout = SceneUiLayout::sized(180.0, 44.0);
        layout.anchor_min = (0.5, 0.5);
        layout.anchor_max = (0.5, 0.5);
        layout.offset = (-90.0, -22.0);
        let mut content = EntityKind::Button.default_ui_content().unwrap();
        content.text = "Play".into();
        world.spawn((
            EditableObject {
                name: "PlayButton".into(),
            },
            EntityKind::Button,
            AddedEntityComponents::default(),
            layout,
            content.clone(),
            SceneSpace::TwoD,
            child_id,
            SceneParentId(Some(root_id)),
            SceneSiblingOrder(0),
        ));
        let mut query_state: SystemState<Query<SceneSaveQuery>> = SystemState::new(&mut world);
        let query = query_state.get(&world).unwrap();

        let source = serialize_scene(root_id, &query).unwrap();
        let loaded = load_scene_v2(&source).unwrap();
        let button = loaded
            .nodes
            .iter()
            .find(|node| node.id == child_id)
            .unwrap();
        assert_eq!(button.kind, EntityKind::Button);
        assert_eq!(button.ui_layout, Some(layout));
        assert_eq!(button.ui_content, Some(content));
        assert!(source.contains("kind: \"button\""));
        assert!(source.contains("ui_layout: Some"));
    }

    #[test]
    fn collision_entity_round_trip_preserves_shape() {
        let mut world = World::new();
        let root_id = SceneNodeId::parse(ROOT_ID).unwrap();
        let child_id = SceneNodeId::parse(CHILD_ID).unwrap();
        world.spawn((
            SceneRoot,
            EditableObject {
                name: "Root".into(),
            },
            EntityKind::Empty2D,
            SceneSpace::TwoD,
            Transform::default(),
            root_id,
            SceneParentId(None),
            SceneSiblingOrder(0),
        ));
        let collision = SceneCollisionRect2D {
            size: (256.0, 48.0),
            offset: (8.0, 12.0),
            enabled: true,
        };
        world.spawn((
            EditableObject {
                name: "Wall".into(),
            },
            EntityKind::CollisionRect2D,
            AddedEntityComponents::default(),
            collision,
            SceneSpace::TwoD,
            Transform::from_xyz(100.0, -200.0, 0.0),
            child_id,
            SceneParentId(Some(root_id)),
            SceneSiblingOrder(0),
        ));
        let mut query_state: SystemState<Query<SceneSaveQuery>> = SystemState::new(&mut world);
        let query = query_state.get(&world).unwrap();

        let source = serialize_scene(root_id, &query).unwrap();
        let loaded = load_scene_v2(&source).unwrap();
        let wall = loaded
            .nodes
            .iter()
            .find(|node| node.id == child_id)
            .unwrap();

        assert_eq!(wall.kind, EntityKind::CollisionRect2D);
        assert_eq!(wall.collision_rect, Some(collision));
        assert!(source.contains("collision_rect: Some"));
    }

    #[test]
    fn v2_scene_save_rejects_duplicate_ids() {
        let scene = SceneFileV2 {
            format_version: 2,
            root: ROOT_ID.into(),
            entities: vec![
                scene_node(ROOT_ID, None, "Root"),
                scene_node(ROOT_ID, Some(ROOT_ID), "Duplicate"),
            ],
        };
        assert!(
            serialize_scene_file(&scene)
                .unwrap_err()
                .contains("duplicate")
        );
    }

    #[test]
    fn v2_scene_save_rejects_missing_parent() {
        let scene = SceneFileV2 {
            format_version: 2,
            root: ROOT_ID.into(),
            entities: vec![
                scene_node(ROOT_ID, None, "Root"),
                scene_node(CHILD_ID, Some("00000000000000000000000000000009"), "Orphan"),
            ],
        };
        assert!(
            serialize_scene_file(&scene)
                .unwrap_err()
                .contains("missing parent")
        );
    }

    #[test]
    fn v2_scene_save_rejects_hierarchy_cycles() {
        let third_id = "00000000000000000000000000000003";
        let scene = SceneFileV2 {
            format_version: 2,
            root: ROOT_ID.into(),
            entities: vec![
                scene_node(ROOT_ID, None, "Root"),
                scene_node(CHILD_ID, Some(third_id), "Child"),
                scene_node(third_id, Some(CHILD_ID), "Grandchild"),
            ],
        };
        assert!(serialize_scene_file(&scene).unwrap_err().contains("cycle"));
    }
}
