//! Native, Godot-style entity picker window.

use std::collections::HashSet;

use bevy::{
    camera::{ClearColorConfig, RenderTarget},
    feathers::cursor::{EntityCursor, OverrideCursor},
    input_focus::AutoFocus,
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui::{InteractionDisabled, UiTargetCamera},
    ui_widgets::{
        Activate, Button as WidgetButton, ControlOrientation, ScrollArea, Scrollbar, ScrollbarThumb,
    },
    window::{
        EnabledButtons, MonitorSelection, SystemCursorIcon, WindowCloseRequested, WindowLevel,
        WindowPosition, WindowRef, WindowResizeConstraints, WindowResolution,
    },
};

use crate::{
    entities::EntityKind,
    hierarchy::{
        RootNodeKind, SceneEntityChoice, SceneEntityOption, SceneEntityPickerCancel,
        SceneEntityPickerCreate, SceneEntityPickerState, SceneRoot, SceneRootMenuState,
        SceneRootOption,
    },
    ui::theme,
};

#[derive(Component)]
struct EntityPickerWindow;

#[derive(Component)]
struct EntityPickerCamera;

#[derive(Component)]
struct EntityPickerUiRoot;

#[derive(Component)]
struct EntityPickerSearchInput;

#[derive(Component, Clone, Copy)]
struct EntityPickerChoiceRow {
    choice: SceneEntityChoice,
    group: Option<PickerGroup>,
}

#[derive(Component)]
struct EntityPickerDescriptionTitle;

#[derive(Component)]
struct EntityPickerDescriptionBody;

#[derive(Component)]
struct EntityPickerNoMatches;

#[derive(Component)]
struct EntityPickerMatchesPane;

#[derive(Component)]
struct EntityPickerDescriptionPane;

#[derive(Component)]
struct EntityPickerDescriptionSplitter;

#[derive(Component)]
struct EntityPickerDescriptionSplitterLine;

#[derive(Resource, Debug, Clone)]
struct EntityPickerLayout {
    matches_height: f32,
}

impl Default for EntityPickerLayout {
    fn default() -> Self {
        Self {
            matches_height: 300.0,
        }
    }
}

impl EntityPickerLayout {
    fn resize_matches(&mut self, delta_y: f32) {
        self.matches_height = (self.matches_height + delta_y).clamp(160.0, 600.0);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PickerGroup {
    Empty,
    TwoD,
    ThreeD,
    Ui,
}

impl PickerGroup {
    const ALL: [Self; 4] = [Self::Empty, Self::TwoD, Self::ThreeD, Self::Ui];

    const fn label(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::TwoD => "2D",
            Self::ThreeD => "3D",
            Self::Ui => "UI",
        }
    }
}

#[derive(Resource, Default)]
struct EntityPickerGroupState {
    collapsed: HashSet<PickerGroup>,
}

#[derive(Component, Clone, Copy)]
struct EntityPickerCategory(PickerGroup);

#[derive(Component, Clone, Copy)]
struct EntityPickerCategoryLabel(PickerGroup);

pub struct EntityPickerPlugin;

impl Plugin for EntityPickerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EntityPickerGroupState>()
            .init_resource::<EntityPickerLayout>()
            .add_observer(handle_picker_group_toggle)
            .add_systems(
                Update,
                (
                    handle_native_picker_close,
                    sync_native_picker_window,
                    apply_picker_layout,
                    sync_picker_search,
                    sync_picker_rows,
                    sync_picker_group_labels,
                    sync_picker_selection,
                    handle_picker_keyboard,
                )
                    .chain(),
            );
    }
}

fn handle_picker_group_toggle(
    activate: On<Activate>,
    categories: Query<&EntityPickerCategory>,
    mut groups: ResMut<EntityPickerGroupState>,
) {
    let Ok(category) = categories.get(activate.entity) else {
        return;
    };
    if !groups.collapsed.insert(category.0) {
        groups.collapsed.remove(&category.0);
    }
}

fn handle_native_picker_close(
    mut events: MessageReader<WindowCloseRequested>,
    windows: Query<(), With<EntityPickerWindow>>,
    mut menu: ResMut<SceneRootMenuState>,
) {
    if events.read().any(|event| windows.contains(event.window)) {
        menu.open = false;
    }
}

