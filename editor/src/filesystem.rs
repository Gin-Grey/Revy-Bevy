use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use arisna_engine::{ProjectRoot, scene_image_resource_path, scene_model_resource_path};
use bevy::{
    picking::pointer::PointerButton,
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui::RelativeCursorPosition,
    ui_widgets::Activate,
    window::FileDragAndDrop,
};

use crate::output::{OutputLevel, OutputLog};
use crate::panels::FileSystemSection;
use crate::scene::OpenSceneRequest;
use crate::ui::{components::EditorVerticalScrollArea, theme};

const DEFAULT_HIDDEN_DIRECTORIES: &[&str] = &["target", "node_modules", "__pycache__", "venv"];

const ROOT_HIDDEN_DIRECTORIES: &[&str] =
    &["build", "dist", "release", "coverage", "out", "bin", "obj"];

const NEW_SCENE_EXTENSION: &str = "bsn";
const EMPTY_SCENE_BSN: &str = r#"Name("Empty2D")
SceneNodeData {
    id: "00000000000000000000000000000001",
    parent: None,
    order: 0,
    name: "Empty2D",
    kind: "empty2d",
    space: Some("2d"),
    components: [],
    systems: [],
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
Transform {
    translation: Vec3(0.0, 0.0, 0.0),
    rotation: Quat(0.0, 0.0, 0.0, 1.0),
    scale: Vec3(1.0, 1.0, 1.0),
}
"#;

pub(crate) fn project_path_from_args(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--project" {
            return args.next().map(PathBuf::from);
        }
        if !arg.to_string_lossy().starts_with('-') {
            return Some(PathBuf::from(arg));
        }
    }
    None
}

#[derive(Resource, Debug)]
pub struct FileSystemState {
    pub entries: Vec<FileEntry>,
    /// Relative folder paths that are expanded in the tree (`""` = res:// root).
    pub expanded: HashSet<String>,
    /// Selected relative path (`""` = res:// root).
    pub selected: Option<String>,
    pub filter: String,
    pub status: String,
    pub revision: u64,
}

impl Default for FileSystemState {
    fn default() -> Self {
        let mut expanded = HashSet::new();
        expanded.insert(String::new());
        Self {
            entries: Vec::new(),
            expanded,
            selected: Some(String::new()),
            filter: String::new(),
            status: "Ready".into(),
            revision: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub relative: String,
    pub is_dir: bool,
}

#[derive(Resource, Default, Debug)]
pub struct FsContextMenu {
    pub open: bool,
    pub screen_pos: Vec2,
    /// Folder (relative) where "New *" actions create files.
    pub target_dir: String,
    pub revision: u64,
}

#[derive(Resource, Debug, Default)]
struct FsContextMenuPointerState {
    armed: bool,
    layout_grace: bool,
}

impl FsContextMenuPointerState {
    fn arm(&mut self) {
        self.armed = true;
        self.layout_grace = true;
    }

    fn reset(&mut self) {
        self.armed = false;
        self.layout_grace = false;
    }
}

#[derive(Resource, Debug, Default)]
struct FsDoubleClickState {
    relative: Option<String>,
    time: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FsDoubleClickAction {
    OpenScene,
    InstantiateModel,
    OpenRustSource,
}

#[derive(Component, Clone, Copy, Default)]
pub struct FileSystemList;

#[derive(Component, Clone, Copy, Default)]
pub struct FileSystemStatusLabel;

#[derive(Component, Clone, Copy, Default)]
pub struct FileSystemPathLabel;

#[derive(Component, Clone, Copy)]
pub struct RefreshFilesButton;

#[derive(Component)]
pub struct FsFilterInput;

#[derive(Component)]
pub struct FsTreeRow {
    pub relative: String,
    pub is_dir: bool,
}

#[derive(Component)]
pub struct FsContextMenuHost;

#[derive(Component)]
pub struct FsContextMenuPanel;

#[derive(Component, Clone, Copy)]
pub struct FsMenuItem(pub FsAction);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsAction {
    NewFolder,
    NewScene,
    NewScript,
    NewResource,
    NewTextFile,
    OpenTerminal,
    OpenExplorer,
    Refresh,
}

#[derive(Message, Default)]
pub struct RefreshFileSystem;

#[derive(Message, Clone, Debug)]
pub struct InstantiateModelRequest {
    pub relative_path: PathBuf,
}

pub struct FileSystemPlugin;

impl Plugin for FileSystemPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FileSystemState>()
            .init_resource::<FsContextMenu>()
            .init_resource::<FsContextMenuPointerState>()
            .init_resource::<FsDoubleClickState>()
            .add_message::<RefreshFileSystem>()
            .add_message::<InstantiateModelRequest>()
            .add_observer(handle_refresh_button)
            .add_systems(Startup, (validate_project_root, spawn_context_menu_host))
            .add_systems(
                Update,
                (
                    handle_file_drop,
                    sync_filter_from_input,
                    refresh_filesystem_state,
                    rebuild_filesystem_ui,
                    highlight_fs_rows,
                    sync_filesystem_labels,
                    rebuild_context_menu,
                    close_context_menu_on_cursor_exit,
                    close_context_menu_on_escape,
                )
                    .chain(),
            );
    }
}

fn validate_project_root(mut project: ResMut<ProjectRoot>, mut state: ResMut<FileSystemState>) {
    if !project.root.exists() {
        state.status = format!("Project not found: {}", project.root.display());
        return;
    }
    if !project.root.is_dir() {
        state.status = format!(
            "Project path is not a directory: {}",
            project.root.display()
        );
        return;
    }
    match fs::canonicalize(&project.root) {
        Ok(root) => project.root = root,
        Err(err) => {
            state.status = format!("Failed to open project: {err}");
            return;
        }
    }
    state.status = format!("Project: {}", project.root.display());
    state.revision = state.revision.wrapping_add(1);
}

fn spawn_context_menu_host(mut commands: Commands) {
    commands.spawn((
        FsContextMenuHost,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        GlobalZIndex(1000),
        Pickable::IGNORE,
    ));
}

pub fn spawn_filesystem_dock(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            FileSystemSection,
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                min_height: Val::Px(120.0),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
        ))
        .with_children(|fs| {
            // Tab header matching the Bevy editor reference.
            fs.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(32.0),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    column_gap: Val::Px(8.0),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_toolbar()),
                BorderColor::all(theme::border()),
            ))
            .with_children(|h| {
                h.spawn((
                    Text::new("Project Files"),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    BorderColor::all(theme::accent()),
                ));
                h.spawn((
                    Text::new("+"),
                    TextFont {
                        font_size: FontSize::Px(15.0),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                ));
                h.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                h.spawn((
                    Button,
                    RefreshFilesButton,
                    Node {
                        width: Val::Px(24.0),
                        height: Val::Px(24.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    children![(
                        Text::new("..."),
                        TextFont {
                            font_size: FontSize::Px(15.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.9, 0.9, 0.92)),
                    )],
                ));
            });

            // Path bar: res://
            fs.spawn((
                FileSystemPathLabel,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(0.0),
                    display: Display::None,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    column_gap: Val::Px(6.0),
                    ..default()
                },
                BackgroundColor(theme::bg_panel()),
                children![(
                    Text::new("res://"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                )],
            ));

            // Filter files
            fs.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(36.0),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(5.0)),
                    column_gap: Val::Px(6.0),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_panel()),
                BorderColor::all(theme::border()),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new("Q"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                ));
                row.spawn((
                    FsFilterInput,
                    Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_panel_alt()),
                    BorderColor::all(theme::border()),
                    EditableText {
                        max_characters: Some(128),
                        ..default()
                    },
                    TextCursorStyle::default(),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.9, 0.9, 0.92)),
                ));
            });

            fs.spawn((
                FileSystemStatusLabel,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(0.0),
                    display: Display::None,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(theme::border()),
                children![(
                    Text::new("Ready"),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(theme::accent()),
                )],
            ));

            fs.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    position_type: PositionType::Relative,
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(theme::bg_panel_alt()),
            ))
            .with_children(|host| {
                host.spawn((
                    EditorVerticalScrollArea,
                    FileSystemList,
                    Button,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(12.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(4.0)),
                        row_gap: Val::Px(1.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BackgroundColor(theme::bg_panel_alt()),
                ))
                .observe(on_filesystem_list_click);
            });
        });
}

