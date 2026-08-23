//! Editable project configuration backed by the active project's `project.toml`.

use std::{
    fs,
    path::{Component, Path},
};

use arisna_engine::{ProjectRoot, ProjectSettings, ProjectWindowMode, ProjectWindowSettings};
use bevy::{
    camera::{ClearColorConfig, RenderTarget},
    input::{
        InputSystems,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{AutoFocus, FocusCause, InputFocus, InputFocusSystems},
    picking::{
        events::{Pointer, Press},
        pointer::PointerButton,
    },
    prelude::*,
    text::{EditableText, EditableTextFilter, TextCursorStyle, TextEdit},
    ui::UiTargetCamera,
    ui_widgets::{Activate, Button as WidgetButton, ManualKeyboardInput},
    window::{
        EnabledButtons, MonitorSelection, PrimaryWindow, WindowCloseRequested, WindowLevel,
        WindowPosition, WindowRef, WindowResizeConstraints, WindowResolution,
    },
};
use toml_edit::DocumentMut;

use crate::{scene::SceneDocument, ui::theme};

#[derive(Component, Clone, Copy, Default)]
pub struct ProjectSettingsMenuButton;

#[derive(Component, Clone, Copy, Default)]
pub struct ProjectMenuButton;

#[derive(Component, Clone, Copy, Default)]
pub struct ProjectMenuDropdown;

#[derive(Component)]
struct ProjectSettingsWindow;

#[derive(Component)]
struct ProjectSettingsCamera;

#[derive(Component)]
struct ProjectSettingsUiRoot;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectSettingsField {
    ProjectName,
    MainScene,
    WindowTitle,
    WindowWidth,
    WindowHeight,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectSettingsAction {
    UseCurrentScene,
    WindowMode(ProjectWindowMode),
    Vsync(bool),
    Apply,
    Cancel,
}

#[derive(Component)]
struct ProjectSettingsStatus;

#[derive(Debug, Clone)]
struct ProjectSettingsDraft {
    project_name: String,
    main_scene: String,
    window_title: String,
    window_width: String,
    window_height: String,
    window_mode: ProjectWindowMode,
    vsync: bool,
}

impl From<ProjectSettings> for ProjectSettingsDraft {
    fn from(settings: ProjectSettings) -> Self {
        Self {
            project_name: settings.name,
            main_scene: settings.main_scene,
            window_title: settings.window.title,
            window_width: settings.window.width.to_string(),
            window_height: settings.window.height.to_string(),
            window_mode: settings.window.mode,
            vsync: settings.window.vsync,
        }
    }
}

impl Default for ProjectSettingsDraft {
    fn default() -> Self {
        ProjectSettings::default().into()
    }
}

#[derive(Resource, Debug)]
struct ProjectSettingsDialog {
    open: bool,
    draft: ProjectSettingsDraft,
    status: String,
    status_is_error: bool,
    ignore_input_changes: bool,
    /// Project Settings 使用独立原生窗口，不能依赖全局焦点推断当前编辑字段。
    active_input: Option<Entity>,
    /// 新点击字段后的第一次修改会替换原值，分辨率输入无需手动删除旧数字。
    replace_active_input: bool,
}

/// 编辑器内共享的实际游戏画面尺寸。
///
/// Project Settings、2D 画面边框和后续摄像机预览都读取同一份资源，避免
/// 各功能各自解析 `project.toml` 后产生不一致。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectDisplaySettings {
    pub width: u32,
    pub height: u32,
}

impl Default for ProjectDisplaySettings {
    fn default() -> Self {
        let window = ProjectWindowSettings::default();
        Self {
            width: window.width,
            height: window.height,
        }
    }
}

impl From<&ProjectSettings> for ProjectDisplaySettings {
    fn from(settings: &ProjectSettings) -> Self {
        Self {
            width: settings.window.width,
            height: settings.window.height,
        }
    }
}

impl Default for ProjectSettingsDialog {
    fn default() -> Self {
        Self {
            open: false,
            draft: default(),
            status: String::new(),
            status_is_error: false,
            ignore_input_changes: false,
            active_input: None,
            replace_active_input: false,
        }
    }
}

#[derive(Resource, Debug, Default)]
struct ProjectMenuState {
    open: bool,
}

pub struct ProjectSettingsPlugin;

impl Plugin for ProjectSettingsPlugin {
    fn build(&self, app: &mut App) {
        let display_settings = app
            .world()
            .get_resource::<ProjectRoot>()
            .and_then(|project| ProjectSettings::load(&project.root).ok())
            .as_ref()
            .map(ProjectDisplaySettings::from)
            .unwrap_or_default();

        app.insert_resource(display_settings)
            .init_resource::<ProjectSettingsDialog>()
            .init_resource::<ProjectMenuState>()
            .add_observer(handle_project_settings_action)
            .add_observer(activate_project_settings_input)
            .add_systems(
                PreUpdate,
                capture_project_settings_keyboard
                    .after(InputSystems)
                    .after(InputFocusSystems::Dispatch),
            )
            .add_systems(
                Update,
                (
                    handle_native_window_close,
                    handle_settings_keyboard,
                    dismiss_project_menu_on_outside_click,
                    sync_project_menu_visibility,
                    sync_project_menu_chrome,
                    sync_native_settings_window,
                    sync_settings_inputs,
                    sync_settings_input_chrome,
                    sync_action_chrome,
                    sync_status_text,
                )
                    .chain(),
            );
    }
}

/// Project Settings 是独立原生窗口。这里按事件来源窗口和显式活动字段路由输入，
/// 然后清空全局按键状态，防止同一次按键继续触发场景快捷键或视口工具。
fn capture_project_settings_keyboard(
    mut keyboard_events: MessageReader<KeyboardInput>,
    settings_windows: Query<Entity, With<ProjectSettingsWindow>>,
    mut inputs: Query<(&ProjectSettingsField, &mut EditableText)>,
    mut physical_keys: ResMut<ButtonInput<KeyCode>>,
    mut logical_keys: ResMut<ButtonInput<Key>>,
    mut dialog: ResMut<ProjectSettingsDialog>,
) {
    if !dialog.open {
        return;
    }

    let settings_window = settings_windows.iter().next();
    for event in keyboard_events.read() {
        if Some(event.window) != settings_window || !event.state.is_pressed() {
            continue;
        }
        if event.logical_key == Key::Escape {
            dialog.open = false;
            dialog.active_input = None;
            dialog.replace_active_input = false;
            continue;
        }

        let Some(active_input) = dialog.active_input else {
            continue;
        };
        let Ok((field, mut input)) = inputs.get_mut(active_input) else {
            dialog.active_input = None;
            dialog.replace_active_input = false;
            continue;
        };
        let Some(edit) = project_settings_text_edit(event, &logical_keys, *field) else {
            continue;
        };

        // Bevy 的标准输入观察器若已成功排队则只补充首次替换动作；若第二窗口
        // 没有完成标准路由，则在这里补排同一个编辑动作，且不会产生双重输入。
        let queued_position = input
            .pending_edits
            .iter()
            .position(|queued| queued == &edit);
        if dialog.replace_active_input && edit_replaces_selection(&edit) {
            let position = queued_position.unwrap_or(input.pending_edits.len());
            if !input
                .pending_edits
                .iter()
                .any(|queued| queued == &TextEdit::SelectAll)
            {
                input.pending_edits.insert(position, TextEdit::SelectAll);
            }
            dialog.replace_active_input = false;
        } else if !edit_replaces_selection(&edit) {
            dialog.replace_active_input = false;
        }
        if queued_position.is_none() {
            input.queue_edit(edit);
        }
    }

    physical_keys.reset_all();
    logical_keys.reset_all();
}

fn activate_project_settings_input(
    press: On<Pointer<Press>>,
    fields: Query<(), With<ProjectSettingsField>>,
    mut inputs: Query<(Entity, &mut EditableText), With<ProjectSettingsField>>,
    mut input_focus: ResMut<InputFocus>,
    mut dialog: ResMut<ProjectSettingsDialog>,
) {
    if !dialog.open || press.button != PointerButton::Primary || !fields.contains(press.entity) {
        return;
    }

    for (entity, mut input) in &mut inputs {
        if entity != press.entity {
            input.queue_edit(TextEdit::CollapseSelection);
        }
    }
    dialog.active_input = Some(press.entity);
    dialog.replace_active_input = true;
    input_focus.set(press.entity, FocusCause::Pressed);
}

fn project_settings_text_edit(
    event: &KeyboardInput,
    keys: &ButtonInput<Key>,
    field: ProjectSettingsField,
) -> Option<TextEdit> {
    let command = if cfg!(target_os = "macos") {
        keys.pressed(Key::Super)
    } else {
        keys.pressed(Key::Control)
    };
    let word = if cfg!(target_os = "macos") {
        keys.pressed(Key::Alt)
    } else {
        keys.pressed(Key::Control)
    };
    let shift = keys.pressed(Key::Shift);
    let alt = keys.pressed(Key::Alt);
    let super_key = keys.pressed(Key::Super);

    if command {
        match &event.logical_key {
            Key::Character(character) if character.eq_ignore_ascii_case("a") => {
                return Some(TextEdit::SelectAll);
            }
            Key::Character(character) if character.eq_ignore_ascii_case("c") => {
                return Some(TextEdit::Copy);
            }
            Key::Character(character) if character.eq_ignore_ascii_case("x") => {
                return Some(TextEdit::Cut);
            }
            Key::Character(character) if character.eq_ignore_ascii_case("v") => {
                return Some(TextEdit::Paste);
            }
            _ => {}
        }
    }

    match &event.logical_key {
        Key::Copy => Some(TextEdit::Copy),
        Key::Cut => Some(TextEdit::Cut),
        Key::Paste => Some(TextEdit::Paste),
        Key::Backspace if word => Some(TextEdit::BackspaceWord),
        Key::Delete if word => Some(TextEdit::DeleteWord),
        Key::Backspace => Some(TextEdit::Backspace),
        Key::Delete => Some(TextEdit::Delete),
        Key::ArrowLeft if word => Some(TextEdit::WordLeft(shift)),
        Key::ArrowRight if word => Some(TextEdit::WordRight(shift)),
        Key::ArrowLeft => Some(TextEdit::Left(shift)),
        Key::ArrowRight => Some(TextEdit::Right(shift)),
        Key::ArrowUp => Some(TextEdit::Up(shift)),
        Key::ArrowDown => Some(TextEdit::Down(shift)),
        Key::Home if command => Some(TextEdit::TextStart(shift)),
        Key::End if command => Some(TextEdit::TextEnd(shift)),
        Key::Home => Some(TextEdit::LineStart(shift)),
        Key::End => Some(TextEdit::LineEnd(shift)),
        Key::Character(_) | Key::Space if !command && !alt && !super_key => {
            let text = event
                .text
                .as_ref()
                .filter(|text| !text.is_empty())
                .map(ToString::to_string)
                .or_else(|| match &event.logical_key {
                    Key::Character(character) => Some(character.to_string()),
                    Key::Space => Some(" ".into()),
                    _ => None,
                })?;
            if matches!(
                field,
                ProjectSettingsField::WindowWidth | ProjectSettingsField::WindowHeight
            ) && !text.chars().all(|character| character.is_ascii_digit())
            {
                return None;
            }
            Some(TextEdit::Insert(text.into()))
        }
        _ => None,
    }
}

fn edit_replaces_selection(edit: &TextEdit) -> bool {
    matches!(
        edit,
        TextEdit::Cut
            | TextEdit::Paste
            | TextEdit::Insert(_)
            | TextEdit::Backspace
            | TextEdit::BackspaceWord
            | TextEdit::Delete
            | TextEdit::DeleteWord
    )
}

fn handle_project_settings_action(
    activate: On<Activate>,
    project_menu_buttons: Query<(), With<ProjectMenuButton>>,
    settings_menu_buttons: Query<(), With<ProjectSettingsMenuButton>>,
    actions: Query<&ProjectSettingsAction>,
    project: Res<ProjectRoot>,
    document: Res<SceneDocument>,
    mut inputs: Query<(&ProjectSettingsField, &mut EditableText)>,
    mut primary_window: Query<&mut Window, With<PrimaryWindow>>,
    mut dialog: ResMut<ProjectSettingsDialog>,
    mut display_settings: ResMut<ProjectDisplaySettings>,
    mut project_menu: ResMut<ProjectMenuState>,
) {
    if project_menu_buttons.contains(activate.entity) {
        project_menu.open = !project_menu.open;
        return;
    }

    if settings_menu_buttons.contains(activate.entity) {
        project_menu.open = false;
        match ProjectSettings::load(&project.root) {
            Ok(settings) => {
                dialog.draft = settings.into();
                dialog.status =
                    "Changes are stored in project.toml and used by the next game launch.".into();
                dialog.status_is_error = false;
            }
            Err(error) => {
                dialog.draft = default();
                dialog.status = error;
                dialog.status_is_error = true;
            }
        }
        dialog.open = true;
        dialog.ignore_input_changes = true;
        dialog.active_input = None;
        dialog.replace_active_input = false;
        return;
    }

    let Ok(action) = actions.get(activate.entity).copied() else {
        return;
    };
    match action {
        ProjectSettingsAction::Cancel => {
            dialog.open = false;
            dialog.active_input = None;
            dialog.replace_active_input = false;
        }
        ProjectSettingsAction::UseCurrentScene => {
            let Some(path) = document.path.as_deref() else {
                dialog.status = "Save or open a scene before setting the main scene.".into();
                dialog.status_is_error = true;
                return;
            };
            match project_relative_path(&project.root, path) {
                Ok(relative) => {
                    dialog.draft.main_scene = relative.clone();
                    for (field, mut input) in &mut inputs {
                        if *field == ProjectSettingsField::MainScene {
                            *input = EditableText::new(&relative);
                        }
                    }
                    mark_unsaved(&mut dialog);
                }
                Err(error) => {
                    dialog.status = error;
                    dialog.status_is_error = true;
                }
            }
        }
        ProjectSettingsAction::WindowMode(mode) => {
            dialog.draft.window_mode = mode;
            mark_unsaved(&mut dialog);
        }
        ProjectSettingsAction::Vsync(vsync) => {
            dialog.draft.vsync = vsync;
            mark_unsaved(&mut dialog);
        }
        ProjectSettingsAction::Apply => {
            copy_inputs_to_draft(&mut dialog.draft, &mut inputs);
            match validate_draft(&dialog.draft, &project.root).and_then(|settings| {
                save_project_settings(&project.root, &settings).map(|_| settings)
            }) {
                Ok(settings) => {
                    *display_settings = ProjectDisplaySettings::from(&settings);
                    if let Ok(mut window) = primary_window.single_mut() {
                        window.title = "Revy".into();
                    }
                    dialog.status = "Project settings saved successfully.".into();
                    dialog.status_is_error = false;
                }
                Err(error) => {
                    dialog.status = error;
                    dialog.status_is_error = true;
                }
            }
        }
    }
}

fn handle_native_window_close(
    mut events: MessageReader<WindowCloseRequested>,
    windows: Query<(), With<ProjectSettingsWindow>>,
    mut dialog: ResMut<ProjectSettingsDialog>,
) {
    if events.read().any(|event| windows.contains(event.window)) {
        dialog.open = false;
        dialog.active_input = None;
        dialog.replace_active_input = false;
    }
}

fn handle_settings_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut dialog: ResMut<ProjectSettingsDialog>,
    mut project_menu: ResMut<ProjectMenuState>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        if dialog.open {
            dialog.open = false;
        }
        project_menu.open = false;
    }
}