fn sync_native_picker_window(
    mut commands: Commands,
    menu: Res<SceneRootMenuState>,
    picker: Res<SceneEntityPickerState>,
    asset_server: Res<AssetServer>,
    mut groups: ResMut<EntityPickerGroupState>,
    windows: Query<Entity, With<EntityPickerWindow>>,
    cameras: Query<Entity, With<EntityPickerCamera>>,
    roots: Query<Entity, With<EntityPickerUiRoot>>,
) {
    if menu.open {
        if windows.is_empty() {
            // Start every dialog with all four entity groups expanded. The
            // collapse state is intentionally local to the current dialog so
            // a previous click cannot make a whole category appear empty.
            groups.collapsed.clear();
            spawn_entity_picker(&mut commands, &asset_server, &picker);
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

fn spawn_entity_picker(
    commands: &mut Commands,
    asset_server: &AssetServer,
    picker: &SceneEntityPickerState,
) {
    let window = commands
        .spawn((
            EntityPickerWindow,
            Window {
                title: "Create New Entity".into(),
                name: Some("arisna-entity-picker".into()),
                position: WindowPosition::Centered(MonitorSelection::Primary),
                // Keep the initial dialog inside a typical editor work area even when
                // Windows display scaling is enabled. The user can still resize it.
                resolution: WindowResolution::new(1120, 860),
                resize_constraints: WindowResizeConstraints {
                    min_width: 720.0,
                    min_height: 580.0,
                    ..default()
                },
                resizable: true,
                decorations: true,
                transparent: false,
                focused: true,
                window_level: WindowLevel::AlwaysOnTop,
                enabled_buttons: EnabledButtons {
                    minimize: false,
                    maximize: false,
                    close: true,
                },
                skip_taskbar: true,
                ..default()
            },
        ))
        .id();

    let camera = commands
        .spawn((
            EntityPickerCamera,
            Camera2d,
            Camera {
                clear_color: ClearColorConfig::Custom(dialog_background()),
                ..default()
            },
            RenderTarget::Window(WindowRef::Entity(window)),
        ))
        .id();

    let root = commands
        .spawn((
            EntityPickerUiRoot,
            UiTargetCamera(camera),
            Node {
                width: percent(100),
                height: percent(100),
                min_width: px(0),
                min_height: px(0),
                padding: UiRect::axes(px(14), px(16)),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(dialog_background()),
        ))
        .id();

    commands.entity(root).with_children(|dialog| {
        dialog
            .spawn(Node {
                width: percent(100),
                height: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_basis: px(0),
                flex_direction: FlexDirection::Row,
                column_gap: px(14),
                ..default()
            })
            .with_children(|content| {
                spawn_sidebar(content, picker, asset_server);
                spawn_main_picker(content, asset_server);
            });
        spawn_footer(dialog);
    });
}

fn spawn_sidebar(
    parent: &mut ChildSpawnerCommands,
    picker: &SceneEntityPickerState,
    asset_server: &AssetServer,
) {
    parent
        .spawn(Node {
            width: px(205),
            min_width: px(170),
            max_width: px(240),
            height: percent(100),
            flex_shrink: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            ..default()
        })
        .with_children(|sidebar| {
            spawn_text(sidebar, "Favorites:", 14.0, label_color());
            sidebar
                .spawn((
                    Node {
                        width: percent(100),
                        min_height: px(130),
                        flex_grow: 1.0,
                        padding: UiRect::all(px(5)),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::clip(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(sidebar_panel()),
                    BorderColor::all(panel_border()),
                ))
                .with_children(|_| {});

            spawn_text(sidebar, "Recent:", 14.0, label_color());
            sidebar
                .spawn((
                    Node {
                        width: percent(100),
                        min_height: px(170),
                        flex_grow: 1.0,
                        padding: UiRect::all(px(5)),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(1),
                        overflow: Overflow::scroll_y(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(sidebar_panel()),
                    BorderColor::all(panel_border()),
                ))
                .with_children(|recent| {
                    for kind in picker.recent.iter().copied() {
                        spawn_choice(
                            recent,
                            SceneEntityChoice::Entity(kind),
                            None,
                            0,
                            asset_server,
                        );
                    }
                });
        });
}

fn spawn_main_picker(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn(Node {
            min_width: px(0),
            height: percent(100),
            flex_grow: 1.0,
            flex_basis: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            ..default()
        })
        .with_children(|main| {
            spawn_text(main, "Search:", 14.0, label_color());
            main.spawn(Node {
                width: percent(100),
                height: px(42),
                min_height: px(42),
                position_type: PositionType::Relative,
                column_gap: px(7),
                ..default()
            })
            .with_children(|search_row| {
                search_row
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(51),
                            top: px(0),
                            bottom: px(0),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(10)),
                            border: UiRect::all(px(2)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(search_background()),
                        BorderColor::all(search_border()),
                    ))
                    .with_children(|field| {
                        field.spawn((
                            EntityPickerSearchInput,
                            AutoFocus,
                            EditableText::default(),
                            TextCursorStyle::default(),
                            TextFont {
                                font_size: FontSize::Px(13.0),
                                ..default()
                            },
                            TextColor(primary_text()),
                            Node {
                                min_width: px(0),
                                height: percent(100),
                                flex_grow: 1.0,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                        ));
                        field.spawn((
                            ImageNode::new(asset_server.load("editor/icons/search.png"))
                                .with_color(primary_text()),
                            Node {
                                width: px(20),
                                min_width: px(20),
                                height: px(20),
                                ..default()
                            },
                        ));
                    });
                search_row
                    .spawn((
                        Button,
                        WidgetButton,
                        Node {
                            position_type: PositionType::Absolute,
                            right: px(0),
                            top: px(0),
                            width: px(44),
                            min_width: px(44),
                            height: percent(100),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(button_background()),
                        BorderColor::all(panel_border()),
                    ))
                    .with_children(|favorite| {
                        favorite.spawn((
                            ImageNode::new(asset_server.load("editor/icons/star.png"))
                                .with_color(primary_text()),
                            Node {
                                width: px(22),
                                height: px(22),
                                ..default()
                            },
                        ));
                    });
            });

            main.spawn(Node {
                width: percent(100),
                height: px(32),
                min_height: px(32),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|header| {
                spawn_text(header, "Matches:", 14.0, label_color());
                spawn_text(header, "Filters  v", 13.0, label_color());
            });

            spawn_matches_pane(main, asset_server);
            spawn_description_splitter(main);
            spawn_text(main, "Description:", 14.0, label_color());
            main.spawn((
                EntityPickerDescriptionPane,
                ScrollArea,
                Node {
                    width: percent(100),
                    height: px(0),
                    min_height: px(110),
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    flex_shrink: 1.0,
                    padding: UiRect::all(px(11)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(9),
                    overflow: Overflow::scroll_y(),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(description_background()),
                BorderColor::all(panel_border()),
            ))
            .with_children(|description| {
                description.spawn((
                    EntityPickerDescriptionTitle,
                    Text::new("Select an entity type"),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(primary_text()),
                ));
                description.spawn((
                    EntityPickerDescriptionBody,
                    Text::new("Choose a match above to see its role and default components."),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(label_color()),
                    Node {
                        width: percent(100),
                        ..default()
                    },
                ));
            });
        });
}

fn spawn_matches_pane(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            EntityPickerMatchesPane,
            Node {
                width: percent(100),
                height: px(300),
                min_height: px(160),
                flex_grow: 0.0,
                flex_shrink: 1.0,
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(matches_background()),
            BorderColor::all(panel_border()),
        ))
        .with_children(|pane| {
            let scroll_target = pane
                .spawn((
                    ScrollArea,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        right: px(12),
                        top: px(0),
                        bottom: px(0),
                        padding: UiRect::all(px(6)),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(1),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|tree| spawn_match_tree(tree, asset_server))
                .id();

            pane.spawn((
                Scrollbar::new(scroll_target, ControlOrientation::Vertical, 28.0),
                Node {
                    position_type: PositionType::Absolute,
                    right: px(2),
                    top: px(4),
                    bottom: px(4),
                    width: px(8),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.11, 0.115, 0.13)),
            ))
            .with_children(|track| {
                track.spawn((
                    ScrollbarThumb {
                        border_radius: BorderRadius::all(px(4)),
                        border: UiRect::ZERO,
                    },
                    BackgroundColor(Color::srgb(0.42, 0.44, 0.49)),
                    EntityCursor::System(SystemCursorIcon::Pointer),
                ));
            });
        });
}

fn spawn_description_splitter(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EntityPickerDescriptionSplitter,
            Button,
            EntityCursor::System(SystemCursorIcon::NsResize),
            Node {
                width: percent(100),
                height: px(9),
                min_height: px(9),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|splitter| {
            splitter.spawn((
                EntityPickerDescriptionSplitterLine,
                Node {
                    width: percent(100),
                    height: px(1),
                    ..default()
                },
                BackgroundColor(panel_border()),
            ));
        })
        .observe(on_description_splitter_drag_start)
        .observe(on_description_splitter_drag)
        .observe(on_description_splitter_drag_end)
        .observe(on_description_splitter_over)
        .observe(on_description_splitter_out);
}

fn apply_picker_layout(
    layout: Res<EntityPickerLayout>,
    mut panes: Query<&mut Node, With<EntityPickerMatchesPane>>,
) {
    for mut node in &mut panes {
        node.height = px(layout.matches_height);
    }
}

fn on_description_splitter_drag_start(
    _drag: On<Pointer<DragStart>>,
    panes: Query<&ComputedNode, With<EntityPickerMatchesPane>>,
    mut layout: ResMut<EntityPickerLayout>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if let Some(pane) = panes.iter().next() {
        layout.matches_height = (pane.size().y * pane.inverse_scale_factor).clamp(160.0, 600.0);
    }
    cursor.0 = Some(EntityCursor::System(SystemCursorIcon::NsResize));
}

fn on_description_splitter_drag(drag: On<Pointer<Drag>>, mut layout: ResMut<EntityPickerLayout>) {
    layout.resize_matches(drag.delta.y);
}

fn on_description_splitter_drag_end(
    _drag: On<Pointer<DragEnd>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    cursor.0 = None;
}

fn on_description_splitter_over(
    over: On<Pointer<Over>>,
    splitters: Query<&Children, With<EntityPickerDescriptionSplitter>>,
    mut lines: Query<&mut BackgroundColor, With<EntityPickerDescriptionSplitterLine>>,
) {
    set_description_splitter_color(over.entity, theme::accent(), &splitters, &mut lines);
}

fn on_description_splitter_out(
    out: On<Pointer<Out>>,
    splitters: Query<&Children, With<EntityPickerDescriptionSplitter>>,
    mut lines: Query<&mut BackgroundColor, With<EntityPickerDescriptionSplitterLine>>,
) {
    set_description_splitter_color(out.entity, panel_border(), &splitters, &mut lines);
}

fn set_description_splitter_color(
    splitter: Entity,
    color: Color,
    splitters: &Query<&Children, With<EntityPickerDescriptionSplitter>>,
    lines: &mut Query<&mut BackgroundColor, With<EntityPickerDescriptionSplitterLine>>,
) {
    let Ok(children) = splitters.get(splitter) else {
        return;
    };
    for child in children {
        if let Ok(mut background) = lines.get_mut(*child) {
            background.0 = color;
        }
    }
}

fn spawn_match_tree(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    for group in PickerGroup::ALL {
        spawn_category(parent, group);
        match group {
            PickerGroup::Empty => {
                spawn_choice(
                    parent,
                    SceneEntityChoice::Entity(EntityKind::Empty),
                    Some(group),
                    1,
                    asset_server,
                );
                spawn_choice(
                    parent,
                    SceneEntityChoice::Entity(EntityKind::AnimationPlayer),
                    Some(group),
                    1,
                    asset_server,
                );
            }
            PickerGroup::TwoD => {
                spawn_choice(
                    parent,
                    SceneEntityChoice::Root(RootNodeKind::TwoD),
                    Some(group),
                    1,
                    asset_server,
                );
                for kind in [
                    EntityKind::Empty2D,
                    EntityKind::CollisionRect2D,
                    EntityKind::Sprite2D,
                    EntityKind::Camera2D,
                ] {
                    spawn_choice(
                        parent,
                        SceneEntityChoice::Entity(kind),
                        Some(group),
                        1,
                        asset_server,
                    );
                }
            }
            PickerGroup::ThreeD => {
                spawn_choice(
                    parent,
                    SceneEntityChoice::Root(RootNodeKind::ThreeD),
                    Some(group),
                    1,
                    asset_server,
                );
                for kind in [
                    EntityKind::Empty3D,
                    EntityKind::Mesh3D,
                    EntityKind::Camera3D,
                    EntityKind::DirectionalLight3D,
                    EntityKind::PointLight3D,
                    EntityKind::SpotLight3D,
                ] {
                    spawn_choice(
                        parent,
                        SceneEntityChoice::Entity(kind),
                        Some(group),
                        1,
                        asset_server,
                    );
                }
            }
            PickerGroup::Ui => {
                for kind in [
                    EntityKind::EmptyUi,
                    EntityKind::Panel,
                    EntityKind::Text,
                    EntityKind::Button,
                    EntityKind::Image,
                ] {
                    spawn_choice(
                        parent,
                        SceneEntityChoice::Entity(kind),
                        Some(group),
                        1,
                        asset_server,
                    );
                }
            }
        }
    }

    parent.spawn((
        EntityPickerNoMatches,
        Text::new("No entity types match the current search."),
        TextFont {
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(muted_text()),
        Node {
            display: Display::None,
            padding: UiRect::all(px(12)),
            ..default()
        },
    ));
}

fn spawn_category(parent: &mut ChildSpawnerCommands, group: PickerGroup) {
    parent.spawn((
        EntityPickerCategory(group),
        Button,
        WidgetButton,
        Node {
            width: percent(100),
            height: px(30),
            min_height: px(30),
            align_items: AlignItems::Center,
            padding: UiRect::left(px(8)),
            ..default()
        },
        BackgroundColor(category_background()),
        children![(
            EntityPickerCategoryLabel(group),
            Text::new(format!("v  {}", group.label())),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(label_color()),
        )],
    ));
}

fn spawn_choice(
    parent: &mut ChildSpawnerCommands,
    choice: SceneEntityChoice,
    group: Option<PickerGroup>,
    depth: usize,
    asset_server: &AssetServer,
) {
    let label = choice_label(choice);
    let mut entity = parent.spawn((
        EntityPickerChoiceRow { choice, group },
        Button,
        WidgetButton,
        Node {
            width: percent(100),
            height: px(31),
            min_height: px(31),
            align_items: AlignItems::Center,
            padding: UiRect::left(px(9.0 + depth as f32 * 18.0)),
            column_gap: px(9),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ));
    match choice {
        SceneEntityChoice::Root(kind) => {
            entity.insert(SceneRootOption(kind));
        }
        SceneEntityChoice::Entity(kind) => {
            entity.insert(SceneEntityOption(kind));
        }
    }
    entity
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(18),
                    min_width: px(18),
                    height: px(18),
                    ..default()
                },
                ImageNode::new(asset_server.load(choice_icon_kind(choice).icon_path()))
                    .with_color(choice_accent(choice)),
            ));
            spawn_text(row, label, 12.5, primary_text());
        })
        .observe(on_choice_over)
        .observe(on_choice_out);
}

fn spawn_footer(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            width: percent(100),
            height: px(52),
            min_height: px(52),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|footer| {
            footer.spawn(Node {
                width: px(219),
                min_width: px(184),
                max_width: px(254),
                flex_shrink: 1.0,
                ..default()
            });
            footer
                .spawn(Node {
                    min_width: px(0),
                    flex_grow: 1.0,
                    justify_content: JustifyContent::SpaceEvenly,
                    ..default()
                })
                .with_children(|buttons| {
                    spawn_footer_button(buttons, true);
                    spawn_footer_button(buttons, false);
                });
        });
}

fn spawn_footer_button(parent: &mut ChildSpawnerCommands, create: bool) {
    let mut button = parent.spawn((
        Button,
        WidgetButton,
        Node {
            width: px(128),
            min_width: px(108),
            height: px(42),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(button_background()),
        BorderColor::all(panel_border()),
    ));
    if create {
        button.insert((SceneEntityPickerCreate, InteractionDisabled));
    } else {
        button.insert(SceneEntityPickerCancel);
    }
    button.with_children(|content| {
        spawn_text(
            content,
            if create { "Create" } else { "Cancel" },
            13.0,
            primary_text(),
        );
    });
}

fn sync_picker_search(
    inputs: Query<&EditableText, (With<EntityPickerSearchInput>, Changed<EditableText>)>,
    mut picker: ResMut<SceneEntityPickerState>,
) {
    for input in &inputs {
        picker.search = input.value().to_string().trim().to_lowercase();
    }
}

fn sync_picker_rows(
    roots: Query<(), With<SceneRoot>>,
    picker: Res<SceneEntityPickerState>,
    groups: Res<EntityPickerGroupState>,
    mut choices: Query<(&EntityPickerChoiceRow, &mut Node), Without<EntityPickerCategory>>,
    mut categories: Query<
        (&EntityPickerCategory, &mut Node),
        (
            Without<EntityPickerChoiceRow>,
            Without<EntityPickerNoMatches>,
        ),
    >,
    mut no_matches: Query<
        &mut Node,
        (
            With<EntityPickerNoMatches>,
            Without<EntityPickerChoiceRow>,
            Without<EntityPickerCategory>,
        ),
    >,
) {
    let has_root = !roots.is_empty();
    let mut visible_choices = 0usize;
    let mut visible_groups = HashSet::new();
    for (row, mut node) in &mut choices {
        let compatible = picker_choice_is_available(has_root, row.choice);
        let group_matches = row
            .group
            .is_some_and(|group| group.label().to_lowercase().contains(&picker.search));
        let matches = picker.search.is_empty()
            || choice_label(row.choice)
                .to_lowercase()
                .contains(&picker.search)
            || group_matches;
        let collapsed = row
            .group
            .is_some_and(|group| groups.collapsed.contains(&group));
        let visible = compatible && matches && (!collapsed || !picker.search.is_empty());
        node.display = if visible {
            visible_choices += 1;
            if let Some(group) = row.group {
                visible_groups.insert(group);
            }
            Display::Flex
        } else {
            Display::None
        };
    }
    for (category, mut node) in &mut categories {
        let matches = picker.search.is_empty()
            || category.0.label().to_lowercase().contains(&picker.search)
            || visible_groups.contains(&category.0);
        node.display = if matches {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut no_matches {
        node.display = if !picker.search.is_empty() && visible_choices == 0 {
            Display::Flex
        } else {
            Display::None
        };
    }
}

const fn picker_choice_is_available(has_root: bool, choice: SceneEntityChoice) -> bool {
    matches!(
        (has_root, choice),
        (false, SceneEntityChoice::Root(_)) | (true, SceneEntityChoice::Entity(_))
    )
}

fn sync_picker_group_labels(
    groups: Res<EntityPickerGroupState>,
    mut labels: Query<(&EntityPickerCategoryLabel, &mut Text)>,
) {
    for (label, mut text) in &mut labels {
        let marker = if groups.collapsed.contains(&label.0) {
            ">"
        } else {
            "v"
        };
        text.0 = format!("{marker}  {}", label.0.label());
    }
}

fn sync_picker_selection(
    mut commands: Commands,
    picker: Res<SceneEntityPickerState>,
    mut rows: Query<
        (&EntityPickerChoiceRow, &mut BackgroundColor),
        Without<SceneEntityPickerCreate>,
    >,
    mut create_buttons: Query<
        (Entity, &mut BackgroundColor, Has<InteractionDisabled>),
        (
            With<SceneEntityPickerCreate>,
            Without<EntityPickerChoiceRow>,
        ),
    >,
    mut titles: Query<
        &mut Text,
        (
            With<EntityPickerDescriptionTitle>,
            Without<EntityPickerDescriptionBody>,
        ),
    >,
    mut bodies: Query<
        &mut Text,
        (
            With<EntityPickerDescriptionBody>,
            Without<EntityPickerDescriptionTitle>,
        ),
    >,
) {
    for (row, mut background) in &mut rows {
        background.0 = if picker.selected == Some(row.choice) {
            selected_background()
        } else {
            Color::NONE
        };
    }
    for (entity, mut background, disabled) in &mut create_buttons {
        let should_disable = picker.selected.is_none();
        background.0 = if should_disable {
            button_background()
        } else {
            create_background()
        };
        if should_disable && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !should_disable && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }

    let (title, body) = picker.selected.map(choice_description).unwrap_or((
        "Select an entity type",
        "Choose a match above to see its role and default components.",
    ));
    for mut text in &mut titles {
        text.0 = title.into();
    }
    for mut text in &mut bodies {
        text.0 = body.into();
    }
}

fn handle_picker_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    menu: Res<SceneRootMenuState>,
    picker: Res<SceneEntityPickerState>,
    create_buttons: Query<Entity, With<SceneEntityPickerCreate>>,
    mut commands: Commands,
) {
    if menu.open
        && picker.selected.is_some()
        && keyboard.just_pressed(KeyCode::Enter)
        && let Some(button) = create_buttons.iter().next()
    {
        commands.trigger(Activate { entity: button });
    }
}

fn on_choice_over(
    over: On<Pointer<Over>>,
    picker: Res<SceneEntityPickerState>,
    mut rows: Query<(&EntityPickerChoiceRow, &mut BackgroundColor)>,
) {
    if let Ok((row, mut background)) = rows.get_mut(over.entity)
        && picker.selected != Some(row.choice)
    {
        background.0 = hover_background();
    }
}

fn on_choice_out(
    out: On<Pointer<Out>>,
    picker: Res<SceneEntityPickerState>,
    mut rows: Query<(&EntityPickerChoiceRow, &mut BackgroundColor)>,
) {
    if let Ok((row, mut background)) = rows.get_mut(out.entity)
        && picker.selected != Some(row.choice)
    {
        background.0 = Color::NONE;
    }
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

fn choice_label(choice: SceneEntityChoice) -> &'static str {
    match choice {
        SceneEntityChoice::Root(RootNodeKind::TwoD) => "Empty2D Scene",
        SceneEntityChoice::Root(RootNodeKind::ThreeD) => "Empty3D Scene",
        SceneEntityChoice::Entity(kind) => kind.label(),
    }
}

fn choice_description(choice: SceneEntityChoice) -> (&'static str, &'static str) {
    match choice {
        SceneEntityChoice::Root(RootNodeKind::TwoD) => (
            "Class Empty2D Scene  <  Entity",
            "Creates a new 2D scene root. Child entities inherit the 2D scene space.",
        ),
        SceneEntityChoice::Root(RootNodeKind::ThreeD) => (
            "Class Empty3D Scene  <  Entity",
            "Creates a new 3D scene root. Child entities inherit the 3D scene space.",
        ),
        SceneEntityChoice::Entity(EntityKind::Empty) => (
            "Class Empty  <  Entity",
            "A minimal logical entity without spatial rendering components.",
        ),
        SceneEntityChoice::Entity(EntityKind::AnimationPlayer) => (
            "Class AnimationPlayer  <  Entity",
            "A scene-local animation library and playback controller targeting stable node IDs.",
        ),
        SceneEntityChoice::Entity(EntityKind::Empty2D) => (
            "Class Empty2D  <  Entity",
            "A transformable 2D entity used as a parent or organizational node.",
        ),
        SceneEntityChoice::Entity(EntityKind::CollisionRect2D) => (
            "Class CollisionRect2D  <  Empty2D",
            "A static axis-aligned rectangle used by the 2D collision solver.",
        ),
        SceneEntityChoice::Entity(EntityKind::Empty3D) => (
            "Class Empty3D  <  Entity",
            "A transformable 3D entity used as a parent or organizational node.",
        ),
        SceneEntityChoice::Entity(EntityKind::Sprite2D) => (
            "Class Sprite2D  <  Empty2D",
            "A 2D entity with Transform, Visibility, and Sprite components.",
        ),
        SceneEntityChoice::Entity(EntityKind::Camera2D) => (
            "Class Camera2D  <  Empty2D",
            "A camera entity that renders a 2D scene.",
        ),
        SceneEntityChoice::Entity(EntityKind::Mesh3D) => (
            "Class Mesh3D  <  Empty3D",
            "A 3D entity with Transform, Visibility, Mesh, and StandardMaterial components.",
        ),
        SceneEntityChoice::Entity(EntityKind::Camera3D) => (
            "Class Camera3D  <  Empty3D",
            "A perspective camera entity that renders a 3D scene.",
        ),
        SceneEntityChoice::Entity(EntityKind::DirectionalLight3D) => (
            "Class DirectionalLight3D  <  Empty3D",
            "A directional light suitable for sunlight and other distant sources.",
        ),
        SceneEntityChoice::Entity(EntityKind::PointLight3D) => (
            "Class PointLight3D  <  Empty3D",
            "A light that emits in every direction from a point in 3D space.",
        ),
        SceneEntityChoice::Entity(EntityKind::SpotLight3D) => (
            "Class SpotLight3D  <  Empty3D",
            "A cone-shaped light source for focused illumination.",
        ),
        SceneEntityChoice::Entity(EntityKind::EmptyUi) => (
            "Class EmptyUI  <  Entity",
            "A layout-only UI entity for grouping controls without drawing a visual element.",
        ),
        SceneEntityChoice::Entity(EntityKind::Panel) => (
            "Class Panel  <  EmptyUI",
            "A rectangular UI container with a solid background and layout data.",
        ),
        SceneEntityChoice::Entity(EntityKind::Text) => (
            "Class Text  <  EmptyUI",
            "A UI text label with a default size, alignment, and editable layout.",
        ),
        SceneEntityChoice::Entity(EntityKind::Button) => (
            "Class Button  <  EmptyUI",
            "An interactive UI button with a default label and layout rectangle.",
        ),
        SceneEntityChoice::Entity(EntityKind::Image) => (
            "Class Image  <  EmptyUI",
            "A UI image element ready for a texture and anchored layout.",
        ),
    }
}

fn choice_accent(choice: SceneEntityChoice) -> Color {
    choice_icon_kind(choice).icon_color()
}

const fn choice_icon_kind(choice: SceneEntityChoice) -> EntityKind {
    match choice {
        SceneEntityChoice::Root(RootNodeKind::TwoD) => EntityKind::Empty2D,
        SceneEntityChoice::Root(RootNodeKind::ThreeD) => EntityKind::Empty3D,
        SceneEntityChoice::Entity(kind) => kind,
    }
}

fn dialog_background() -> Color {
    Color::srgb(0.145, 0.145, 0.150)
}

fn sidebar_panel() -> Color {
    Color::srgb(0.095, 0.095, 0.100)
}

fn matches_background() -> Color {
    Color::srgb(0.085, 0.085, 0.090)
}

fn description_background() -> Color {
    Color::srgb(0.075, 0.075, 0.080)
}

fn search_background() -> Color {
    Color::srgb(0.075, 0.075, 0.082)
}

fn category_background() -> Color {
    Color::srgb(0.245, 0.245, 0.255)
}

fn button_background() -> Color {
    Color::srgb(0.275, 0.275, 0.285)
}

fn create_background() -> Color {
    theme::accent()
}

fn selected_background() -> Color {
    Color::srgb(0.285, 0.285, 0.295)
}

fn hover_background() -> Color {
    Color::srgb(0.20, 0.205, 0.215)
}

fn panel_border() -> Color {
    Color::srgb(0.18, 0.18, 0.19)
}

fn search_border() -> Color {
    Color::srgb(0.12, 0.47, 0.88)
}

fn primary_text() -> Color {
    Color::srgb(0.90, 0.90, 0.92)
}

fn label_color() -> Color {
    Color::srgb(0.76, 0.76, 0.79)
}

fn muted_text() -> Color {
    Color::srgb(0.47, 0.47, 0.50)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_labels_and_descriptions_cover_every_entity_kind() {
        for kind in [
            EntityKind::Empty,
            EntityKind::AnimationPlayer,
            EntityKind::Empty2D,
            EntityKind::CollisionRect2D,
            EntityKind::Empty3D,
            EntityKind::Sprite2D,
            EntityKind::Camera2D,
            EntityKind::Mesh3D,
            EntityKind::Camera3D,
            EntityKind::DirectionalLight3D,
            EntityKind::PointLight3D,
            EntityKind::SpotLight3D,
            EntityKind::EmptyUi,
            EntityKind::Panel,
            EntityKind::Text,
            EntityKind::Button,
            EntityKind::Image,
        ] {
            let choice = SceneEntityChoice::Entity(kind);
            assert!(!choice_label(choice).is_empty());
            let (title, body) = choice_description(choice);
            assert!(!title.is_empty());
            assert!(!body.is_empty());
        }
    }

    #[test]
    fn every_entity_kind_has_an_existing_icon_asset() {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        for kind in [
            EntityKind::Empty,
            EntityKind::AnimationPlayer,
            EntityKind::Empty2D,
            EntityKind::CollisionRect2D,
            EntityKind::Empty3D,
            EntityKind::Sprite2D,
            EntityKind::Camera2D,
            EntityKind::Mesh3D,
            EntityKind::Camera3D,
            EntityKind::DirectionalLight3D,
            EntityKind::PointLight3D,
            EntityKind::SpotLight3D,
            EntityKind::EmptyUi,
            EntityKind::Panel,
            EntityKind::Text,
            EntityKind::Button,
            EntityKind::Image,
        ] {
            let icon = workspace.join("assets").join(kind.icon_path());
            assert!(
                icon.is_file(),
                "missing icon for {kind:?}: {}",
                icon.display()
            );
        }
    }

    #[test]
    fn picker_uses_four_flat_empty_based_groups() {
        assert_eq!(
            PickerGroup::ALL.map(PickerGroup::label),
            ["Empty", "2D", "3D", "UI"]
        );
        assert_eq!(
            choice_label(SceneEntityChoice::Root(RootNodeKind::TwoD)),
            "Empty2D Scene"
        );
        assert_eq!(
            choice_label(SceneEntityChoice::Root(RootNodeKind::ThreeD)),
            "Empty3D Scene"
        );
        for choice in [
            SceneEntityChoice::Root(RootNodeKind::TwoD),
            SceneEntityChoice::Root(RootNodeKind::ThreeD),
            SceneEntityChoice::Entity(EntityKind::Empty2D),
            SceneEntityChoice::Entity(EntityKind::CollisionRect2D),
            SceneEntityChoice::Entity(EntityKind::Empty3D),
            SceneEntityChoice::Entity(EntityKind::Sprite2D),
            SceneEntityChoice::Entity(EntityKind::Mesh3D),
            SceneEntityChoice::Entity(EntityKind::EmptyUi),
            SceneEntityChoice::Entity(EntityKind::Panel),
            SceneEntityChoice::Entity(EntityKind::Text),
            SceneEntityChoice::Entity(EntityKind::Button),
            SceneEntityChoice::Entity(EntityKind::Image),
        ] {
            let (title, _) = choice_description(choice);
            assert!(!title.contains("Node2D"));
            assert!(!title.contains("Node3D"));
        }
    }

    #[test]
    fn existing_scene_exposes_every_three_d_entity_choice() {
        for kind in [
            EntityKind::Empty3D,
            EntityKind::Mesh3D,
            EntityKind::Camera3D,
            EntityKind::DirectionalLight3D,
            EntityKind::PointLight3D,
            EntityKind::SpotLight3D,
        ] {
            assert!(picker_choice_is_available(
                true,
                SceneEntityChoice::Entity(kind)
            ));
        }
    }

    #[test]
    fn picker_groups_toggle_between_expanded_and_collapsed() {
        let mut app = App::new();
        app.init_resource::<EntityPickerGroupState>()
            .add_observer(handle_picker_group_toggle);
        let category = app
            .world_mut()
            .spawn(EntityPickerCategory(PickerGroup::TwoD))
            .id();

        app.world_mut().trigger(Activate { entity: category });
        assert!(
            app.world()
                .resource::<EntityPickerGroupState>()
                .collapsed
                .contains(&PickerGroup::TwoD)
        );

        app.world_mut().trigger(Activate { entity: category });
        assert!(
            !app.world()
                .resource::<EntityPickerGroupState>()
                .collapsed
                .contains(&PickerGroup::TwoD)
        );
    }

    #[test]
    fn picker_description_splitter_resizes_and_clamps_matches_panel() {
        let mut layout = EntityPickerLayout::default();
        layout.resize_matches(75.0);
        assert_eq!(layout.matches_height, 375.0);
        layout.resize_matches(-1000.0);
        assert_eq!(layout.matches_height, 160.0);
        layout.resize_matches(1000.0);
        assert_eq!(layout.matches_height, 600.0);
    }
}
