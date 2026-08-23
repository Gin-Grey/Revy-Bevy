mod model;

use std::{collections::HashMap, fs, path::PathBuf, process::Command};

use arisna_engine::{PlatformPlugin, native_render_plugin};
use bevy::{
    app::AppExit,
    asset::{Assets, RenderAssetUsages},
    feathers::{FeathersPlugins, dark_theme::create_dark_theme, theme::UiTheme},
    image::{CompressedImageFormats, ImageSampler, ImageType},
    picking::Pickable,
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui_widgets::{Activate, Button as WidgetButton},
    window::{WindowResizeConstraints, WindowResolution},
};

use crate::paths;
use model::{
    RecentProject, RecentProjects, create_project, create_target_path, create_validation,
    now_epoch_seconds, relative_time, validate_project,
};

pub fn run() {
    let recents = RecentProjects::load();
    let state = ManagerState::new(recents);
    App::new()
        .insert_resource(ClearColor(bg_app()))
        .insert_resource(state)
        .init_resource::<DialogState>()
        .init_resource::<ThumbnailCache>()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Revy Engine - Project Manager".into(),
                        resolution: WindowResolution::new(1180, 760),
                        resize_constraints: WindowResizeConstraints {
                            min_width: 760.0,
                            min_height: 520.0,
                            ..default()
                        },
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: paths::editor_asset_root().to_string_lossy().into_owned(),
                    ..default()
                })
                .set(native_render_plugin()),
        )
        .add_plugins(PlatformPlugin)
        .add_plugins(FeathersPlugins)
        .insert_resource(UiTheme(create_dark_theme()))
        .add_observer(handle_manager_action)
        .add_systems(Startup, (load_manager_font, setup_manager_ui).chain())
        .add_systems(
            Update,
            (
                sync_search,
                sync_dialog_inputs,
                handle_dialog_escape,
                rebuild_project_list,
                rebuild_details_panel,
                rebuild_dialog,
                sync_project_row_chrome,
                sync_button_chrome,
                sync_status_text,
                sync_sort_label,
                sync_dialog_validation,
            )
                .chain(),
        )
        .run();
}

pub fn remember_project(path: &std::path::Path) {
    let mut recents = RecentProjects::load();
    if recents.add_or_update(path, now_epoch_seconds()).is_ok() {
        let _ = recents.save();
    }
}

#[derive(Resource)]
struct ManagerState {
    recents: RecentProjects,
    selected: Option<PathBuf>,
    filter: String,
    sort: ProjectSort,
    status: String,
    revision: u64,
}

impl ManagerState {
    fn new(recents: RecentProjects) -> Self {
        let selected = recents
            .last_project
            .as_ref()
            .filter(|path| {
                recents
                    .projects
                    .iter()
                    .any(|project| model::same_path(&project.path, path))
            })
            .and_then(|path| {
                recents
                    .projects
                    .iter()
                    .find(|project| model::same_path(&project.path, path))
                    .map(|project| project.path.clone())
            })
            .or_else(|| recents.projects.first().map(|project| project.path.clone()));
        let status = if recents.projects.is_empty() {
            "Create or import a project to get started".into()
        } else {
            format!("{} project(s)", recents.projects.len())
        };
        Self {
            recents,
            selected,
            filter: String::new(),
            sort: ProjectSort::LastOpened,
            status,
            revision: 1,
        }
    }

    fn selected_project(&self) -> Option<&RecentProject> {
        let selected = self.selected.as_ref()?;
        self.recents
            .projects
            .iter()
            .find(|project| &project.path == selected)
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn persist(&mut self) {
        if let Err(error) = self.recents.save() {
            self.status = error;
        }
    }
}

#[derive(Resource, Default)]
struct DialogState {
    kind: DialogKind,
    project_name: String,
    create_parent: String,
    import_path: String,
    error_title: String,
    error_message: String,
    error_return: DialogKind,
    revision: u64,
}

impl DialogState {
    fn show_create(&mut self) {
        self.kind = DialogKind::Create;
        self.project_name = "My Game".into();
        self.create_parent = default_project_parent().to_string_lossy().into_owned();
        self.revision = self.revision.wrapping_add(1);
    }

    fn show_import(&mut self) {
        self.kind = DialogKind::Import;
        self.import_path.clear();
        self.revision = self.revision.wrapping_add(1);
    }

    fn close(&mut self) {
        self.kind = if self.kind == DialogKind::Error {
            std::mem::take(&mut self.error_return)
        } else {
            DialogKind::None
        };
        self.revision = self.revision.wrapping_add(1);
    }