fn handle_refresh_button(
    activate: On<Activate>,
    buttons: Query<(), With<RefreshFilesButton>>,
    mut writer: MessageWriter<RefreshFileSystem>,
) {
    if buttons.contains(activate.entity) {
        writer.write(RefreshFileSystem);
    }
}

fn sync_filter_from_input(
    query: Query<&EditableText, (With<FsFilterInput>, Changed<EditableText>)>,
    mut state: ResMut<FileSystemState>,
) {
    for editable in &query {
        let value = editable.value().to_string();
        if state.filter != value {
            state.filter = value;
            state.revision = state.revision.wrapping_add(1);
        }
    }
}

fn handle_file_drop(
    mut events: MessageReader<FileDragAndDrop>,
    project: Res<ProjectRoot>,
    mut state: ResMut<FileSystemState>,
    mut writer: MessageWriter<RefreshFileSystem>,
) {
    let mut imported = 0usize;
    let mut errors = Vec::new();

    let dest_dir = import_destination(&project, &state);

    for event in events.read() {
        let FileDragAndDrop::DroppedFile { path_buf, .. } = event else {
            continue;
        };
        match import_path(&dest_dir, path_buf) {
            Ok(n) => imported += n,
            Err(err) => errors.push(err),
        }
    }

    if imported > 0 || !errors.is_empty() {
        if imported > 0 && errors.is_empty() {
            state.status = format!("Imported {imported} file(s)");
        } else if imported > 0 {
            state.status = format!("Imported {imported}; some failed: {}", errors.join("; "));
        } else {
            state.status = format!("Import failed: {}", errors.join("; "));
        }
        writer.write(RefreshFileSystem);
    }
}

fn import_destination(project: &ProjectRoot, state: &FileSystemState) -> PathBuf {
    let Some(sel) = state.selected.as_ref() else {
        return project.root.clone();
    };
    if sel.is_empty() {
        return project.root.clone();
    }
    let Some(path) = project.resolve_existing(sel) else {
        return project.root.clone();
    };
    if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(&project.root).to_path_buf()
    }
}