fn dismiss_project_menu_on_outside_click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut menu: ResMut<ProjectMenuState>,
    surfaces: Query<
        &Interaction,
        Or<(
            With<ProjectMenuButton>,
            With<ProjectMenuDropdown>,
            With<ProjectSettingsMenuButton>,
        )>,
    >,
) {
    if !menu.open || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let pointer_inside = surfaces
        .iter()
        .any(|interaction| *interaction != Interaction::None);
    if !pointer_inside {
        menu.open = false;
    }
}

fn sync_project_menu_visibility(
    menu: Res<ProjectMenuState>,
    mut dropdowns: Query<&mut Node, With<ProjectMenuDropdown>>,
) {
    if !menu.is_changed() {
        return;
    }
    for mut node in &mut dropdowns {
        node.display = if menu.open {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn sync_project_menu_chrome(
    menu: Res<ProjectMenuState>,
    mut top_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<ProjectMenuButton>, Without<ProjectSettingsMenuButton>),
    >,
    mut settings_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<ProjectSettingsMenuButton>, Without<ProjectMenuButton>),
    >,
) {
    for (interaction, mut background) in &mut top_buttons {
        background.0 = if menu.open || *interaction != Interaction::None {
            theme::bg_hover()
        } else {
            Color::NONE
        };
    }
    for (interaction, mut background) in &mut settings_buttons {
        background.0 = if *interaction != Interaction::None {
            theme::bg_hover()
        } else {
            Color::NONE
        };
    }
}

fn sync_native_settings_window(
    mut commands: Commands,
    dialog: Res<ProjectSettingsDialog>,
    project: Res<ProjectRoot>,
    windows: Query<Entity, With<ProjectSettingsWindow>>,
    cameras: Query<Entity, With<ProjectSettingsCamera>>,
    roots: Query<Entity, With<ProjectSettingsUiRoot>>,
) {
    if dialog.open {
        if windows.is_empty() {
            spawn_project_settings_window(&mut commands, &project, &dialog.draft);
        }
        return;
    }

    for entity in &roots {
        commands.entity(entity).try_despawn();
    }
    for entity in &cameras {
        commands.entity(entity).try_despawn();
    }
    for entity in &windows {
        commands.entity(entity).try_despawn();
    }
}

fn spawn_project_settings_window(
    commands: &mut Commands,
    project: &ProjectRoot,
    draft: &ProjectSettingsDraft,
) {
    let window = commands
        .spawn((
            ProjectSettingsWindow,
            Window {
                title: "Project Settings".into(),
                name: Some("arisna-project-settings".into()),
                position: WindowPosition::Centered(MonitorSelection::Primary),
                resolution: WindowResolution::new(920, 700),
                resize_constraints: WindowResizeConstraints {
                    min_width: 760.0,
                    min_height: 600.0,
                    ..default()
                },
                resizable: true,
                decorations: true,
                transparent: false,
                focused: true,
                window_level: WindowLevel::AlwaysOnTop,
                enabled_buttons: EnabledButtons {
                    minimize: false,
                    maximize: true,
                    close: true,
                },
                skip_taskbar: true,
                ..default()
            },
        ))
        .id();
    let camera = commands
        .spawn((
            ProjectSettingsCamera,
            Camera2d,
            Camera {
                clear_color: ClearColorConfig::Custom(theme::bg_app()),
                ..default()
            },
            RenderTarget::Window(WindowRef::Entity(window)),
        ))
        .id();
    let root = commands
        .spawn((
            ProjectSettingsUiRoot,
            UiTargetCamera(camera),
            Node {
                width: percent(100),
                height: percent(100),
                min_width: px(0),
                min_height: px(0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::bg_app()),
        ))
        .id();

    commands.entity(root).with_children(|root| {
        spawn_header(root, project);
        root.spawn(Node {
            width: percent(100),
            height: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_basis: px(0),
            flex_direction: FlexDirection::Row,
            ..default()
        })
        .with_children(|body| {
            spawn_sidebar(body);
            spawn_settings_form(body, draft);
        });
        spawn_footer(root);
    });
}

fn spawn_header(parent: &mut ChildSpawnerCommands, project: &ProjectRoot) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(72),
                min_height: px(72),
                padding: UiRect::axes(px(22), px(13)),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                row_gap: px(4),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme::bg_toolbar()),
            BorderColor::all(theme::border()),
        ))
        .with_children(|header| {
            spawn_text(header, "Project Settings", 19.0, theme::text_primary());
            spawn_text(
                header,
                project.root.to_string_lossy(),
                10.5,
                theme::text_muted(),
            );
        });
}

