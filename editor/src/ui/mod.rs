//! 编辑器 UI 组合层。
//!
//! 稳定外壳由 BSN 场景声明；需要动态重建子节点的面板，在外壳生成后
//! 挂载到明确的 Host。这样 shell.rs 只负责布局，不持有业务状态。

pub mod theme;

pub(crate) mod components;
mod shell;
mod widgets;

use bevy::{
    feathers::{
        FeathersPlugins, cursor::EntityCursor, dark_theme::create_dark_theme, theme::UiTheme,
    },
    gizmos::transform_gizmo::TransformGizmoSettings,
    prelude::*,
    ui_widgets::{Activate, ControlOrientation, ScrollArea, Scrollbar, ScrollbarThumb},
    window::SystemCursorIcon,
};

use crate::{
    animation_timeline::spawn_animation_timeline,
    filesystem::spawn_filesystem_dock,
    inspector::spawn_inspector_body,
    panels::{BottomDockSplitter, SplitterKind, attach_splitter},
    viewport::{Snap2dSettings, setup_viewport},
    workspace::WorkspaceMode,
};

use components::{
    AnimationTimelineHost, DetailsHost, DetailsSplitterHost, EditorVerticalScrollArea,
    EditorVerticalScrollbar, FileSystemHost, FileSystemSplitterHost, GizmoToolbarButton,
    SceneSplitterHost, Snap2dGridButton, Snap2dStepButton, Snap2dStepLabel, Snap2dToolbarButton,
    ViewportPreviewSplitterHost, ViewportSideSplitterHost, WorkspaceToolbarGroup,
};

pub struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FeathersPlugins)
            .insert_resource(UiTheme(create_dark_theme()))
            .add_observer(handle_gizmo_toolbar)
            .add_observer(handle_snap_2d_toolbar)
            .add_systems(
                Startup,
                (shell::editor_shell.spawn(), mount_dynamic_panels)
                    .chain()
                    .after(setup_viewport),
            )
            .add_systems(
                Update,
                (
                    mount_vertical_scrollbars,
                    sync_gizmo_toolbar,
                    sync_snap_2d_toolbar,
                    sync_workspace_chrome,
                ),
            );
    }
}

fn mount_vertical_scrollbars(
    mut commands: Commands,
    targets: Query<(Entity, &ChildOf), Added<EditorVerticalScrollArea>>,
) {
    // ScrollArea 必须放在实际滚动目标上，Scrollbar 则是其父 Host 的兄弟节点。
    // 若把滚动条生成到目标内部，滚动时轨道也会跟随内容移动。
    for (target, parent) in &targets {
        commands.entity(target).insert(ScrollArea);
        commands.entity(parent.parent()).with_children(|host| {
            host.spawn((
                EditorVerticalScrollbar,
                Scrollbar::new(target, ControlOrientation::Vertical, 28.0),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(2.0),
                    top: Val::Px(4.0),
                    bottom: Val::Px(4.0),
                    width: Val::Px(8.0),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.11, 0.115, 0.13)),
                ZIndex(1),
            ))
            .with_children(|track| {
                track.spawn((
                    ScrollbarThumb {
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        border: UiRect::ZERO,
                    },
                    BackgroundColor(Color::srgb(0.42, 0.44, 0.49)),
                    EntityCursor::System(SystemCursorIcon::Pointer),
                ));
            });
        });
    }
}