fn import_path(dest_dir: &Path, src: &Path) -> Result<usize, String> {
    if !src.exists() {
        return Err(format!("missing {}", src.display()));
    }

    fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;

    if src.is_file() {
        let name = src
            .file_name()
            .ok_or_else(|| "invalid file name".to_string())?;
        let dest = unique_dest(dest_dir, name);
        fs::copy(src, &dest).map_err(|e| e.to_string())?;
        return Ok(1);
    }

    let dir_name = src
        .file_name()
        .ok_or_else(|| "invalid folder name".to_string())?;
    let nested = dest_dir.join(dir_name);
    fs::create_dir_all(&nested).map_err(|e| e.to_string())?;

    let mut count = 0usize;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_file() {
            let name = path
                .file_name()
                .ok_or_else(|| "invalid file name".to_string())?;
            let dest = unique_dest(&nested, name);
            fs::copy(&path, &dest).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    Ok(count)
}

fn unique_dest(dir: &Path, file_name: &std::ffi::OsStr) -> PathBuf {
    let mut dest = dir.join(file_name);
    if !dest.exists() {
        return dest;
    }
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = Path::new(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    for i in 1..1000 {
        dest = if ext.is_empty() {
            dir.join(format!("{stem}_{i}"))
        } else {
            dir.join(format!("{stem}_{i}.{ext}"))
        };
        if !dest.exists() {
            break;
        }
    }
    dest
}

fn unique_named(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let name = if ext.is_empty() {
        base.to_string()
    } else {
        format!("{base}.{ext}")
    };
    unique_dest(dir, std::ffi::OsStr::new(&name))
}

fn refresh_filesystem_state(
    mut events: MessageReader<RefreshFileSystem>,
    project: Res<ProjectRoot>,
    mut state: ResMut<FileSystemState>,
    mut bootstrapped: Local<bool>,
) {
    let should = !*bootstrapped || events.read().count() > 0;
    if !should {
        return;
    }
    *bootstrapped = true;

    match scan_project(&project.root) {
        Ok(entries) => {
            let n = entries.len();
            state.entries = entries;
            if !(state.status.starts_with("Imported")
                || state.status.starts_with("Import failed")
                || state.status.starts_with("Created")
                || state.status.starts_with("Opened"))
            {
                state.status = format!("{n} item(s) - right-click for menu");
            }
            state.revision = state.revision.wrapping_add(1);
        }
        Err(err) => {
            state.entries.clear();
            state.status = format!("Scan failed: {err}");
            state.revision = state.revision.wrapping_add(1);
        }
    }
}

fn scan_project(root: &Path) -> Result<Vec<FileEntry>, String> {
    let mut out = Vec::new();
    if !root.exists() {
        return Err(format!("project not found: {}", root.display()));
    }
    if !root.is_dir() {
        return Err(format!(
            "project path is not a directory: {}",
            root.display()
        ));
    }
    scan_dir(root, root, &mut out)?;
    Ok(out)
}

fn scan_dir(root: &Path, dir: &Path, out: &mut Vec<FileEntry>) -> Result<(), String> {
    let is_project_root = dir == root;
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|result| {
            let entry = result.ok()?;
            let file_type = entry.file_type().ok()?;
            if file_type.is_symlink() {
                return None;
            }
            let is_dir = file_type.is_dir();
            if !is_dir && !file_type.is_file() {
                return None;
            }
            if is_ignored_entry(&entry.file_name(), is_dir, is_project_root) {
                return None;
            }
            Some((entry, is_dir))
        })
        .collect();
    entries.sort_by(
        |(a, a_is_dir), (b, b_is_dir)| match (*a_is_dir, *b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .cmp(&b.file_name().to_string_lossy().to_ascii_lowercase())
                .then_with(|| a.file_name().cmp(&b.file_name())),
        },
    );

    for (entry, is_dir) in entries {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(FileEntry {
            relative: rel,
            is_dir,
        });
        if is_dir {
            // A single unreadable nested directory must not hide the rest of the project.
            let _ = scan_dir(root, &path, out);
        }
    }
    Ok(())
}

fn is_ignored_entry(name: &std::ffi::OsStr, is_dir: bool, is_project_root: bool) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.')
        || (is_dir
            && (DEFAULT_HIDDEN_DIRECTORIES
                .iter()
                .any(|hidden| name.eq_ignore_ascii_case(hidden))
                || (is_project_root
                    && ROOT_HIDDEN_DIRECTORIES
                        .iter()
                        .any(|hidden| name.eq_ignore_ascii_case(hidden)))))
}

fn parent_of(relative: &str) -> String {
    match relative.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

fn depth_of(relative: &str) -> usize {
    if relative.is_empty() {
        0
    } else {
        relative.matches('/').count() + 1
    }
}

fn is_visible(entry: &FileEntry, state: &FileSystemState) -> bool {
    let filter = state.filter.trim().to_ascii_lowercase();
    if !filter.is_empty() {
        if entry_matches_filter(entry, &filter) {
            return true;
        }
        if entry.is_dir {
            let prefix = format!("{}/", entry.relative);
            return state.entries.iter().any(|candidate| {
                candidate.relative.starts_with(&prefix) && entry_matches_filter(candidate, &filter)
            });
        }
        return false;
    }

    let mut parent = parent_of(&entry.relative);
    loop {
        if !state.expanded.contains(&parent) {
            return false;
        }
        if parent.is_empty() {
            return true;
        }
        parent = parent_of(&parent);
    }
}

fn entry_matches_filter(entry: &FileEntry, filter: &str) -> bool {
    let name = entry
        .relative
        .rsplit('/')
        .next()
        .unwrap_or(entry.relative.as_str());
    name.to_ascii_lowercase().contains(filter)
        || entry.relative.to_ascii_lowercase().contains(filter)
}

fn rebuild_filesystem_ui(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    state: Res<FileSystemState>,
    list: Query<Entity, With<FileSystemList>>,
    mut last_rev: Local<u64>,
) {
    if state.revision == *last_rev {
        return;
    }
    let Ok(list_entity) = list.single() else {
        return;
    };
    *last_rev = state.revision;

    commands.entity(list_entity).despawn_related::<Children>();

    // Root: res://
    spawn_tree_row(
        &mut commands,
        list_entity,
        FsTreeRowView {
            relative: "",
            label: "res://",
            is_dir: true,
            depth: 0,
            expanded: state.expanded.contains(""),
            selected: state.selected.as_deref() == Some(""),
        },
        &asset_server,
    );

    let visible: Vec<_> = state
        .entries
        .iter()
        .filter(|e| is_visible(e, &state))
        .cloned()
        .collect();

    if visible.is_empty() && state.entries.is_empty() {
        commands.spawn((
            ChildOf(list_entity),
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            children![(
                Text::new("(empty — drop files or right-click)"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::text_muted()),
            )],
        ));
        return;
    }

    for entry in visible {
        let name = entry
            .relative
            .rsplit('/')
            .next()
            .unwrap_or(entry.relative.as_str());
        let depth = depth_of(&entry.relative);
        let expanded = entry.is_dir && state.expanded.contains(&entry.relative);
        let selected = state.selected.as_deref() == Some(entry.relative.as_str());
        spawn_tree_row(
            &mut commands,
            list_entity,
            FsTreeRowView {
                relative: &entry.relative,
                label: name,
                is_dir: entry.is_dir,
                depth,
                expanded,
                selected,
            },
            &asset_server,
        );
    }
}

struct FsTreeRowView<'a> {
    relative: &'a str,
    label: &'a str,
    is_dir: bool,
    depth: usize,
    expanded: bool,
    selected: bool,
}