fn spawn_sidebar(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: px(205),
                min_width: px(180),
                height: percent(100),
                padding: UiRect::all(px(14)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                border: UiRect::right(px(1)),
                ..default()
            },
            BackgroundColor(theme::bg_panel_alt()),
            BorderColor::all(theme::border()),
        ))
        .with_children(|sidebar| {
            spawn_sidebar_entry(sidebar, "General", true);
            spawn_sidebar_entry(sidebar, "Application", false);
            spawn_sidebar_entry(sidebar, "Display / Window", false);
        });
}

fn spawn_sidebar_entry(parent: &mut ChildSpawnerCommands, label: &str, active: bool) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(34),
                min_height: px(34),
                padding: UiRect::horizontal(px(10)),
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(if active {
                theme::bg_selected()
            } else {
                Color::NONE
            }),
        ))
        .with_children(|entry| {
            spawn_text(
                entry,
                label,
                11.5,
                if active {
                    theme::text_primary()
                } else {
                    theme::text_muted()
                },
            );
        });
}

fn spawn_settings_form(parent: &mut ChildSpawnerCommands, draft: &ProjectSettingsDraft) {
    parent
        .spawn(Node {
            width: px(0),
            height: percent(100),
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_basis: px(0),
            padding: UiRect::axes(px(24), px(18)),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            overflow: Overflow::scroll_y(),
            ..default()
        })
        .with_children(|form| {
            spawn_section_title(form, "Application");
            spawn_field(
                form,
                "Project Name",
                ProjectSettingsField::ProjectName,
                &draft.project_name,
                true,
            );
            spawn_main_scene_field(form, &draft.main_scene);

            form.spawn(Node {
                width: percent(100),
                height: px(5),
                min_height: px(5),
                ..default()
            });
            spawn_section_title(form, "Display / Window");
            spawn_field(
                form,
                "Window Title",
                ProjectSettingsField::WindowTitle,
                &draft.window_title,
                false,
            );
            form.spawn(Node {
                width: percent(100),
                flex_direction: FlexDirection::Row,
                column_gap: px(12),
                ..default()
            })
            .with_children(|resolution| {
                spawn_field(
                    resolution,
                    "Width",
                    ProjectSettingsField::WindowWidth,
                    &draft.window_width,
                    false,
                );
                spawn_field(
                    resolution,
                    "Height",
                    ProjectSettingsField::WindowHeight,
                    &draft.window_height,
                    false,
                );
            });
            spawn_option_row(
                form,
                "Window Mode",
                &[
                    (
                        "Windowed",
                        ProjectSettingsAction::WindowMode(ProjectWindowMode::Windowed),
                    ),
                    (
                        "Borderless",
                        ProjectSettingsAction::WindowMode(ProjectWindowMode::Borderless),
                    ),
                    (
                        "Fullscreen",
                        ProjectSettingsAction::WindowMode(ProjectWindowMode::Fullscreen),
                    ),
                ],
            );
            spawn_option_row(
                form,
                "Vertical Sync",
                &[
                    ("Enabled", ProjectSettingsAction::Vsync(true)),
                    ("Disabled", ProjectSettingsAction::Vsync(false)),
                ],
            );
        });
}