    fn show_error(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.error_return = match self.kind {
            DialogKind::Create | DialogKind::Import => self.kind,
            DialogKind::None | DialogKind::Error => DialogKind::None,
        };
        self.kind = DialogKind::Error;
        self.error_title = title.into();
        self.error_message = message.into();
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DialogKind {
    #[default]
    None,
    Create,
    Import,
    Error,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ProjectSort {
    #[default]
    LastOpened,
    Name,
}

#[derive(Resource, Default)]
struct ManagerFont(Option<Handle<Font>>);

#[derive(Resource, Default)]
struct ThumbnailCache(HashMap<PathBuf, Handle<Image>>);

#[derive(Component)]
struct ProjectListHost;

#[derive(Component)]
struct DetailsHost;

#[derive(Component)]
struct DialogHost;

#[derive(Component)]
struct ManagerStatusText;

#[derive(Component)]
struct SortLabel;

#[derive(Component)]
struct SearchInput;

#[derive(Component)]
struct CreateNameInput;

#[derive(Component)]
struct CreateParentInput;

#[derive(Component)]
struct ImportPathInput;

#[derive(Component)]
struct DialogValidationText;

#[derive(Component)]
struct CreateTargetText;

#[derive(Component, Clone)]
struct ProjectRow(PathBuf);

#[derive(Component, Clone, Copy)]
struct ButtonChrome {
    normal: Color,
    hovered: Color,
    pressed: Color,
    border: Color,
}

#[derive(Component, Clone, Copy, Debug)]
enum ManagerAction {
    NewProject,
    ImportProject,
    Refresh,
    ToggleSort,
    OpenSelected,
    ShowSelectedFolder,
    RemoveSelected,
    RemoveMissing,
    BrowseCreateParent,
    BrowseImportPath,
    ConfirmCreate,
    ConfirmImport,
    CancelDialog,
}

fn load_manager_font(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let handle = system_font_candidates().into_iter().find_map(|path| {
        fs::read(path)
            .ok()
            .map(|bytes| fonts.add(Font::from_bytes(bytes)))
    });
    commands.insert_resource(ManagerFont(handle));
}

fn setup_manager_ui(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    font: Res<ManagerFont>,
) {
    commands.spawn(Camera2d);
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                min_width: px(760),
                min_height: px(520),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(bg_app()),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: percent(100),
                    height: px(72),
                    min_height: px(72),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(20)),
                    column_gap: px(12),
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BackgroundColor(bg_header()),
                BorderColor::all(border()),
            ))
            .with_children(|header| {
                header
                    .spawn((
                        Node {
                            width: px(38),
                            height: px(38),
                            padding: UiRect::all(px(8)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(accent()),
                    ))
                    .with_child((
                        ImageNode::new(asset_server.load("editor/icons/box.png"))
                            .with_color(Color::WHITE),
                        Node {
                            width: percent(100),
                            height: percent(100),
                            ..default()
                        },
                    ));
                spawn_text(header, "REVY", 22.0, text_primary(), &font);
                spawn_text(header, "ENGINE", 10.0, text_muted(), &font);
                header.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                header
                    .spawn((
                        Node {
                            height: percent(100),
                            min_width: px(130),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            padding: UiRect::horizontal(px(18)),
                            border: UiRect::bottom(px(3)),
                            ..default()
                        },
                        BorderColor::all(accent()),
                    ))
                    .with_children(|tab| {
                        spawn_text(tab, "Projects", 15.0, accent_soft(), &font);
                    });
                header.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                spawn_text(header, "Revy 0.1.0", 11.0, text_muted(), &font);
            });

            root.spawn((
                Node {
                    width: percent(100),
                    height: px(60),
                    min_height: px(60),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(16)),
                    column_gap: px(8),
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BackgroundColor(bg_toolbar()),
                BorderColor::all(border()),
            ))
            .with_children(|toolbar| {
                spawn_action_button(
                    toolbar,
                    ManagerAction::NewProject,
                    "editor/icons/plus.png",
                    "New Project",
                    true,
                    &asset_server,
                    &font,
                );
                spawn_action_button(
                    toolbar,
                    ManagerAction::ImportProject,
                    "editor/icons/folder-open.png",
                    "Import",
                    false,
                    &asset_server,
                    &font,
                );
                spawn_icon_button(
                    toolbar,
                    ManagerAction::Refresh,
                    "editor/icons/redo-2.png",
                    &asset_server,
                );
                toolbar.spawn(Node {
                    min_width: px(0),
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    ..default()
                });
                toolbar
                    .spawn((
                        Node {
                            width: px(390),
                            min_width: px(190),
                            max_width: percent(50),
                            flex_shrink: 1.0,
                            height: px(34),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(10)),
                            column_gap: px(8),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        BackgroundColor(bg_field()),
                        BorderColor::all(border_soft()),
                    ))
                    .with_children(|search| {
                        search.spawn((
                            ImageNode::new(asset_server.load("editor/icons/search.png"))
                                .with_color(text_muted()),
                            Node {
                                width: px(16),
                                height: px(16),
                                ..default()
                            },
                        ));
                        spawn_text(search, "Search", 11.5, text_disabled(), &font);
                        search.spawn((
                            SearchInput,
                            EditableText::default(),
                            TextCursorStyle::default(),
                            text_font(&font, 12.0),
                            TextColor(text_primary()),
                            Node {
                                min_width: px(0),
                                flex_grow: 1.0,
                                height: percent(100),
                                align_items: AlignItems::Center,
                                ..default()
                            },
                        ));
                    });
                toolbar
                    .spawn((
                        Button,
                        WidgetButton,
                        ManagerAction::ToggleSort,
                        button_chrome(false),
                        Node {
                            height: px(34),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(12)),
                            column_gap: px(8),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        BackgroundColor(bg_panel()),
                        BorderColor::all(border_soft()),
                    ))
                    .with_children(|sort| {
                        sort.spawn((
                            SortLabel,
                            Text::new("Last Opened"),
                            text_font(&font, 11.5),
                            TextColor(text_primary()),
                        ));
                        sort.spawn((
                            ImageNode::new(asset_server.load("editor/icons/chevron-down.png"))
                                .with_color(text_muted()),
                            Node {
                                width: px(14),
                                height: px(14),
                                ..default()
                            },
                        ));
                    });
            });

            root.spawn(Node {
                width: percent(100),
                min_height: px(0),
                flex_grow: 1.0,
                flex_basis: px(0),
                flex_direction: FlexDirection::Row,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|content| {
                content.spawn((
                    ProjectListHost,
                    Node {
                        width: px(0),
                        min_width: px(0),
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_basis: px(0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::axes(px(10), px(8)),
                        row_gap: px(2),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BackgroundColor(bg_list()),
                ));
                content.spawn((
                    DetailsHost,
                    Node {
                        width: px(292),
                        min_width: px(250),
                        max_width: px(340),
                        flex_shrink: 0.0,
                        min_height: px(0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(16)),
                        row_gap: px(10),
                        border: UiRect::left(px(1)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(bg_sidebar()),
                    BorderColor::all(border()),
                ));
            });

            root.spawn((
                Node {
                    width: percent(100),
                    height: px(30),
                    min_height: px(30),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(12)),
                    column_gap: px(8),
                    border: UiRect::top(px(1)),
                    ..default()
                },
                BackgroundColor(bg_header()),
                BorderColor::all(border()),
            ))
            .with_children(|status| {
                status.spawn((
                    Node {
                        width: px(7),
                        height: px(7),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(success()),
                ));
                status.spawn((
                    ManagerStatusText,
                    Text::new("Ready"),
                    text_font(&font, 10.5),
                    TextColor(text_muted()),
                ));
                status.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                spawn_text(
                    status,
                    "Double-click a project to open",
                    10.0,
                    text_disabled(),
                    &font,
                );
            });
        });

    commands.spawn((
        DialogHost,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        GlobalZIndex(1000),
        Pickable::IGNORE,
    ));
}

fn rebuild_project_list(
    mut commands: Commands,
    state: Res<ManagerState>,
    font: Res<ManagerFont>,
    list: Query<Entity, With<ProjectListHost>>,
    mut images: ResMut<Assets<Image>>,
    mut thumbnails: ResMut<ThumbnailCache>,
    mut last_revision: Local<u64>,
) {
    if state.revision == *last_revision {
        return;
    }
    let Ok(host) = list.single() else {
        return;
    };
    *last_revision = state.revision;
    commands.entity(host).despawn_related::<Children>();

    let filter = state.filter.trim().to_lowercase();
    let mut visible: Vec<_> = state
        .recents
        .projects
        .iter()
        .filter(|project| {
            filter.is_empty()
                || project.name.to_lowercase().contains(&filter)
                || project
                    .path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&filter)
        })
        .cloned()
        .collect();
    match state.sort {
        ProjectSort::LastOpened => visible.sort_by(|left, right| {
            right
                .last_opened
                .cmp(&left.last_opened)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        }),
        ProjectSort::Name => visible.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        }),
    }

    if visible.is_empty() {
        commands
            .spawn((
                ChildOf(host),
                Node {
                    width: percent(100),
                    min_height: px(230),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    ..default()
                },
            ))
            .with_children(|empty| {
                let message = if state.recents.projects.is_empty() {
                    "No projects yet"
                } else {
                    "No matching projects"
                };
                spawn_text(empty, message, 18.0, text_primary(), &font);
                spawn_text(
                    empty,
                    "Use New Project or Import in the toolbar",
                    11.5,
                    text_muted(),
                    &font,
                );
            });
        return;
    }

    for project in visible {
        let row = commands
            .spawn((
                ChildOf(host),
                Button,
                ProjectRow(project.path.clone()),
                Node {
                    width: percent(100),
                    height: px(92),
                    min_height: px(92),
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(12), px(10)),
                    column_gap: px(14),
                    border: UiRect {
                        left: px(3),
                        right: px(0),
                        top: px(0),
                        bottom: px(1),
                    },
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(border()),
            ))
            .id();

        commands.entity(row).with_children(|row_ui| {
            row_ui
                .spawn((
                    Node {
                        width: px(62),
                        height: px(62),
                        min_width: px(62),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(thumbnail_bg(&project.name)),
                    BorderColor::all(border_soft()),
                ))
                .with_children(|thumbnail| {
                    if let Some(handle) = project
                        .icon
                        .as_ref()
                        .and_then(|path| load_thumbnail(path, &mut images, &mut thumbnails))
                    {
                        thumbnail.spawn((
                            ImageNode::new(handle),
                            Node {
                                width: percent(100),
                                height: percent(100),
                                ..default()
                            },
                        ));
                    } else {
                        let initial = project
                            .name
                            .chars()
                            .next()
                            .map(|character| character.to_string())
                            .unwrap_or_else(|| "A".into());
                        spawn_text(thumbnail, initial, 26.0, Color::WHITE, &font);
                    }
                });
            row_ui
                .spawn(Node {
                    width: px(0),
                    min_width: px(0),
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    row_gap: px(6),
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|info| {
                    spawn_text(info, project.name.clone(), 16.0, text_primary(), &font);
                    spawn_text(
                        info,
                        project.path.to_string_lossy(),
                        11.5,
                        if project.is_missing() {
                            danger()
                        } else {
                            text_muted()
                        },
                        &font,
                    );
                    spawn_text(
                        info,
                        if project.is_valid() {
                            "Rust project  |  Format 1"
                        } else {
                            "Project needs attention"
                        },
                        10.0,
                        if project.is_valid() {
                            text_disabled()
                        } else {
                            warning()
                        },
                        &font,
                    );
                });
            row_ui
                .spawn(Node {
                    width: px(108),
                    min_width: px(108),
                    flex_shrink: 0.0,
                    align_items: AlignItems::FlexEnd,
                    justify_content: JustifyContent::Center,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(5),
                    ..default()
                })
                .with_children(|time| {
                    spawn_text(time, "LAST OPENED", 9.0, text_disabled(), &font);
                    spawn_text(
                        time,
                        relative_time(project.last_opened),
                        11.0,
                        text_muted(),
                        &font,
                    );
                });
        });
        commands.entity(row).observe(handle_project_click);
    }
}

fn rebuild_details_panel(
    mut commands: Commands,
    state: Res<ManagerState>,
    font: Res<ManagerFont>,
    asset_server: Res<AssetServer>,
    hosts: Query<Entity, With<DetailsHost>>,
    mut last_revision: Local<u64>,
) {
    if state.revision == *last_revision {
        return;
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    *last_revision = state.revision;
    commands.entity(host).despawn_related::<Children>();

    let Some(project) = state.selected_project().cloned() else {
        commands.entity(host).with_children(|details| {
            spawn_text(details, "PROJECT", 9.5, text_disabled(), &font);
            spawn_text(details, "Nothing selected", 16.0, text_primary(), &font);
            spawn_text(
                details,
                "Select a project from the list.",
                11.5,
                text_muted(),
                &font,
            );
            details.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            spawn_action_button(
                details,
                ManagerAction::RemoveMissing,
                "editor/icons/x.png",
                "Remove Missing",
                false,
                &asset_server,
                &font,
            );
        });
        return;
    };

    commands.entity(host).with_children(|details| {
        spawn_text(details, "SELECTED PROJECT", 9.5, text_disabled(), &font);
        spawn_text(details, project.name.clone(), 18.0, text_primary(), &font);
        spawn_text(
            details,
            project.path.to_string_lossy(),
            11.0,
            text_muted(),
            &font,
        );
        details
            .spawn(Node {
                min_height: px(28),
                align_items: AlignItems::Center,
                column_gap: px(7),
                ..default()
            })
            .with_children(|validity| {
                validity.spawn((
                    Node {
                        width: px(8),
                        height: px(8),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(if project.is_valid() {
                        success()
                    } else {
                        danger()
                    }),
                ));
                spawn_text(
                    validity,
                    if project.is_valid() {
                        "Ready to open"
                    } else {
                        "Cannot open"
                    },
                    11.0,
                    if project.is_valid() {
                        success()
                    } else {
                        danger()
                    },
                    &font,
                );
            });
        if let Some(error) = &project.validation_error {
            spawn_text(details, error.clone(), 10.5, warning(), &font);
        }
        details.spawn(Node {
            height: px(4),
            ..default()
        });
        spawn_action_button(
            details,
            ManagerAction::OpenSelected,
            "editor/icons/play.png",
            "Open Project",
            true,
            &asset_server,
            &font,
        );
        spawn_action_button(
            details,
            ManagerAction::ShowSelectedFolder,
            "editor/icons/folder-open.png",
            "Show in Explorer",
            false,
            &asset_server,
            &font,
        );
        details.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        details.spawn((
            Node {
                width: percent(100),
                height: px(1),
                margin: UiRect::vertical(px(4)),
                ..default()
            },
            BackgroundColor(border()),
        ));
        spawn_action_button(
            details,
            ManagerAction::RemoveSelected,
            "editor/icons/x.png",
            "Remove from List",
            false,
            &asset_server,
            &font,
        );
        spawn_text(
            details,
            "Project files remain on disk",
            9.5,
            text_disabled(),
            &font,
        );
        spawn_action_button(
            details,
            ManagerAction::RemoveMissing,
            "editor/icons/x.png",
            "Remove Missing",
            false,
            &asset_server,
            &font,
        );
    });
}

fn rebuild_dialog(
    mut commands: Commands,
    dialog: Res<DialogState>,
    font: Res<ManagerFont>,
    asset_server: Res<AssetServer>,
    hosts: Query<Entity, With<DialogHost>>,
    mut last_revision: Local<u64>,
) {
    if dialog.revision == *last_revision {
        return;
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    *last_revision = dialog.revision;
    commands.entity(host).despawn_related::<Children>();
    if dialog.kind == DialogKind::None {
        commands.entity(host).insert(Pickable::IGNORE);
        return;
    }
    commands.entity(host).insert(Pickable::default());

    commands.entity(host).with_children(|overlay| {
        overlay
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(0),
                    bottom: px(0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::all(px(20)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.015, 0.018, 0.022, 0.82)),
            ))
            .with_children(|center| {
                let height = match dialog.kind {
                    DialogKind::Create => 440.0,
                    DialogKind::Import => 340.0,
                    DialogKind::Error => 300.0,
                    DialogKind::None => 0.0,
                };
                center
                    .spawn((
                        Node {
                            width: px(610),
                            max_width: percent(96),
                            height: px(height),
                            max_height: percent(96),
                            flex_direction: FlexDirection::Column,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(bg_modal()),
                        BorderColor::all(border_soft()),
                    ))
                    .with_children(|panel| {
                        panel
                            .spawn((
                                Node {
                                    width: percent(100),
                                    height: px(62),
                                    min_height: px(62),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(px(20)),
                                    border: UiRect::bottom(px(1)),
                                    ..default()
                                },
                                BackgroundColor(bg_toolbar()),
                                BorderColor::all(border()),
                            ))
                            .with_children(|header| {
                                spawn_text(
                                    header,
                                    match dialog.kind {
                                        DialogKind::Create => "Create New Project",
                                        DialogKind::Import => "Import Existing Project",
                                        DialogKind::Error => dialog.error_title.as_str(),
                                        DialogKind::None => "",
                                    },
                                    17.0,
                                    text_primary(),
                                    &font,
                                );
                                header.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                spawn_icon_button(
                                    header,
                                    ManagerAction::CancelDialog,
                                    "editor/icons/x.png",
                                    &asset_server,
                                );
                            });

                        panel
                            .spawn(Node {
                                width: percent(100),
                                min_height: px(0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(20)),
                                row_gap: px(10),
                                ..default()
                            })
                            .with_children(|body| match dialog.kind {
                                DialogKind::Create => {
                                    spawn_create_dialog_body(body, &dialog, &font, &asset_server)
                                }
                                DialogKind::Import => {
                                    spawn_import_dialog_body(body, &dialog, &font, &asset_server)
                                }
                                DialogKind::Error => spawn_error_dialog_body(body, &dialog, &font),
                                DialogKind::None => {}
                            });

                        panel
                            .spawn((
                                Node {
                                    width: percent(100),
                                    height: px(70),
                                    min_height: px(70),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(px(20)),
                                    column_gap: px(8),
                                    border: UiRect::top(px(1)),
                                    ..default()
                                },
                                BackgroundColor(bg_toolbar()),
                                BorderColor::all(border()),
                            ))
                            .with_children(|footer| {
                                if dialog.kind == DialogKind::Error {
                                    footer.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    spawn_text_button(
                                        footer,
                                        ManagerAction::CancelDialog,
                                        "Close",
                                        true,
                                        &font,
                                    );
                                    return;
                                }
                                footer.spawn((
                                    DialogValidationText,
                                    Text::new(""),
                                    text_font(&font, 10.5),
                                    TextColor(text_muted()),
                                    Node {
                                        min_width: px(0),
                                        flex_grow: 1.0,
                                        ..default()
                                    },
                                ));
                                spawn_text_button(
                                    footer,
                                    ManagerAction::CancelDialog,
                                    "Cancel",
                                    false,
                                    &font,
                                );
                                spawn_action_button(
                                    footer,
                                    if dialog.kind == DialogKind::Create {
                                        ManagerAction::ConfirmCreate
                                    } else {
                                        ManagerAction::ConfirmImport
                                    },
                                    if dialog.kind == DialogKind::Create {
                                        "editor/icons/plus.png"
                                    } else {
                                        "editor/icons/folder-open.png"
                                    },
                                    if dialog.kind == DialogKind::Create {
                                        "Create & Edit"
                                    } else {
                                        "Import & Edit"
                                    },
                                    true,
                                    &asset_server,
                                    &font,
                                );
                            });
                    });
            });
    });
}

fn spawn_create_dialog_body(
    body: &mut ChildSpawnerCommands,
    dialog: &DialogState,
    font: &ManagerFont,
    asset_server: &AssetServer,
) {
    spawn_field_label(body, "PROJECT NAME", font);
    spawn_text_input(body, CreateNameInput, &dialog.project_name, font);
    body.spawn(Node {
        height: px(4),
        ..default()
    });
    spawn_field_label(body, "SAVE LOCATION", font);
    body.spawn(Node {
        width: percent(100),
        height: px(38),
        align_items: AlignItems::Center,
        column_gap: px(8),
        ..default()
    })
    .with_children(|path_row| {
        spawn_text_input(path_row, CreateParentInput, &dialog.create_parent, font);
        spawn_action_button(
            path_row,
            ManagerAction::BrowseCreateParent,
            "editor/icons/folder-open.png",
            "Browse",
            false,
            asset_server,
            font,
        );
    });
    spawn_text(body, "Project folder", 9.5, text_disabled(), font);
    body.spawn((
        CreateTargetText,
        Text::new(""),
        text_font(font, 10.5),
        TextColor(text_muted()),
    ));
}

fn spawn_import_dialog_body(
    body: &mut ChildSpawnerCommands,
    dialog: &DialogState,
    font: &ManagerFont,
    asset_server: &AssetServer,
) {
    spawn_field_label(body, "PROJECT FOLDER", font);
    body.spawn(Node {
        width: percent(100),
        height: px(38),
        align_items: AlignItems::Center,
        column_gap: px(8),
        ..default()
    })
    .with_children(|path_row| {
        spawn_text_input(path_row, ImportPathInput, &dialog.import_path, font);
        spawn_action_button(
            path_row,
            ManagerAction::BrowseImportPath,
            "editor/icons/folder-open.png",
            "Browse",
            false,
            asset_server,
            font,
        );
    });
    spawn_text(
        body,
        "Select a folder containing project.toml",
        10.5,
        text_muted(),
        font,
    );
}

fn spawn_error_dialog_body(
    body: &mut ChildSpawnerCommands,
    dialog: &DialogState,
    font: &ManagerFont,
) {
    body.spawn((
        Node {
            width: percent(100),
            padding: UiRect::all(px(14)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.28, 0.09, 0.10, 0.45)),
        BorderColor::all(danger()),
    ))
    .with_children(|message| {
        spawn_text(
            message,
            dialog.error_message.clone(),
            12.0,
            text_primary(),
            font,
        );
    });
    spawn_text(
        body,
        "Select a Revy project created by this editor and try again.",
        10.5,
        text_muted(),
        font,
    );
}

fn handle_manager_action(
    activate: On<Activate>,
    actions: Query<&ManagerAction>,
    mut state: ResMut<ManagerState>,
    mut dialog: ResMut<DialogState>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    match action {
        ManagerAction::NewProject => dialog.show_create(),
        ManagerAction::ImportProject => {
            dialog.show_import();
            let initial = default_project_parent();
            if let Some(path) = pick_folder("Import a Revy project", &initial) {
                dialog.import_path = path.to_string_lossy().into_owned();
                import_and_launch(&path, &mut state, &mut dialog, &mut exit);
            } else {
                dialog.close();
            }
        }
        ManagerAction::Refresh => {
            state.recents.refresh();
            state.status = format!("Refreshed {} project(s)", state.recents.projects.len());
            state.bump();
            state.persist();
        }
        ManagerAction::ToggleSort => {
            state.sort = match state.sort {
                ProjectSort::LastOpened => ProjectSort::Name,
                ProjectSort::Name => ProjectSort::LastOpened,
            };
            state.bump();
        }
        ManagerAction::OpenSelected => {
            if let Some(path) = state.selected.clone() {
                launch_project(&path, &mut state, &mut dialog, &mut exit);
            }
        }
        ManagerAction::ShowSelectedFolder => {
            if let Some(path) = state.selected.clone() {
                match show_in_file_manager(&path) {
                    Ok(()) => state.status = format!("Opened {}", path.display()),
                    Err(error) => {
                        state.status = error.clone();
                        dialog.show_error("Could Not Open Folder", error);
                    }
                }
                state.bump();
            }
        }
        ManagerAction::RemoveSelected => {
            if let Some(path) = state.selected.take() {
                state.recents.remove(&path);
                state.selected = state
                    .recents
                    .projects
                    .first()
                    .map(|project| project.path.clone());
                state.status = "Removed project from the list; files were not changed".into();
                state.bump();
                state.persist();
            }
        }
        ManagerAction::RemoveMissing => remove_missing_records(&mut state),
        ManagerAction::BrowseCreateParent => {
            let initial = PathBuf::from(&dialog.create_parent);
            if let Some(path) = pick_folder("Choose where to create the project", &initial) {
                dialog.create_parent = path.to_string_lossy().into_owned();
                dialog.revision = dialog.revision.wrapping_add(1);
            }
        }
        ManagerAction::BrowseImportPath => {
            let initial = PathBuf::from(&dialog.import_path);
            if let Some(path) = pick_folder("Choose a Revy project", &initial) {
                dialog.import_path = path.to_string_lossy().into_owned();
                dialog.revision = dialog.revision.wrapping_add(1);
            }
        }
        ManagerAction::ConfirmCreate => {
            let parent = PathBuf::from(dialog.create_parent.trim());
            match create_project(&dialog.project_name, &parent) {
                Ok(path) => {
                    dialog.close();
                    launch_project(&path, &mut state, &mut dialog, &mut exit);
                }
                Err(error) => {
                    state.status = error.clone();
                    state.bump();
                    dialog.show_error("Project Creation Failed", error);
                }
            }
        }
        ManagerAction::ConfirmImport => {
            let path = PathBuf::from(dialog.import_path.trim());
            import_and_launch(&path, &mut state, &mut dialog, &mut exit);
        }
        ManagerAction::CancelDialog => dialog.close(),
    }
}

fn remove_missing_records(state: &mut ManagerState) {
    let removed = state.recents.remove_missing();
    if state.selected.as_ref().is_some_and(|selected| {
        !state
            .recents
            .projects
            .iter()
            .any(|project| &project.path == selected)
    }) {
        state.selected = state
            .recents
            .projects
            .first()
            .map(|project| project.path.clone());
    }
    state.status = if removed == 0 {
        "No missing project records".into()
    } else {
        format!("Removed {removed} missing project record(s)")
    };
    state.bump();
    state.persist();
}

fn import_and_launch(
    path: &std::path::Path,
    state: &mut ManagerState,
    dialog: &mut DialogState,
    exit: &mut MessageWriter<AppExit>,
) {
    match validate_project(path) {
        Ok(_) => {
            dialog.close();
            launch_project(path, state, dialog, exit);
        }
        Err(error) => {
            let message = format!("{}\n\n{}", path.display(), error);
            state.status = format!("Import failed: {error}");
            state.bump();
            dialog.show_error("Import Failed", message);
        }
    }
}

fn handle_project_click(
    click: On<Pointer<Click>>,
    rows: Query<&ProjectRow>,
    mut state: ResMut<ManagerState>,
    mut dialog: ResMut<DialogState>,
    mut exit: MessageWriter<AppExit>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    state.selected = Some(row.0.clone());
    state.status = format!("Selected {}", row.0.display());
    state.bump();
    if click.count >= 2 {
        launch_project(&row.0, &mut state, &mut dialog, &mut exit);
    }
}

fn launch_project(
    path: &std::path::Path,
    state: &mut ManagerState,
    dialog: &mut DialogState,
    exit: &mut MessageWriter<AppExit>,
) {
    let canonical = match state.recents.add_or_update(path, now_epoch_seconds()) {
        Ok(path) => path,
        Err(error) => {
            state.status = error.clone();
            state.bump();
            dialog.show_error("Could Not Open Project", error);
            return;
        }
    };
    state.selected = Some(canonical.clone());
    state.persist();

    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            state.status = format!("Could not locate the editor executable: {error}");
            state.bump();
            dialog.show_error("Could Not Open Project", state.status.clone());
            return;
        }
    };
    match Command::new(executable)
        .arg("--project")
        .arg(&canonical)
        .spawn()
    {
        Ok(_) => {
            exit.write(AppExit::Success);
        }
        Err(error) => {
            state.status = format!("Could not open project: {error}");
            state.bump();
            dialog.show_error("Could Not Open Project", state.status.clone());
        }
    }
}

fn sync_search(
    inputs: Query<&EditableText, (With<SearchInput>, Changed<EditableText>)>,
    mut state: ResMut<ManagerState>,
) {
    for input in &inputs {
        let filter = input.value().to_string();
        if filter != state.filter {
            state.filter = filter;
            state.bump();
        }
    }
}

fn sync_dialog_inputs(
    names: Query<
        &EditableText,
        (
            With<CreateNameInput>,
            Without<CreateParentInput>,
            Without<ImportPathInput>,
            Changed<EditableText>,
        ),
    >,
    parents: Query<
        &EditableText,
        (
            With<CreateParentInput>,
            Without<CreateNameInput>,
            Without<ImportPathInput>,
            Changed<EditableText>,
        ),
    >,
    imports: Query<
        &EditableText,
        (
            With<ImportPathInput>,
            Without<CreateNameInput>,
            Without<CreateParentInput>,
            Changed<EditableText>,
        ),
    >,
    mut dialog: ResMut<DialogState>,
) {
    for input in &names {
        dialog.project_name = input.value().to_string();
    }
    for input in &parents {
        dialog.create_parent = input.value().to_string();
    }
    for input in &imports {
        dialog.import_path = input.value().to_string();
    }
}

fn sync_dialog_validation(
    dialog: Res<DialogState>,
    mut validation: Query<(&mut Text, &mut TextColor), With<DialogValidationText>>,
    mut target_labels: Query<&mut Text, (With<CreateTargetText>, Without<DialogValidationText>)>,
) {
    let result = match dialog.kind {
        DialogKind::Create => {
            let parent = PathBuf::from(dialog.create_parent.trim());
            for mut label in &mut target_labels {
                label.0 = create_target_path(&dialog.project_name, &parent)
                    .to_string_lossy()
                    .into_owned();
            }
            create_validation(&dialog.project_name, &parent).map(|_| "Ready to create".to_string())
        }
        DialogKind::Import => validate_project(&PathBuf::from(dialog.import_path.trim()))
            .map(|project| format!("Found project: {}", project.name)),
        DialogKind::Error => return,
        DialogKind::None => return,
    };
    for (mut label, mut color) in &mut validation {
        match &result {
            Ok(message) => {
                label.0 = message.clone();
                color.0 = success();
            }
            Err(error) => {
                label.0 = error.clone();
                color.0 = warning();
            }
        }
    }
}

fn handle_dialog_escape(keys: Res<ButtonInput<KeyCode>>, mut dialog: ResMut<DialogState>) {
    if dialog.kind != DialogKind::None && keys.just_pressed(KeyCode::Escape) {
        dialog.close();
    }
}

fn sync_project_row_chrome(
    state: Res<ManagerState>,
    mut rows: Query<(
        &ProjectRow,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (row, interaction, mut background, mut border_color) in &mut rows {
        let selected = state.selected.as_ref() == Some(&row.0);
        background.0 = match interaction {
            Interaction::Pressed => bg_selected_pressed(),
            Interaction::Hovered if selected => bg_selected_hovered(),
            Interaction::Hovered => bg_row_hovered(),
            Interaction::None if selected => bg_selected(),
            Interaction::None => Color::NONE,
        };
        *border_color = BorderColor::all(if selected { accent() } else { border() });
    }
}

fn sync_button_chrome(
    mut buttons: Query<
        (
            &Interaction,
            &ButtonChrome,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Changed<Interaction>,
    >,
) {
    for (interaction, chrome, mut background, mut border_color) in &mut buttons {
        background.0 = match interaction {
            Interaction::Pressed => chrome.pressed,
            Interaction::Hovered => chrome.hovered,
            Interaction::None => chrome.normal,
        };
        *border_color = BorderColor::all(chrome.border);
    }
}

fn sync_status_text(
    state: Res<ManagerState>,
    mut labels: Query<&mut Text, With<ManagerStatusText>>,
) {
    if !state.is_changed() {
        return;
    }
    for mut label in &mut labels {
        label.0 = state.status.clone();
    }
}

fn sync_sort_label(state: Res<ManagerState>, mut labels: Query<&mut Text, With<SortLabel>>) {
    if !state.is_changed() {
        return;
    }
    let value = match state.sort {
        ProjectSort::LastOpened => "Last Opened",
        ProjectSort::Name => "Name",
    };
    for mut label in &mut labels {
        label.0 = value.into();
    }
}

fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    action: ManagerAction,
    icon_path: &'static str,
    label: impl Into<String>,
    primary: bool,
    asset_server: &AssetServer,
    font: &ManagerFont,
) {
    let chrome = button_chrome(primary);
    parent
        .spawn((
            Button,
            WidgetButton,
            action,
            chrome,
            Node {
                height: px(34),
                min_width: px(34),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(px(11)),
                column_gap: px(7),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(chrome.normal),
            BorderColor::all(chrome.border),
        ))
        .with_children(|button| {
            button.spawn((
                ImageNode::new(asset_server.load(icon_path)).with_color(if primary {
                    Color::WHITE
                } else {
                    text_primary()
                }),
                Node {
                    width: px(16),
                    height: px(16),
                    ..default()
                },
            ));
            spawn_text(
                button,
                label,
                11.5,
                if primary {
                    Color::WHITE
                } else {
                    text_primary()
                },
                font,
            );
        });
}

fn spawn_text_button(
    parent: &mut ChildSpawnerCommands,
    action: ManagerAction,
    label: impl Into<String>,
    primary: bool,
    font: &ManagerFont,
) {
    let chrome = button_chrome(primary);
    parent
        .spawn((
            Button,
            WidgetButton,
            action,
            chrome,
            Node {
                height: px(34),
                min_width: px(78),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(px(12)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(chrome.normal),
            BorderColor::all(chrome.border),
        ))
        .with_children(|button| {
            spawn_text(
                button,
                label,
                11.5,
                if primary {
                    Color::WHITE
                } else {
                    text_primary()
                },
                font,
            );
        });
}

fn spawn_icon_button(
    parent: &mut ChildSpawnerCommands,
    action: ManagerAction,
    icon_path: &'static str,
    asset_server: &AssetServer,
) {
    let chrome = button_chrome(false);
    parent
        .spawn((
            Button,
            WidgetButton,
            action,
            chrome,
            Node {
                width: px(34),
                height: px(34),
                min_width: px(34),
                flex_shrink: 0.0,
                padding: UiRect::all(px(8)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(chrome.normal),
            BorderColor::all(chrome.border),
        ))
        .with_child((
            ImageNode::new(asset_server.load(icon_path)).with_color(text_primary()),
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ));
}

fn spawn_text_input<M: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    initial: &str,
    font: &ManagerFont,
) {
    parent.spawn((
        marker,
        EditableText::new(initial),
        TextCursorStyle::default(),
        text_font(font, 12.0),
        TextColor(text_primary()),
        Node {
            min_width: px(0),
            height: px(38),
            flex_grow: 1.0,
            padding: UiRect::axes(px(10), px(8)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(bg_field()),
        BorderColor::all(border_soft()),
    ));
}

fn spawn_field_label(parent: &mut ChildSpawnerCommands, label: &str, font: &ManagerFont) {
    spawn_text(parent, label, 9.5, text_muted(), font);
}

fn spawn_text(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    size: f32,
    color: Color,
    font: &ManagerFont,
) {
    parent.spawn((Text::new(value), text_font(font, size), TextColor(color)));
}

fn text_font(font: &ManagerFont, size: f32) -> TextFont {
    let style = TextFont::from_font_size(FontSize::Px(size));
    match &font.0 {
        Some(handle) => style.with_font(handle.clone()),
        None => style,
    }
}

fn button_chrome(primary: bool) -> ButtonChrome {
    if primary {
        ButtonChrome {
            normal: accent(),
            hovered: accent_hovered(),
            pressed: accent_pressed(),
            border: accent_border(),
        }
    } else {
        ButtonChrome {
            normal: bg_button(),
            hovered: bg_button_hovered(),
            pressed: bg_button_pressed(),
            border: border_soft(),
        }
    }
}

fn load_thumbnail(
    path: &std::path::Path,
    images: &mut Assets<Image>,
    cache: &mut ThumbnailCache,
) -> Option<Handle<Image>> {
    if let Some(handle) = cache.0.get(path) {
        return Some(handle.clone());
    }
    let image_type = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => ImageType::Extension("png"),
        Some("jpg") | Some("jpeg") => ImageType::Extension("jpg"),
        _ => return None,
    };
    let bytes = fs::read(path).ok()?;
    let image = Image::from_buffer(
        &bytes,
        image_type,
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::default(),
    )
    .ok()?;
    let handle = images.add(image);
    cache.0.insert(path.to_path_buf(), handle.clone());
    Some(handle)
}

fn thumbnail_bg(name: &str) -> Color {
    let hash = name.bytes().fold(17_u32, |value, byte| {
        value.wrapping_mul(31) + u32::from(byte)
    });
    let colors = [
        Color::srgb(0.11, 0.45, 0.62),
        Color::srgb(0.22, 0.48, 0.38),
        Color::srgb(0.52, 0.31, 0.34),
        Color::srgb(0.42, 0.35, 0.56),
        Color::srgb(0.46, 0.41, 0.24),
    ];
    colors[(hash as usize) % colors.len()]
}

fn default_project_parent() -> PathBuf {
    if cfg!(debug_assertions) {
        paths::workspace_root().join("projects")
    } else {
        paths::executable_directory().join("projects")
    }
}

fn system_font_candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        return ["msyh.ttc", "segoeui.ttf", "arial.ttf"]
            .into_iter()
            .map(|name| root.join("Fonts").join(name))
            .collect();
    }
    #[cfg(target_os = "macos")]
    {
        return vec![
            PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
            PathBuf::from("/System/Library/Fonts/SFNS.ttf"),
        ];
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        vec![
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        ]
    }
}

#[cfg(target_os = "windows")]
fn pick_folder(title: &str, initial: &std::path::Path) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = $env:REVY_PICKER_TITLE
$dialog.ShowNewFolderButton = $true
if (Test-Path -LiteralPath $env:REVY_PICKER_INITIAL) {
    $dialog.SelectedPath = $env:REVY_PICKER_INITIAL
}
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    [Console]::Write($dialog.SelectedPath)
}
"#;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-Command", script])
        .env("REVY_PICKER_TITLE", title)
        .env("REVY_PICKER_INITIAL", initial)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!selected.is_empty()).then(|| PathBuf::from(selected))
}

#[cfg(target_os = "macos")]
fn pick_folder(_title: &str, _initial: &std::path::Path) -> Option<PathBuf> {
    let output = Command::new("osascript")
        .args([
            "-e",
            "POSIX path of (choose folder with prompt \"Choose a project folder\")",
        ])
        .output()
        .ok()?;
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!selected.is_empty()).then(|| PathBuf::from(selected))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn pick_folder(title: &str, initial: &std::path::Path) -> Option<PathBuf> {
    let output = Command::new("zenity")
        .args([
            "--file-selection",
            "--directory",
            "--title",
            title,
            "--filename",
        ])
        .arg(initial)
        .output()
        .ok()?;
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!selected.is_empty()).then(|| PathBuf::from(selected))
}

fn show_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Project folder is missing: {}", path.display()));
    }
    #[cfg(target_os = "windows")]
    let result = Command::new("explorer.exe").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(path).spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(path).spawn();
    result
        .map(|_| ())
        .map_err(|error| format!("Could not open the project folder: {error}"))
}

fn bg_app() -> Color {
    Color::srgb(0.047, 0.052, 0.059)
}

fn bg_header() -> Color {
    Color::srgb(0.064, 0.070, 0.078)
}

fn bg_toolbar() -> Color {
    Color::srgb(0.082, 0.089, 0.098)
}

fn bg_list() -> Color {
    Color::srgb(0.060, 0.066, 0.074)
}

fn bg_sidebar() -> Color {
    Color::srgb(0.073, 0.080, 0.089)
}

fn bg_panel() -> Color {
    Color::srgb(0.105, 0.113, 0.124)
}

fn bg_modal() -> Color {
    Color::srgb(0.074, 0.081, 0.091)
}

fn bg_field() -> Color {
    Color::srgb(0.044, 0.050, 0.057)
}

fn bg_button() -> Color {
    Color::srgb(0.123, 0.133, 0.145)
}

fn bg_button_hovered() -> Color {
    Color::srgb(0.158, 0.170, 0.185)
}

fn bg_button_pressed() -> Color {
    Color::srgb(0.091, 0.099, 0.110)
}

fn bg_row_hovered() -> Color {
    Color::srgb(0.091, 0.102, 0.115)
}

fn bg_selected() -> Color {
    Color::srgb(0.077, 0.145, 0.183)
}

fn bg_selected_hovered() -> Color {
    Color::srgb(0.091, 0.170, 0.214)
}

fn bg_selected_pressed() -> Color {
    Color::srgb(0.064, 0.123, 0.158)
}

fn border() -> Color {
    Color::srgb(0.025, 0.029, 0.034)
}

fn border_soft() -> Color {
    Color::srgb(0.185, 0.198, 0.216)
}

fn accent() -> Color {
    Color::srgb(0.070, 0.520, 0.700)
}

fn accent_hovered() -> Color {
    Color::srgb(0.085, 0.600, 0.795)
}

fn accent_pressed() -> Color {
    Color::srgb(0.052, 0.410, 0.570)
}

fn accent_border() -> Color {
    Color::srgb(0.120, 0.650, 0.830)
}

fn accent_soft() -> Color {
    Color::srgb(0.350, 0.750, 0.930)
}

fn text_primary() -> Color {
    Color::srgb(0.900, 0.915, 0.935)
}

fn text_muted() -> Color {
    Color::srgb(0.590, 0.620, 0.660)
}

fn text_disabled() -> Color {
    Color::srgb(0.390, 0.420, 0.455)
}

fn success() -> Color {
    Color::srgb(0.300, 0.790, 0.500)
}

fn warning() -> Color {
    Color::srgb(0.940, 0.670, 0.260)
}

fn danger() -> Color {
    Color::srgb(0.920, 0.350, 0.390)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_project_activation_opens_create_dialog() {
        let mut app = App::new();
        app.insert_resource(ManagerState::new(RecentProjects::default()))
            .init_resource::<DialogState>()
            .add_observer(handle_manager_action);
        let button = app.world_mut().spawn(ManagerAction::NewProject).id();

        app.world_mut().trigger(Activate { entity: button });

        assert_eq!(
            app.world().resource::<DialogState>().kind,
            DialogKind::Create
        );
    }

    #[test]
    fn action_buttons_use_widget_button_component() {
        let mut world = World::new();
        let button = world
            .spawn((Button, WidgetButton, ManagerAction::NewProject))
            .id();

        assert!(world.entity(button).contains::<Button>());
        assert!(world.entity(button).contains::<WidgetButton>());
    }

    #[test]
    fn pointer_click_on_action_button_opens_create_dialog() {
        use std::time::Duration;

        use bevy::{
            camera::NormalizedRenderTarget,
            picking::{
                backend::HitData,
                events::{Click, Pointer, Press},
                pointer::{Location, PointerButton, PointerId},
            },
            ui_widgets::ButtonPlugin,
        };

        let mut app = App::new();
        app.insert_resource(ManagerState::new(RecentProjects::default()))
            .init_resource::<DialogState>()
            .add_plugins(ButtonPlugin)
            .add_observer(handle_manager_action);
        let button = app
            .world_mut()
            .spawn((Button, WidgetButton, ManagerAction::NewProject))
            .id();
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 100,
                height: 100,
            },
            position: Vec2::new(10.0, 10.0),
        };
        let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);

        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location.clone(),
            Press {
                button: PointerButton::Primary,
                hit: hit.clone(),
                count: 1,
            },
            button,
        ));
        app.world_mut().flush();
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location,
            Click {
                button: PointerButton::Primary,
                hit,
                duration: Duration::from_millis(10),
                count: 1,
            },
            button,
        ));
        app.world_mut().flush();

        assert_eq!(
            app.world().resource::<DialogState>().kind,
            DialogKind::Create
        );
    }
}