fn spawn_tree_row(
    commands: &mut Commands,
    list_entity: Entity,
    view: FsTreeRowView<'_>,
    asset_server: &AssetServer,
) {
    let pad_left = 6.0 + view.depth as f32 * 14.0;

    let row = commands
        .spawn((
            FsTreeRow {
                relative: view.relative.to_string(),
                is_dir: view.is_dir,
            },
            ChildOf(list_entity),
            Button,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(23.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                padding: UiRect::new(Val::Px(pad_left), Val::Px(4.0), Val::Px(2.0), Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(row_bg(view.selected)),
        ))
        .id();

    commands.entity(row).with_children(|children| {
        children.spawn((
            Node {
                width: Val::Px(10.0),
                min_width: Val::Px(10.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Text::new(if view.is_dir {
                if view.expanded { "v" } else { ">" }
            } else {
                ""
            }),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(theme::text_muted()),
        ));

        if view.is_dir {
            let icon_path = if view.expanded {
                "editor/icons/folder-open.png"
            } else {
                "editor/icons/folder.png"
            };
            children.spawn((
                ImageNode::new(asset_server.load(icon_path)).with_color(theme::folder_icon()),
                Node {
                    width: Val::Px(15.0),
                    min_width: Val::Px(15.0),
                    height: Val::Px(15.0),
                    ..default()
                },
            ));
        } else {
            children.spawn((
                Node {
                    width: Val::Px(15.0),
                    min_width: Val::Px(15.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Text::new(file_icon(view.label)),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::text_muted()),
            ));
        }

        children.spawn((
            Text::new(view.label),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(theme::text_primary()),
        ));
    });

    commands.entity(row).observe(on_tree_row_click);
}

fn file_icon(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if is_supported_image_file(name) {
        "#"
    } else if is_supported_model_file(name) {
        "M"
    } else if lower.ends_with(".wav") || lower.ends_with(".ogg") || lower.ends_with(".mp3") {
        "~"
    } else if lower.ends_with(".rs") || lower.ends_with(".gd") {
        "{}"
    } else if lower.ends_with(".bsn")
        || lower.ends_with(".scene")
        || lower.ends_with(".scn")
        || lower.ends_with(".scn.ron")
    {
        "*"
    } else {
        "-"
    }
}

pub(crate) fn is_supported_image_file(relative: &str) -> bool {
    Path::new(relative)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "bmp" | "tga" | "ktx2"
            )
        })
}

pub(crate) fn image_resource_path_from_filesystem(relative: &str) -> Result<String, String> {
    let relative = relative.trim().replace('\\', "/");
    if !is_supported_image_file(&relative) {
        return Err("Only supported image files can be assigned to a UI Image entity.".into());
    }
    let Some(asset_relative) = relative.strip_prefix("assets/") else {
        return Err("UI textures must be inside the project's assets folder.".into());
    };
    scene_image_resource_path(asset_relative)
        .ok_or_else(|| "The selected image has an invalid project path.".into())
}

pub(crate) fn is_supported_model_file(relative: &str) -> bool {
    Path::new(relative)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "fbx" | "gltf" | "glb"
            )
        })
}

pub(crate) fn model_resource_path_from_filesystem(relative: &str) -> Result<String, String> {
    let relative = relative.trim().replace('\\', "/");
    if !is_supported_model_file(&relative) {
        return Err("Only FBX, GLB, or GLTF model files can be assigned.".into());
    }
    let Some(asset_relative) = relative.strip_prefix("assets/") else {
        return Err("3D models must be inside the project's assets folder.".into());
    };
    scene_model_resource_path(asset_relative)
        .ok_or_else(|| "The selected model has an invalid project path.".into())
}

pub(crate) fn rust_script_resource_path_from_filesystem(relative: &str) -> Result<String, String> {
    let relative = relative.trim().replace('\\', "/");
    if !relative
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("rs"))
    {
        return Err("Only Rust source files can be assigned to a system.".into());
    }
    let Some(source_relative) = relative.strip_prefix("src/") else {
        return Err("System scripts must be inside the project's src folder.".into());
    };
    if source_relative
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err("The selected Rust source has an invalid project path.".into());
    }
    Ok(format!("res://src/{source_relative}"))
}

fn row_bg(selected: bool) -> Color {
    if selected {
        Color::srgb(0.22, 0.35, 0.52)
    } else {
        Color::NONE
    }
}