fn spawn_section_title(parent: &mut ChildSpawnerCommands, title: &str) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(30),
                min_height: px(30),
                align_items: AlignItems::Center,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|row| {
            spawn_text(row, title, 13.5, theme::text_primary());
        });
}

fn spawn_field(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    field: ProjectSettingsField,
    current: &str,
    autofocus: bool,
) {
    let shares_horizontal_row = matches!(
        field,
        ProjectSettingsField::WindowWidth | ProjectSettingsField::WindowHeight
    );
    parent
        .spawn(Node {
            width: if shares_horizontal_row {
                px(0)
            } else {
                percent(100)
            },
            min_width: px(0),
            flex_grow: if shares_horizontal_row { 1.0 } else { 0.0 },
            flex_basis: if shares_horizontal_row {
                px(0)
            } else {
                Val::Auto
            },
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            ..default()
        })
        .with_children(|block| {
            spawn_text(block, label, 11.0, theme::text_muted());
            let mut field_entity = block.spawn((
                field,
                ManualKeyboardInput,
                EditableText::new(current),
                TextCursorStyle::default(),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: percent(100),
                    height: px(34),
                    min_height: px(34),
                    min_width: px(0),
                    padding: UiRect::axes(px(9), px(6)),
                    border: UiRect::all(px(1)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ));
            if matches!(
                field,
                ProjectSettingsField::WindowWidth | ProjectSettingsField::WindowHeight
            ) {
                field_entity.insert(EditableTextFilter::new(|character| {
                    character.is_ascii_digit()
                }));
            }
            if autofocus {
                field_entity.insert(AutoFocus);
            }
        });
}

fn spawn_main_scene_field(parent: &mut ChildSpawnerCommands, current: &str) {
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            ..default()
        })
        .with_children(|block| {
            spawn_text(
                block,
                "Main Scene (project-relative .bsn path)",
                11.0,
                theme::text_muted(),
            );
            block
                .spawn(Node {
                    width: percent(100),
                    height: px(34),
                    column_gap: px(7),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        ProjectSettingsField::MainScene,
                        ManualKeyboardInput,
                        EditableText::new(current),
                        TextCursorStyle::default(),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                        Node {
                            width: px(0),
                            height: percent(100),
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            padding: UiRect::axes(px(9), px(6)),
                            border: UiRect::all(px(1)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(theme::bg_field()),
                        BorderColor::all(theme::border_soft()),
                    ));
                    spawn_action_button(
                        row,
                        ProjectSettingsAction::UseCurrentScene,
                        "Use Current",
                        104.0,
                    );
                });
        });
}