fn mount_dynamic_panels(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    details_hosts: Query<Entity, With<DetailsHost>>,
    animation_timeline_hosts: Query<Entity, With<AnimationTimelineHost>>,
    filesystem_hosts: Query<Entity, With<FileSystemHost>>,
    scene_splitters: Query<Entity, With<SceneSplitterHost>>,
    details_splitters: Query<Entity, With<DetailsSplitterHost>>,
    filesystem_splitters: Query<Entity, With<FileSystemSplitterHost>>,
    bottom_dock_splitters: Query<Entity, With<BottomDockSplitter>>,
    viewport_side_splitters: Query<Entity, With<ViewportSideSplitterHost>>,
    viewport_preview_splitters: Query<Entity, With<ViewportPreviewSplitterHost>>,
) {
    // Inspector 和 FileSystem 的子项会随项目/选择变化，因此不能写死在 BSN 外壳中。
    for entity in &details_hosts {
        commands
            .entity(entity)
            .with_children(|parent| spawn_inspector_body(parent, &asset_server));
    }

    for entity in &animation_timeline_hosts {
        commands
            .entity(entity)
            .with_children(|parent| spawn_animation_timeline(parent, &asset_server));
    }

    for entity in &filesystem_hosts {
        commands.entity(entity).with_children(spawn_filesystem_dock);
    }

    for entity in &scene_splitters {
        attach_splitter(&mut commands, entity, SplitterKind::SceneHorizontal);
    }
    for entity in &details_splitters {
        attach_splitter(&mut commands, entity, SplitterKind::DetailsHorizontal);
    }
    for entity in &filesystem_splitters {
        attach_splitter(&mut commands, entity, SplitterKind::FileSystemVertical);
    }
    for entity in &bottom_dock_splitters {
        attach_splitter(&mut commands, entity, SplitterKind::BottomDockVertical);
    }
    for entity in &viewport_side_splitters {
        attach_splitter(&mut commands, entity, SplitterKind::ViewportSideHorizontal);
    }
    for entity in &viewport_preview_splitters {
        attach_splitter(&mut commands, entity, SplitterKind::ViewportPreviewVertical);
    }
}

fn handle_gizmo_toolbar(
    activate: On<Activate>,
    buttons: Query<&GizmoToolbarButton>,
    mut settings: ResMut<TransformGizmoSettings>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        settings.mode = button.0;
    }
}

fn sync_gizmo_toolbar(
    settings: Res<TransformGizmoSettings>,
    mut buttons: Query<(&GizmoToolbarButton, &mut BorderColor)>,
) {
    if !settings.is_changed() {
        return;
    }

    for (button, mut border) in &mut buttons {
        *border = BorderColor::all(if button.0 == settings.mode {
            theme::accent()
        } else {
            Color::NONE
        });
    }
}

fn handle_snap_2d_toolbar(
    activate: On<Activate>,
    toggles: Query<(), With<Snap2dToolbarButton>>,
    grids: Query<(), With<Snap2dGridButton>>,
    steps: Query<(), With<Snap2dStepButton>>,
    project: Option<Res<arisna_engine::ProjectRoot>>,
    mut settings: ResMut<Snap2dSettings>,
) {
    let changed = if toggles.get(activate.entity).is_ok() {
        settings.enabled = !settings.enabled;
        true
    } else if grids.get(activate.entity).is_ok() {
        settings.grid_visible = !settings.grid_visible;
        true
    } else if steps.get(activate.entity).is_ok() {
        settings.cycle_grid_size();
        true
    } else {
        false
    };
    if changed
        && let Some(project) = project.as_deref()
        && let Err(error) = settings.persist(project)
    {
        warn!("Could not persist 2D snap settings: {error}");
    }
}

fn sync_snap_2d_toolbar(
    settings: Res<Snap2dSettings>,
    mut buttons: Query<
        (
            &mut BorderColor,
            Has<Snap2dToolbarButton>,
            Has<Snap2dGridButton>,
        ),
        Or<(With<Snap2dToolbarButton>, With<Snap2dGridButton>)>,
    >,
    mut labels: Query<&mut Text, With<Snap2dStepLabel>>,
) {
    if !settings.is_changed() {
        return;
    }
    for (mut border, snap_button, grid_button) in &mut buttons {
        let active = (snap_button && settings.enabled) || (grid_button && settings.grid_visible);
        *border = BorderColor::all(if active { theme::accent() } else { Color::NONE });
    }
    for mut label in &mut labels {
        label.0 = format!("{} px", settings.grid_size as u32);
    }
}

fn sync_workspace_chrome(
    mode: Res<WorkspaceMode>,
    mut groups: Query<(&WorkspaceToolbarGroup, &mut Node)>,
) {
    if !mode.is_changed() {
        return;
    }

    for (group, mut node) in &mut groups {
        node.display = if group.0 == *mode {
            Display::Flex
        } else {
            Display::None
        };
    }
}