fn on_tree_row_click(
    click: On<Pointer<Click>>,
    mut state: ResMut<FileSystemState>,
    mut menu: ResMut<FsContextMenu>,
    mut menu_pointer: ResMut<FsContextMenuPointerState>,
    project: Res<ProjectRoot>,
    mut output: Option<ResMut<OutputLog>>,
    rows: Query<&FsTreeRow>,
    mut open_scenes: MessageWriter<OpenSceneRequest>,
    mut instantiate_models: MessageWriter<InstantiateModelRequest>,
    time: Res<Time>,
    mut double_click: ResMut<FsDoubleClickState>,
) {
    let Ok(row) = rows.get(click.entity) else {
        return;
    };

    match click.button {
        PointerButton::Primary => {
            menu.open = false;
            menu.revision = menu.revision.wrapping_add(1);
            menu_pointer.reset();

            state.selected = Some(row.relative.clone());
            let now = time.elapsed_secs();
            let repeated = double_click.relative.as_deref() == Some(row.relative.as_str())
                && now - double_click.time <= 0.45;
            double_click.relative = Some(row.relative.clone());
            double_click.time = now;
            if click.count >= 2 || repeated {
                match double_click_action(&row.relative, row.is_dir) {
                    Some(FsDoubleClickAction::OpenScene) => {
                        open_scenes.write(OpenSceneRequest {
                            relative_path: PathBuf::from(&row.relative),
                        });
                        state.revision = state.revision.wrapping_add(1);
                        return;
                    }
                    Some(FsDoubleClickAction::InstantiateModel) => {
                        instantiate_models.write(InstantiateModelRequest {
                            relative_path: PathBuf::from(&row.relative),
                        });
                        state.revision = state.revision.wrapping_add(1);
                        return;
                    }
                    Some(FsDoubleClickAction::OpenRustSource) => {
                        let (level, message) =
                            match open_rust_source_in_rustrover(&project.root, &row.relative) {
                                Ok(()) => (
                                    OutputLevel::Info,
                                    format!("Opened {} in RustRover", row.relative),
                                ),
                                Err(error) => (OutputLevel::Error, error),
                            };
                        state.status = message.clone();
                        if let Some(output) = output.as_deref_mut() {
                            output.push(level, message);
                        }
                        state.revision = state.revision.wrapping_add(1);
                        return;
                    }
                    None => {}
                }
            }
            if row.is_dir {
                if state.expanded.contains(&row.relative) {
                    state.expanded.remove(&row.relative);
                } else {
                    state.expanded.insert(row.relative.clone());
                }
            }
            state.revision = state.revision.wrapping_add(1);
        }
        PointerButton::Secondary => {
            state.selected = Some(row.relative.clone());
            let target_dir = if row.is_dir {
                row.relative.clone()
            } else {
                parent_of(&row.relative)
            };
            open_context_menu(
                &mut menu,
                &mut menu_pointer,
                click.pointer_location.position,
                target_dir,
            );
            state.revision = state.revision.wrapping_add(1);
        }
        _ => {}
    }
}

fn is_scene_file(relative: &str) -> bool {
    let relative = relative.to_ascii_lowercase();
    relative.ends_with(".bsn") || relative.ends_with(".scn.ron")
}

fn is_rust_source_file(relative: &str) -> bool {
    Path::new(relative)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}

fn double_click_action(relative: &str, is_dir: bool) -> Option<FsDoubleClickAction> {
    if is_dir {
        return None;
    }
    if is_scene_file(relative) {
        Some(FsDoubleClickAction::OpenScene)
    } else if is_supported_model_file(relative) {
        Some(FsDoubleClickAction::InstantiateModel)
    } else if is_rust_source_file(relative) {
        Some(FsDoubleClickAction::OpenRustSource)
    } else {
        None
    }
}

fn open_rust_source_in_rustrover(project_root: &Path, relative: &str) -> Result<(), String> {
    let source_path = project_root.join(relative);
    if !source_path.is_file() {
        return Err(format!(
            "Rust source file was not found: {}",
            source_path.display()
        ));
    }

    let Some(executable) = find_rustrover() else {
        return Err(
            "RustRover executable was not found. Set RUSTROVER_PATH or add RustRover to PATH."
                .into(),
        );
    };

    Command::new(&executable)
        .arg(external_process_path(&source_path))
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open RustRover: {error}"))
}

#[cfg(windows)]
fn external_process_path(path: &Path) -> PathBuf {
    let path = path.to_string_lossy();
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.into_owned().into()
    }
}

#[cfg(not(windows))]
fn external_process_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn find_rustrover() -> Option<PathBuf> {
    if let Some(path) = env::var_os("RUSTROVER_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }

    for executable in rustrover_executable_names() {
        if let Some(path) = executable_on_path(executable) {
            return Some(path);
        }
    }

    for root in jetbrains_search_roots() {
        if let Some(path) = find_rustrover_under(&root) {
            return Some(path);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn rustrover_executable_names() -> &'static [&'static str] {
    &["rustrover64.exe", "rustrover.exe"]
}

#[cfg(not(target_os = "windows"))]
fn rustrover_executable_names() -> &'static [&'static str] {
    &["rustrover", "rustrover.sh"]
}

fn executable_on_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn jetbrains_search_roots() -> impl Iterator<Item = PathBuf> {
    let mut roots = Vec::new();
    for variable in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
        let Some(base) = env::var_os(variable).map(PathBuf::from) else {
            continue;
        };
        let jetbrains = base.join("JetBrains");
        roots.push(jetbrains.clone());
        roots.push(jetbrains.join("Installations"));
        roots.push(jetbrains.join("Toolbox").join("apps").join("RustRover"));
    }
    roots.into_iter()
}

fn find_rustrover_under(root: &Path) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }

    for executable in rustrover_executable_names() {
        let direct = root.join("bin").join(executable);
        if direct.is_file() {
            return Some(direct);
        }
    }

    let entries = fs::read_dir(root).ok()?;
    let mut directories = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .collect::<Vec<_>>();
    directories.sort_by_key(|entry| entry.file_name());
    directories.reverse();

    for entry in directories {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.contains("rustrover") || root.ends_with("RustRover") {
            if let Some(path) = find_rustrover_under(&entry.path()) {
                return Some(path);
            }
        }
    }
    None
}

fn on_filesystem_list_click(
    click: On<Pointer<Click>>,
    mut menu: ResMut<FsContextMenu>,
    mut menu_pointer: ResMut<FsContextMenuPointerState>,
    state: Res<FileSystemState>,
    rows: Query<&FsTreeRow>,
) {
    // Only handle empty-area clicks (not bubbled from a row).
    if rows.get(click.entity).is_ok() {
        return;
    }
    if click.button != PointerButton::Secondary {
        return;
    }
    let target = state
        .selected
        .clone()
        .filter(|s| s.is_empty() || state.entries.iter().any(|e| e.relative == *s && e.is_dir))
        .unwrap_or_default();
    open_context_menu(
        &mut menu,
        &mut menu_pointer,
        click.pointer_location.position,
        target,
    );
}

fn open_context_menu(
    menu: &mut FsContextMenu,
    pointer: &mut FsContextMenuPointerState,
    pos: Vec2,
    target_dir: String,
) {
    menu.open = true;
    menu.screen_pos = pos;
    menu.target_dir = target_dir;
    menu.revision = menu.revision.wrapping_add(1);
    pointer.arm();
}