fn spawn_option_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    options: &[(&str, ProjectSettingsAction)],
) {
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            ..default()
        })
        .with_children(|block| {
            spawn_text(block, label, 11.0, theme::text_muted());
            block
                .spawn(Node {
                    width: percent(100),
                    height: px(32),
                    column_gap: px(6),
                    ..default()
                })
                .with_children(|row| {
                    for (caption, action) in options.iter().copied() {
                        spawn_action_button(row, action, caption, 112.0);
                    }
                });
        });
}

fn spawn_footer(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(62),
                min_height: px(62),
                padding: UiRect::axes(px(18), px(12)),
                align_items: AlignItems::Center,
                column_gap: px(9),
                border: UiRect::top(px(1)),
                ..default()
            },
            BackgroundColor(theme::bg_toolbar()),
            BorderColor::all(theme::border()),
        ))
        .with_children(|footer| {
            footer.spawn((
                ProjectSettingsStatus,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
                Node {
                    width: px(0),
                    min_width: px(0),
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    ..default()
                },
            ));
            spawn_action_button(footer, ProjectSettingsAction::Cancel, "Cancel", 86.0);
            spawn_action_button(footer, ProjectSettingsAction::Apply, "Apply", 86.0);
        });
}

fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    action: ProjectSettingsAction,
    label: &str,
    width: f32,
) {
    parent
        .spawn((
            Button,
            WidgetButton,
            action,
            Node {
                width: px(width),
                min_width: px(width),
                height: px(32),
                min_height: px(32),
                padding: UiRect::horizontal(px(9)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(theme::bg_field()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|button| {
            spawn_text(button, label, 11.0, theme::text_primary());
        });
}

fn spawn_text(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    size: f32,
    color: Color,
) -> Entity {
    parent
        .spawn((
            Text::new(value),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
        ))
        .id()
}

fn sync_settings_inputs(
    mut inputs: Query<(&ProjectSettingsField, &EditableText), Changed<EditableText>>,
    mut dialog: ResMut<ProjectSettingsDialog>,
) {
    let mut changed = false;
    for (field, input) in &mut inputs {
        set_draft_field(&mut dialog.draft, *field, input.value());
        changed = true;
    }
    if !changed {
        return;
    }
    if dialog.ignore_input_changes {
        dialog.ignore_input_changes = false;
    } else {
        mark_unsaved(&mut dialog);
    }
}

/// 活动字段使用唯一强调边框，避免多个残留选区让用户误判键盘输入目标。
fn sync_settings_input_chrome(
    dialog: Res<ProjectSettingsDialog>,
    mut inputs: Query<(Entity, &mut BorderColor), With<ProjectSettingsField>>,
) {
    if !dialog.is_changed() {
        return;
    }
    for (entity, mut border) in &mut inputs {
        *border = BorderColor::all(if dialog.active_input == Some(entity) {
            theme::accent()
        } else {
            theme::border_soft()
        });
    }
}

fn sync_action_chrome(
    dialog: Res<ProjectSettingsDialog>,
    mut buttons: Query<
        (
            &ProjectSettingsAction,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Button>,
    >,
) {
    for (action, interaction, mut background, mut border) in &mut buttons {
        let selected = match *action {
            ProjectSettingsAction::WindowMode(mode) => dialog.draft.window_mode == mode,
            ProjectSettingsAction::Vsync(vsync) => dialog.draft.vsync == vsync,
            _ => false,
        };
        let primary = *action == ProjectSettingsAction::Apply;
        background.0 = match interaction {
            Interaction::Pressed => theme::bg_selected_pressed(),
            Interaction::Hovered if primary || selected => theme::accent_hover(),
            Interaction::Hovered => theme::bg_hover(),
            Interaction::None if primary => theme::accent(),
            Interaction::None if selected => theme::bg_selected(),
            Interaction::None => theme::bg_field(),
        };
        *border = BorderColor::all(if primary || selected {
            theme::accent()
        } else {
            theme::border_soft()
        });
    }
}

fn sync_status_text(
    dialog: Res<ProjectSettingsDialog>,
    mut labels: Query<(&mut Text, &mut TextColor), With<ProjectSettingsStatus>>,
) {
    if !dialog.is_changed() {
        return;
    }
    for (mut text, mut color) in &mut labels {
        text.0 = dialog.status.clone();
        color.0 = if dialog.status_is_error {
            theme::warning()
        } else {
            theme::text_muted()
        };
    }
}

fn copy_inputs_to_draft(
    draft: &mut ProjectSettingsDraft,
    inputs: &mut Query<(&ProjectSettingsField, &mut EditableText)>,
) {
    for (field, input) in inputs.iter_mut() {
        set_draft_field(draft, *field, input.value());
    }
}

fn set_draft_field(
    draft: &mut ProjectSettingsDraft,
    field: ProjectSettingsField,
    value: impl ToString,
) {
    let value = value.to_string();
    match field {
        ProjectSettingsField::ProjectName => draft.project_name = value,
        ProjectSettingsField::MainScene => draft.main_scene = value,
        ProjectSettingsField::WindowTitle => draft.window_title = value,
        ProjectSettingsField::WindowWidth => draft.window_width = value,
        ProjectSettingsField::WindowHeight => draft.window_height = value,
    }
}

fn mark_unsaved(dialog: &mut ProjectSettingsDialog) {
    dialog.status = "Unsaved changes".into();
    dialog.status_is_error = false;
}

fn validate_draft(
    draft: &ProjectSettingsDraft,
    project_root: &Path,
) -> Result<ProjectSettings, String> {
    let name = draft.project_name.trim();
    if name.is_empty() {
        return Err("Project Name cannot be empty.".into());
    }
    if name.chars().count() > 128 || name.chars().any(char::is_control) {
        return Err("Project Name must be 128 printable characters or fewer.".into());
    }
    let title = draft.window_title.trim();
    if title.is_empty() {
        return Err("Window Title cannot be empty.".into());
    }
    let width = parse_dimension("Width", &draft.window_width, 320)?;
    let height = parse_dimension("Height", &draft.window_height, 240)?;
    let main_scene = normalize_main_scene(project_root, &draft.main_scene)?;

    Ok(ProjectSettings {
        name: name.to_owned(),
        main_scene,
        window: ProjectWindowSettings {
            title: title.to_owned(),
            width,
            height,
            mode: draft.window_mode,
            vsync: draft.vsync,
        },
    })
}

fn parse_dimension(label: &str, value: &str, minimum: u32) -> Result<u32, String> {
    let parsed = value
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("{label} must be a whole number."))?;
    if !(minimum..=16384).contains(&parsed) {
        return Err(format!(
            "{label} must be between {minimum} and 16384 pixels."
        ));
    }
    Ok(parsed)
}

fn normalize_main_scene(project_root: &Path, input: &str) -> Result<String, String> {
    let normalized = input
        .trim()
        .strip_prefix("res://")
        .unwrap_or(input.trim())
        .replace('\\', "/");
    if normalized.is_empty() {
        return Ok(String::new());
    }
    let relative = Path::new(&normalized);
    if relative.is_absolute()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err("Main Scene must stay inside the project directory.".into());
    }
    if relative
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("bsn"))
    {
        return Err("Main Scene must be a .bsn scene file.".into());
    }
    if !project_root.join(relative).is_file() {
        return Err(format!("Main Scene does not exist: res://{normalized}"));
    }
    Ok(normalized)
}

fn project_relative_path(project_root: &Path, path: &Path) -> Result<String, String> {
    let root = fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.strip_prefix(&root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .map_err(|_| "The current scene is outside the active project.".to_string())
}

fn save_project_settings(project_root: &Path, settings: &ProjectSettings) -> Result<(), String> {
    let manifest_path = project_root.join("project.toml");
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("Could not read project.toml: {error}"))?;
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| format!("Invalid project.toml: {error}"))?;
    if document
        .get("project")
        .and_then(|item| item.as_table_like())
        .is_none()
    {
        return Err("project.toml is missing [project]".into());
    }

    let rendered = update_toml_section(
        &source,
        "project",
        &[
            ("name", toml_string(&settings.name)),
            ("main_scene", toml_string(&settings.main_scene)),
        ],
        false,
    )?;
    let rendered = update_toml_section(
        &rendered,
        "display.window",
        &[
            ("title", toml_string(&settings.window.title)),
            ("width", settings.window.width.to_string()),
            ("height", settings.window.height.to_string()),
            ("mode", toml_string(settings.window.mode.as_str())),
            ("vsync", settings.window.vsync.to_string()),
        ],
        true,
    )?;

    fs::write(&manifest_path, rendered)
        .map_err(|error| format!("Could not save project.toml: {error}"))
}

pub(crate) fn update_toml_section(
    source: &str,
    section: &str,
    replacements: &[(&str, String)],
    create: bool,
) -> Result<String, String> {
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let ended_with_newline = source.ends_with('\n');
    let mut lines: Vec<String> = source.lines().map(str::to_owned).collect();
    let header = format!("[{section}]");
    let section_start = lines.iter().position(|line| line.trim() == header);
    let section_start = match section_start {
        Some(index) => index,
        None if create => {
            if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.len() - 1
        }
        None => return Err(format!("project.toml is missing [{section}]")),
    };
    let mut section_end = lines
        .iter()
        .enumerate()
        .skip(section_start + 1)
        .find(|(_, line)| {
            let line = line.trim();
            line.starts_with('[') && line.ends_with(']')
        })
        .map(|(index, _)| index)
        .unwrap_or(lines.len());

    for (key, literal) in replacements {
        let existing = (section_start + 1..section_end).find(|&index| {
            lines[index]
                .split_once('=')
                .is_some_and(|(candidate, _)| candidate.trim() == *key)
        });
        let replacement = format!("{key} = {literal}");
        if let Some(index) = existing {
            lines[index] = replacement;
        } else {
            lines.insert(section_end, replacement);
            section_end += 1;
        }
    }

    let mut rendered = lines.join(newline);
    if ended_with_newline || !rendered.is_empty() {
        rendered.push_str(newline);
    }
    Ok(rendered)
}

fn toml_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('\"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::{ButtonState, InputPlugin, keyboard::KeyboardInput},
        input_focus::{FocusCause, InputDispatchPlugin, InputFocus, InputFocusPlugin},
        picking::events::{Pointer, Release},
        text::TextEdit,
        ui::UiScale,
        ui_widgets::EditableTextInputPlugin,
        window::Ime,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_project() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("arisna-editor-settings-{nonce}"));
        fs::create_dir_all(root.join("assets/scenes")).unwrap();
        fs::write(root.join("assets/scenes/main.bsn"), "(scene:())").unwrap();
        fs::write(
            root.join("project.toml"),
            "[project]\nname = \"Original\"\nformat_version = 1\nmain_scene = \"\"\n\n[custom]\nkeep = true\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn project_settings_save_preserves_unrelated_manifest_values() {
        let root = temporary_project();
        let draft = ProjectSettingsDraft {
            project_name: "Edited Project".into(),
            main_scene: "res://assets/scenes/main.bsn".into(),
            window_title: "Edited Window".into(),
            window_width: "1440".into(),
            window_height: "810".into(),
            window_mode: ProjectWindowMode::Borderless,
            vsync: false,
        };
        let settings = validate_draft(&draft, &root).unwrap();
        save_project_settings(&root, &settings).unwrap();

        let source = fs::read_to_string(root.join("project.toml")).unwrap();
        assert!(source.contains("keep = true"));
        let loaded = ProjectSettings::load(&root).unwrap();
        assert_eq!(loaded.name, "Edited Project");
        assert_eq!(loaded.main_scene, "assets/scenes/main.bsn");
        assert_eq!(loaded.window.width, 1440);
        assert_eq!(loaded.window.height, 810);
        assert_eq!(loaded.window.mode, ProjectWindowMode::Borderless);
        assert!(!loaded.window.vsync);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_settings_reject_paths_outside_the_project() {
        let root = temporary_project();
        assert!(normalize_main_scene(&root, "../outside.bsn").is_err());
        assert!(normalize_main_scene(&root, "assets/scenes/missing.bsn").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_menu_toggles_without_opening_settings_directly() {
        let mut app = App::new();
        app.insert_resource(ProjectRoot::new("."))
            .init_resource::<SceneDocument>()
            .init_resource::<ProjectSettingsDialog>()
            .init_resource::<ProjectDisplaySettings>()
            .init_resource::<ProjectMenuState>()
            .add_observer(handle_project_settings_action);
        let button = app.world_mut().spawn(ProjectMenuButton).id();

        app.world_mut().trigger(Activate { entity: button });
        assert!(app.world().resource::<ProjectMenuState>().open);
        assert!(!app.world().resource::<ProjectSettingsDialog>().open);

        app.world_mut().trigger(Activate { entity: button });
        assert!(!app.world().resource::<ProjectMenuState>().open);
    }

    #[test]
    fn project_display_settings_use_the_saved_window_resolution() {
        let settings = ProjectSettings {
            window: ProjectWindowSettings {
                width: 960,
                height: 540,
                ..default()
            },
            ..default()
        };

        assert_eq!(
            ProjectDisplaySettings::from(&settings),
            ProjectDisplaySettings {
                width: 960,
                height: 540,
            }
        );
    }

    #[test]
    fn open_project_settings_capture_keyboard_before_scene_shortcuts() {
        let mut app = App::new();
        app.add_plugins(InputPlugin)
            .init_resource::<ProjectSettingsDialog>()
            .add_systems(
                PreUpdate,
                capture_project_settings_keyboard.after(InputSystems),
            );
        app.world_mut().spawn(ProjectSettingsWindow);
        app.world_mut().resource_mut::<ProjectSettingsDialog>().open = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Digit2);
        app.world_mut()
            .resource_mut::<ButtonInput<Key>>()
            .press(Key::Character("2".into()));

        app.update();

        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .get_pressed()
                .next()
                .is_none()
        );
        assert!(
            app.world()
                .resource::<ButtonInput<Key>>()
                .get_pressed()
                .next()
                .is_none()
        );
    }

    fn project_settings_input_test_app() -> (App, Entity, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins(InputPlugin)
            .init_resource::<ProjectSettingsDialog>()
            .add_systems(
                PreUpdate,
                capture_project_settings_keyboard.after(InputSystems),
            );
        let settings_window = app.world_mut().spawn(ProjectSettingsWindow).id();
        let width = app
            .world_mut()
            .spawn((ProjectSettingsField::WindowWidth, EditableText::new("960")))
            .id();
        let height = app
            .world_mut()
            .spawn((ProjectSettingsField::WindowHeight, EditableText::new("540")))
            .id();
        for entity in [width, height] {
            app.world_mut()
                .entity_mut(entity)
                .get_mut::<EditableText>()
                .unwrap()
                .pending_edits
                .clear();
        }
        {
            let mut dialog = app.world_mut().resource_mut::<ProjectSettingsDialog>();
            dialog.open = true;
            dialog.active_input = Some(width);
            dialog.replace_active_input = true;
        }
        (app, settings_window, width, height)
    }

    #[test]
    fn project_settings_routes_character_only_to_active_secondary_window_field() {
        let (mut app, settings_window, width, height) = project_settings_input_test_app();
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Digit2,
            logical_key: Key::Character("2".into()),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: settings_window,
        });

        app.update();

        assert_eq!(
            app.world()
                .entity(width)
                .get::<EditableText>()
                .unwrap()
                .pending_edits,
            [TextEdit::SelectAll, TextEdit::Insert("2".into())]
        );
        assert!(
            app.world()
                .entity(height)
                .get::<EditableText>()
                .unwrap()
                .pending_edits
                .is_empty()
        );
        assert!(
            !app.world()
                .resource::<ProjectSettingsDialog>()
                .replace_active_input
        );
    }

    #[test]
    fn project_settings_rejects_non_numeric_resolution_input() {
        let (mut app, settings_window, width, _) = project_settings_input_test_app();
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyX,
            logical_key: Key::Character("x".into()),
            state: ButtonState::Pressed,
            text: Some("x".into()),
            repeat: false,
            window: settings_window,
        });

        app.update();

        assert!(
            app.world()
                .entity(width)
                .get::<EditableText>()
                .unwrap()
                .pending_edits
                .is_empty()
        );
    }

    #[test]
    fn secondary_window_character_without_text_reaches_editable_text() {
        let mut app = App::new();
        app.add_plugins((
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            EditableTextInputPlugin,
        ))
        .init_resource::<UiScale>()
        .add_message::<Ime>()
        .add_message::<Pointer<Release>>();
        let primary_window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let settings_window = app.world_mut().spawn(Window::default()).id();
        app.update();

        let input = app.world_mut().spawn(EditableText::new("960")).id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, FocusCause::Pressed);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Digit2,
            logical_key: Key::Character("2".into()),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: settings_window,
        });

        app.update();

        let edits = &app
            .world()
            .entity(input)
            .get::<EditableText>()
            .unwrap()
            .pending_edits;
        assert!(
            edits
                .iter()
                .any(|edit| edit == &TextEdit::Insert("2".into()))
        );
        assert_ne!(primary_window, settings_window);
    }
}