fn highlight_fs_rows(
    state: Res<FileSystemState>,
    mut rows: Query<(&FsTreeRow, &mut BackgroundColor)>,
) {
    if !state.is_changed() {
        return;
    }
    for (row, mut bg) in &mut rows {
        let selected = state.selected.as_deref() == Some(row.relative.as_str());
        *bg = BackgroundColor(row_bg(selected));
    }
}

fn sync_filesystem_labels(
    state: Res<FileSystemState>,
    path_roots: Query<&Children, With<FileSystemPathLabel>>,
    status_roots: Query<&Children, With<FileSystemStatusLabel>>,
    mut texts: Query<&mut Text>,
) {
    if !state.is_changed() {
        return;
    }

    let path_text = match state.selected.as_deref() {
        Some("") | None => "res://".to_string(),
        Some(path) => format!("res://{path}"),
    };
    for children in &path_roots {
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child) {
                text.0 = path_text.clone();
            }
        }
    }
    for children in &status_roots {
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child) {
                text.0 = state.status.clone();
            }
        }
    }
}

fn rebuild_context_menu(
    mut commands: Commands,
    menu: Res<FsContextMenu>,
    host: Query<Entity, With<FsContextMenuHost>>,
    mut last_rev: Local<u64>,
) {
    if menu.revision == *last_rev {
        return;
    }
    *last_rev = menu.revision;

    let Ok(host_entity) = host.single() else {
        return;
    };
    commands.entity(host_entity).despawn_related::<Children>();

    if !menu.open {
        return;
    }

    let items: &[(FsAction, &str)] = &[
        (FsAction::NewFolder, "New Folder..."),
        (FsAction::NewScene, "New Scene..."),
        (FsAction::NewScript, "New Script..."),
        (FsAction::NewResource, "New Resource..."),
        (FsAction::NewTextFile, "New Text File..."),
        (FsAction::OpenTerminal, "Open in Terminal"),
        (FsAction::OpenExplorer, "Open in File Manager"),
        (FsAction::Refresh, "Refresh"),
    ];

    let panel = commands
        .spawn((
            FsContextMenuPanel,
            ChildOf(host_entity),
            Node {
                position_type: PositionType::Absolute,
                // Put the opening click just inside the panel instead of on
                // its exact top-left boundary, where hit-testing is ambiguous.
                left: Val::Px((menu.screen_pos.x - 2.0).max(0.0)),
                top: Val::Px((menu.screen_pos.y - 2.0).max(0.0)),
                min_width: Val::Px(200.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(4.0)),
                row_gap: Val::Px(1.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.18, 0.19, 0.21)),
            BorderColor::all(Color::srgb(0.08, 0.08, 0.09)),
            GlobalZIndex(1001),
            RelativeCursorPosition::default(),
        ))
        .id();

    for (action, label) in items {
        let item = commands
            .spawn((
                FsMenuItem(*action),
                ChildOf(panel),
                Button,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                children![(
                    Text::new(*label),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.9, 0.9, 0.92)),
                )],
            ))
            .id();
        commands.entity(item).observe(on_menu_item_click);
        commands.entity(item).observe(on_menu_item_over);
        commands.entity(item).observe(on_menu_item_out);
    }
}

/// Close the file-system menu once the pointer leaves the panel as a whole.
///
/// Opening arms the menu immediately. A single update is reserved for spawning
/// and laying out the dynamic panel; after that, the first outside sample closes
/// it even when the pointer moved directly left/up from the opening click.
fn close_context_menu_on_cursor_exit(
    mut menu: ResMut<FsContextMenu>,
    mut pointer: ResMut<FsContextMenuPointerState>,
    panels: Query<&RelativeCursorPosition, With<FsContextMenuPanel>>,
) {
    if !menu.open {
        pointer.reset();
        return;
    }

    if pointer.layout_grace {
        pointer.layout_grace = false;
        return;
    }

    let cursor_over = panels.iter().any(RelativeCursorPosition::cursor_over);
    if !cursor_over && pointer.armed {
        pointer.reset();
        menu.open = false;
        menu.revision = menu.revision.wrapping_add(1);
    }
}

fn on_menu_item_over(
    over: On<Pointer<Over>>,
    mut backgrounds: Query<&mut BackgroundColor, With<FsMenuItem>>,
) {
    if let Ok(mut bg) = backgrounds.get_mut(over.entity) {
        *bg = BackgroundColor(theme::accent());
    }
}

fn on_menu_item_out(
    out: On<Pointer<Out>>,
    mut backgrounds: Query<&mut BackgroundColor, With<FsMenuItem>>,
) {
    if let Ok(mut bg) = backgrounds.get_mut(out.entity) {
        *bg = BackgroundColor(Color::NONE);
    }
}

fn on_menu_item_click(
    click: On<Pointer<Click>>,
    items: Query<&FsMenuItem>,
    project: Res<ProjectRoot>,
    mut state: ResMut<FileSystemState>,
    mut menu: ResMut<FsContextMenu>,
    mut writer: MessageWriter<RefreshFileSystem>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(item) = items.get(click.entity) else {
        return;
    };

    let Some(target_dir) = project
        .resolve_existing(&menu.target_dir)
        .filter(|path| path.is_dir())
    else {
        state.status = "Invalid project folder".into();
        menu.open = false;
        menu.revision = menu.revision.wrapping_add(1);
        return;
    };

    match item.0 {
        FsAction::NewFolder => {
            let path = unique_named(&target_dir, "new_folder", "");
            match fs::create_dir_all(&path) {
                Ok(()) => {
                    state.status = format!("Created folder {}", path.display());
                    if let Ok(rel) = path.strip_prefix(&project.root) {
                        let rel = rel.to_string_lossy().replace('\\', "/");
                        state.expanded.insert(menu.target_dir.clone());
                        state.selected = Some(rel);
                    }
                }
                Err(err) => state.status = format!("Create folder failed: {err}"),
            }
            writer.write(RefreshFileSystem);
        }
        FsAction::NewScene => {
            create_text_asset(
                &mut state,
                &project.root,
                &target_dir,
                "new_scene",
                NEW_SCENE_EXTENSION,
                EMPTY_SCENE_BSN,
            );
            writer.write(RefreshFileSystem);
        }
        FsAction::NewScript => {
            create_text_asset(
                &mut state,
                &project.root,
                &target_dir,
                "new_script",
                "rs",
                "// New script\n",
            );
            writer.write(RefreshFileSystem);
        }
        FsAction::NewResource => {
            create_text_asset(
                &mut state,
                &project.root,
                &target_dir,
                "new_resource",
                "res",
                "# resource placeholder\n",
            );
            writer.write(RefreshFileSystem);
        }
        FsAction::NewTextFile => {
            create_text_asset(
                &mut state,
                &project.root,
                &target_dir,
                "new_file",
                "txt",
                "",
            );
            writer.write(RefreshFileSystem);
        }
        FsAction::OpenTerminal => match open_terminal(&target_dir) {
            Ok(()) => state.status = format!("Opened terminal in {}", target_dir.display()),
            Err(err) => state.status = format!("Terminal failed: {err}"),
        },
        FsAction::OpenExplorer => match open_file_manager(&target_dir) {
            Ok(()) => state.status = format!("Opened file manager: {}", target_dir.display()),
            Err(err) => state.status = format!("File manager failed: {err}"),
        },
        FsAction::Refresh => {
            writer.write(RefreshFileSystem);
        }
    }

    menu.open = false;
    menu.revision = menu.revision.wrapping_add(1);
}

fn create_text_asset(
    state: &mut FileSystemState,
    assets_root: &Path,
    target_dir: &Path,
    base: &str,
    ext: &str,
    contents: &str,
) {
    let path = unique_named(target_dir, base, ext);
    match fs::write(&path, contents) {
        Ok(()) => {
            state.status = format!("Created {}", path.display());
            if let Ok(rel) = path.strip_prefix(assets_root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                state.selected = Some(rel);
            }
        }
        Err(err) => state.status = format!("Create failed: {err}"),
    }
}

fn open_file_manager(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn open_terminal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args([
                "/C",
                "start",
                "cmd",
                "/K",
                &format!("cd /d {}", path.display()),
            ])
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-a", "Terminal", path.as_os_str()])
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("x-terminal-emulator")
            .arg("--working-directory")
            .arg(path)
            .spawn()
            .or_else(|_| {
                Command::new("gnome-terminal")
                    .arg(format!("--working-directory={}", path.display()))
                    .spawn()
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn close_context_menu_on_escape(keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<FsContextMenu>) {
    if keys.just_pressed(KeyCode::Escape) && menu.open {
        menu.open = false;
        menu.revision = menu.revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::default_project_root;

    #[test]
    fn project_argument_accepts_positional_path() {
        let path = project_path_from_args([OsString::from("E:/Games/Demo")]);
        assert_eq!(path, Some(PathBuf::from("E:/Games/Demo")));
    }

    #[test]
    fn project_argument_accepts_named_path() {
        let path =
            project_path_from_args([OsString::from("--project"), OsString::from("E:/Games/Demo")]);
        assert_eq!(path, Some(PathBuf::from("E:/Games/Demo")));
    }

    #[test]
    fn project_scan_ignores_internal_and_cache_entries() {
        assert!(is_ignored_entry(std::ffi::OsStr::new(".git"), true, true));
        assert!(is_ignored_entry(std::ffi::OsStr::new(".env"), false, true));
        for name in DEFAULT_HIDDEN_DIRECTORIES {
            assert!(is_ignored_entry(std::ffi::OsStr::new(name), true, false));
            assert!(is_ignored_entry(
                std::ffi::OsStr::new(&name.to_ascii_uppercase()),
                true,
                false
            ));
        }
        for name in ROOT_HIDDEN_DIRECTORIES {
            assert!(is_ignored_entry(std::ffi::OsStr::new(name), true, true));
            assert!(!is_ignored_entry(std::ffi::OsStr::new(name), true, false));
        }
        assert!(!is_ignored_entry(
            std::ffi::OsStr::new("assets"),
            true,
            true
        ));
        assert!(!is_ignored_entry(std::ffi::OsStr::new("src"), true, true));
        assert!(!is_ignored_entry(
            std::ffi::OsStr::new("build.rs"),
            false,
            true
        ));
        assert!(!is_ignored_entry(
            std::ffi::OsStr::new("target.rs"),
            false,
            true
        ));
    }

    #[test]
    fn new_scene_uses_runtime_bsn() {
        assert_eq!(NEW_SCENE_EXTENSION, "bsn");
        assert!(EMPTY_SCENE_BSN.contains("SceneNodeData {"));
        assert!(EMPTY_SCENE_BSN.contains("Transform {"));
    }

    #[test]
    fn context_menu_closes_when_pointer_moves_directly_outside() {
        let mut app = App::new();
        app.init_resource::<FsContextMenu>()
            .init_resource::<FsContextMenuPointerState>()
            .add_systems(Update, close_context_menu_on_cursor_exit);
        let panel = app
            .world_mut()
            .spawn((
                FsContextMenuPanel,
                RelativeCursorPosition {
                    cursor_over: false,
                    normalized: None,
                },
            ))
            .id();
        app.world_mut().resource_mut::<FsContextMenu>().open = true;
        app.world_mut()
            .resource_mut::<FsContextMenuPointerState>()
            .arm();

        // Allow the dynamic panel one update to spawn and complete layout.
        app.update();
        assert!(app.world().resource::<FsContextMenu>().open);

        // The pointer never entered the panel. This models moving directly
        // left from the right-click point and must still close the menu.
        app.update();

        assert!(!app.world().resource::<FsContextMenu>().open);
        assert_eq!(app.world().resource::<FsContextMenu>().revision, 1);
        assert!(
            app.world()
                .entity(panel)
                .contains::<RelativeCursorPosition>()
        );
    }

    #[test]
    fn scene_files_are_recognized_for_double_click_open() {
        assert!(is_scene_file("assets/scenes/main.bsn"));
        assert!(is_scene_file("assets/scenes/MAIN.BSN"));
        assert!(is_scene_file("assets/scenes/main.scn.ron"));
        assert!(!is_scene_file("assets/scenes/main.ron"));
    }

    #[test]
    fn filesystem_double_click_actions_keep_file_behaviors_separate() {
        assert_eq!(
            double_click_action("assets/scenes/main.bsn", false),
            Some(FsDoubleClickAction::OpenScene)
        );
        assert_eq!(
            double_click_action("assets/models/hero.glb", false),
            Some(FsDoubleClickAction::InstantiateModel)
        );
        assert_eq!(
            double_click_action("src/player.rs", false),
            Some(FsDoubleClickAction::OpenRustSource)
        );
        assert_eq!(double_click_action("src", true), None);
        assert_eq!(double_click_action("README.md", false), None);
    }

    #[test]
    fn rust_source_extension_is_case_insensitive() {
        assert!(is_rust_source_file("src/player.rs"));
        assert!(is_rust_source_file("src/PLAYER.RS"));
        assert!(!is_rust_source_file("src/player.ron"));
    }

    #[test]
    fn filesystem_images_map_to_canonical_scene_resources() {
        assert_eq!(
            image_resource_path_from_filesystem("assets/ui/logo.png").unwrap(),
            "res://ui/logo.png"
        );
        assert!(image_resource_path_from_filesystem("docs/logo.png").is_err());
        assert!(image_resource_path_from_filesystem("assets/ui/logo.svg").is_err());
    }

    #[test]
    fn filesystem_rust_sources_map_to_system_resources() {
        assert_eq!(
            rust_script_resource_path_from_filesystem("src/systems/camera_follow.rs").unwrap(),
            "res://src/systems/camera_follow.rs"
        );
        assert!(rust_script_resource_path_from_filesystem("assets/camera_follow.rs").is_err());
        assert!(rust_script_resource_path_from_filesystem("src/camera_follow.cs").is_err());
    }

    #[test]
    fn filesystem_models_map_to_canonical_model_resources() {
        assert!(is_supported_model_file("assets/models/HERO.FBX"));
        assert!(is_supported_model_file("assets/models/hero.glb"));
        assert!(is_supported_model_file("assets/models/hero.GLTF"));
        assert_eq!(
            model_resource_path_from_filesystem("assets/models/character.fbx").unwrap(),
            "res://models/character.fbx"
        );
        assert_eq!(
            model_resource_path_from_filesystem("assets/3d/role/hero.glb").unwrap(),
            "res://3d/role/hero.glb"
        );
        assert_eq!(
            model_resource_path_from_filesystem("assets/3d/role/hero.gltf").unwrap(),
            "res://3d/role/hero.gltf"
        );
        assert!(model_resource_path_from_filesystem("docs/character.fbx").is_err());
        assert!(model_resource_path_from_filesystem("assets/models/character.blend").is_err());
    }

    #[test]
    fn filtered_project_tree_keeps_matching_file_ancestors() {
        let mut state = FileSystemState::default();
        state.entries = vec![
            FileEntry {
                relative: "assets".into(),
                is_dir: true,
            },
            FileEntry {
                relative: "assets/scenes".into(),
                is_dir: true,
            },
            FileEntry {
                relative: "assets/scenes/main.scene".into(),
                is_dir: false,
            },
        ];
        state.filter = "main".into();

        assert!(state.entries.iter().all(|entry| is_visible(entry, &state)));
    }

    #[test]
    fn project_scan_reads_player_files_and_hides_generated_directories() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "arisna_filesystem_test_{}_{}",
            std::process::id(),
            unique
        ));

        fs::create_dir_all(root.join("assets/scenes")).unwrap();
        fs::create_dir_all(root.join("src/bin")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::create_dir_all(root.join("node_modules/package")).unwrap();
        fs::create_dir_all(root.join("build/generated")).unwrap();
        fs::write(root.join("project.toml"), "[project]\nname = \"Test\"\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("src/bin/tool.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("assets/scenes/main.scene"), "# scene\n").unwrap();

        let entries = scan_project(&root).expect("player project should be readable");
        let ordered_paths: Vec<_> = entries
            .iter()
            .map(|entry| entry.relative.as_str())
            .collect();
        let paths: HashSet<_> = entries
            .iter()
            .map(|entry| entry.relative.as_str())
            .collect();

        assert!(paths.contains("project.toml"));
        assert!(paths.contains("src/main.rs"));
        assert!(paths.contains("assets/scenes/main.scene"));
        assert!(!paths.iter().any(|path| path.starts_with("target")));
        assert!(!paths.iter().any(|path| path.starts_with(".git")));
        assert!(!paths.iter().any(|path| path.starts_with("node_modules")));
        assert!(!paths.iter().any(|path| path.starts_with("build")));
        assert_eq!(
            ordered_paths,
            [
                "assets",
                "assets/scenes",
                "assets/scenes/main.scene",
                "src",
                "src/bin",
                "src/bin/tool.rs",
                "src/main.rs",
                "project.toml",
            ]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn default_project_is_separate_from_editor_assets() {
        let project_root = default_project_root();
        let editor_assets = crate::paths::workspace_root().join("assets");
        assert_ne!(project_root, editor_assets);

        let entries = scan_project(&project_root).expect("default project should be readable");
        assert!(entries.iter().any(|entry| entry.relative == "project.toml"));
        assert!(entries.iter().any(|entry| entry.relative == "assets"));
        assert!(
            !entries
                .iter()
                .any(|entry| entry.relative.starts_with("editor"))
        );
    }
}
