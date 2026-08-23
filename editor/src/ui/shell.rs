//! The stable editor hierarchy, authored with Bevy Scene Notation (BSN).
//!
//! Runtime-owned content is represented by host markers and mounted by
//! `ui::mount_dynamic_panels`; this keeps this file focused on layout only.

use bevy::{
    feathers::{
        controls::{ButtonVariant, FeathersToolButton},
        display::icon,
    },
    gizmos::transform_gizmo::TransformGizmoMode,
    picking::Pickable,
    prelude::*,
    ui::RelativeCursorPosition,
};

use crate::{
    hierarchy::{SceneContextAddEntity, SceneContextMenu, SceneNodeAction, SceneTreeList},
    inspector::InspectorTabKind,
    output::{OutputErrorLabel, OutputInfoLabel, OutputList, OutputTotalLabel, OutputWarningLabel},
    panels::{
        BottomDockContent, BottomDockPanel, BottomDockSplitter, BottomDockTab, BottomDockTabButton,
        BottomDockTabLabel, DetailsPanel, FileSystemPanel, OutputPanel, ScenePanel,
        ViewportPreviewBottom, ViewportPreviewColumn, ViewportPreviewSplitter, ViewportPreviewTop,
        ViewportSideSplitter,
    },
    play::{GameRunAction, GameViewportPane, GameViewportStatusLabel},
    undo::HistoryAction,
    viewport::EditorViewportPane,
    workspace::{EditorViewMode, EditorViewPanel, WorkspaceMode},
};

use super::{
    components::{
        AnimationTimelineHost, DetailsHost, DetailsSplitterHost, EditorVerticalScrollArea,
        FileSystemHost, FileSystemSplitterHost, SceneSaveDialogHost, SceneSplitterHost,
        SceneTabBar, SceneTabLabel, SystemScriptPickerHost, ViewportPreviewSplitterHost,
        ViewportSideSplitterHost, WorkspaceToolbarGroup,
    },
    theme,
    widgets::{
        close_scene_button, editor_menu, editor_view_tab, game_run_button, gizmo_button,
        grid_2d_button, history_button, inspector_tab, menu_item, panel_tab, project_menu,
        save_scene_button, scene_add_entity_button, scene_context_add_entity_button,
        scene_context_node_action_button, search_box, separator, snap_2d_button,
        snap_2d_step_button, text_label, toolbar_icon, toolbar_icon_text, workspace_mode_tab,
    },
};

pub fn editor_shell() -> impl SceneList {
    bsn_list![({ editor_root() })]
}

fn editor_root() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            min_width: px(0),
            min_height: px(0),
            max_width: percent(100),
            max_height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
        }
        BackgroundColor(theme::bg_app())
        Children [
            ({menu_bar()}),
            ({main_toolbar()}),
            ({main_workspace()}),
            ({scene_context_menu()}),
            (
                SceneSaveDialogHost
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(0),
                    bottom: px(0),
                    display: Display::None,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                GlobalZIndex(2000)
                BackgroundColor(Color::NONE)
            ),
            (
                SystemScriptPickerHost
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(0),
                    bottom: px(0),
                    display: Display::None,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                GlobalZIndex(2200)
                BackgroundColor(Color::NONE)
            )
        ]
    }
}

fn menu_bar() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(theme::MENU_HEIGHT),
            min_height: px(theme::MENU_HEIGHT),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            column_gap: px(3),
            border: UiRect::bottom(px(1)),
        }
        BackgroundColor(theme::bg_menu())
        BorderColor::all(theme::border())
        Children [
            ({icon("editor/icons/star.png")}),
            ({text_label("Bevy", 13.0, theme::text_primary())}),
            ({separator(4.0)}),
            ({menu_item("File")}),
            ({editor_menu()}),
            ({menu_item("Debug")}),
            ({project_menu()}),
            ({menu_item("Help")}),
            ({separator(4.0)}),
            ({game_run_button(GameRunAction::PlayProject, "editor/icons/play.png")}),
            ({game_run_button(GameRunAction::PlayCurrent, "editor/icons/camera.png")}),
            ({game_run_button(GameRunAction::Pause, "editor/icons/pause.png")}),
            ({game_run_button(GameRunAction::Stop, "editor/icons/square.png")}),
            (Node { flex_grow: 1.0 }),
            ({scene_view_dropdown()}),
            ({window_button("_")}),
            ({window_button("[]")}),
            ({window_button("x")})
        ]
    }
}

fn main_toolbar() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(theme::MAIN_TOOLBAR_HEIGHT),
            min_height: px(theme::MAIN_TOOLBAR_HEIGHT),
            display: Display::None,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(10)),
            column_gap: px(3),
            border: UiRect::bottom(px(1)),
        }
        BackgroundColor(theme::bg_toolbar())
        BorderColor::all(theme::border())
        Children [
            ({save_scene_button()}),
            ({separator(4.0)}),
            ({toolbar_icon_text("editor/icons/plus.png", "Add")}),
            ({toolbar_icon_text("editor/icons/folder-open.png", "Content")}),
            (
                Node {
                    width: px(40),
                    min_width: px(0),
                    flex_basis: px(40),
                    flex_shrink: 1.0,
                }
            ),
            (
                Node {
                    width: px(352),
                    min_width: px(352),
                    max_width: px(352),
                    flex_basis: px(352),
                    flex_shrink: 0.0,
                    height: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(2),
                }
                Children [
                    ({editor_view_tab(
                        EditorViewMode::TwoD,
                        "editor/icons/grid-3x3.png",
                    )}),
                    ({editor_view_tab(
                        EditorViewMode::ThreeD,
                        "editor/icons/box.png",
                    )}),
                    ({editor_view_tab(
                        EditorViewMode::Game,
                        "editor/icons/camera.png",
                    )}),
                    ({editor_view_tab(
                        EditorViewMode::AssetStore,
                        "editor/icons/folder-open.png",
                    )})
                ]
            ),
            (
                Node {
                    width: px(320),
                    min_width: px(320),
                    max_width: px(320),
                    flex_basis: px(320),
                    flex_shrink: 0.0,
                    height: percent(100),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::FlexEnd,
                    column_gap: px(3),
                }
                Children [
                    ({game_run_button(
                        GameRunAction::PlayProject,
                        "editor/icons/play.png",
                    )}),
                    ({game_run_button(
                        GameRunAction::Pause,
                        "editor/icons/pause.png",
                    )}),
                    ({game_run_button(
                        GameRunAction::Stop,
                        "editor/icons/square.png",
                    )}),
                    ({separator(4.0)}),
                    ({toolbar_icon_text("editor/icons/settings.png", "Platforms")}),
                    ({history_button(
                        HistoryAction::Undo,
                        "editor/icons/undo-2.png",
                    )}),
                    ({history_button(
                        HistoryAction::Redo,
                        "editor/icons/redo-2.png",
                    )})
                ]
            )
        ]
    }
}

fn scene_view_dropdown() -> impl Scene {
    bsn! {
        Node {
            height: px(28),
            min_width: px(126),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::horizontal(px(10)),
            border: px(1),
            border_radius: BorderRadius::all(px(5)),
        }
        BackgroundColor(theme::bg_panel())
        BorderColor::all(theme::border_soft())
        Children [
            ({text_label("Scene View", 11.5, theme::text_primary())}),
            ({icon("editor/icons/chevron-down.png")})
        ]
    }
}

fn window_button(label: &'static str) -> impl Scene {
    bsn! {
        Node {
            width: px(30),
            min_width: px(30),
            height: px(28),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        Children [({text_label(label, 13.0, theme::text_primary())})]
    }
}

fn main_workspace() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(0),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            overflow: Overflow::clip(),
        }
        Children [
            ({left_dock()}),
            (
                SceneSplitterHost
                Node {
                    width: px(6),
                    height: percent(100),
                    min_width: px(6),
                }
                BackgroundColor(theme::border())
            ),
            ({center_dock()}),
            (
                DetailsSplitterHost
                Node {
                    width: px(6),
                    height: percent(100),
                    min_width: px(6),
                }
                BackgroundColor(theme::border())
            ),
            ({details_panel()})
        ]
    }
}

fn left_dock() -> impl Scene {
    bsn! {
        ScenePanel
        Node {
            width: px(theme::SCENE_PANEL_WIDTH),
            min_width: px(220),
            max_width: px(560),
            flex_shrink: 0.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::visible(),
        }
        Children [
            ({scene_panel()}),
            (
                FileSystemSplitterHost
                Node {
                    width: percent(100),
                    height: px(6),
                    min_height: px(6),
                }
                BackgroundColor(theme::border())
            ),
            ({filesystem_panel()})
        ]
    }
}

fn center_dock() -> impl Scene {
    bsn! {
        Node {
            width: px(0),
            flex_grow: 1.0,
            flex_shrink: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::visible(),
        }
        Children [
            ({center_workspace()}),
            (
                BottomDockSplitter
                Node {
                    width: percent(100),
                    height: px(6),
                    min_height: px(6),
                    display: Display::Flex,
                }
                BackgroundColor(theme::border())
            ),
            ({bottom_dock()}),
            ({bottom_dock_tabs()})
        ]
    }
}

fn center_workspace() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(0),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            min_height: px(120),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
        }
        Children [
            ({scene_workspace()}),
            ({game_workspace()}),
            ({asset_store_workspace()})
        ]
    }
}

fn scene_workspace() -> impl Scene {
    bsn! {
        template_value(EditorViewPanel::Scene)
        Node {
            width: percent(100),
            height: percent(100),
            min_width: px(0),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
        }
        Children [
            ({document_tabs()}),
            (
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4),
                    padding: px(4),
                }
                Children [
                    ({live_viewport_panel()}),
                    (
                        ViewportSideSplitter
                        ViewportSideSplitterHost
                        Node {
                            width: px(6),
                            min_width: px(6),
                            height: percent(100),
                            display: Display::None,
                        }
                        BackgroundColor(theme::border())
                    ),
                    (
                        ViewportPreviewColumn
                        Node {
                            width: px(330),
                            min_width: px(260),
                            height: percent(100),
                            display: Display::None,
                            flex_direction: FlexDirection::Column,
                        }
                        Children [
                            (
                                ViewportPreviewTop
                                Node {
                                    width: percent(100),
                                    height: px(0),
                                    flex_grow: 0.5,
                                    min_height: px(0),
                                    display: Display::Flex,
                                    flex_direction: FlexDirection::Column,
                                }
                                Children [({preview_viewport_panel(EditorViewportPane::top_right())})]
                            ),
                            (
                                ViewportPreviewSplitter
                                ViewportPreviewSplitterHost
                                Node {
                                    width: percent(100),
                                    height: px(6),
                                    min_height: px(6),
                                    display: Display::None,
                                }
                                BackgroundColor(theme::border())
                            ),
                            (
                                ViewportPreviewBottom
                                Node {
                                    width: percent(100),
                                    height: px(0),
                                    flex_grow: 0.5,
                                    min_height: px(0),
                                    display: Display::Flex,
                                    flex_direction: FlexDirection::Column,
                                }
                                Children [({preview_viewport_panel(EditorViewportPane::bottom_right())})]
                            )
                        ]
                    )
                ]
            ),
            ({asset_browser_panel()})
        ]
    }
}

fn live_viewport_panel() -> impl Scene {
    bsn! {
        Node {
            width: px(0),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            border_radius: BorderRadius::all(px(6)),
        }
        BackgroundColor(theme::viewport_frame())
        Children [
            ({viewport_panel_toolbar(true)}),
            (
                Node {
                    width: percent(100),
                    height: px(0),
                    flex_grow: 1.0,
                    min_height: px(0),
                    border: UiRect::top(px(1)),
                }
                BorderColor::all(theme::border())
                Children [(
                    template_value(EditorViewportPane::main())
                    Node {
                        width: percent(100),
                        height: percent(100),
                    }
                    template_value(Pickable::IGNORE)
                )]
            )
        ]
    }
}

fn preview_viewport_panel(pane: EditorViewportPane) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(0),
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            border_radius: BorderRadius::all(px(6)),
        }
        BackgroundColor(theme::viewport_frame())
        Children [
            ({viewport_panel_toolbar(false)}),
            (
                Node {
                    width: percent(100),
                    height: px(0),
                    flex_grow: 1.0,
                    min_height: px(0),
                    border: UiRect::top(px(1)),
                }
                BackgroundColor(Color::srgb(0.185, 0.188, 0.190))
                BorderColor::all(theme::border())
                Children [
                    ({preview_viewport_pane(pane)}),
                    ({axis_gizmo()})
                ]
            )
        ]
    }
}

fn viewport_panel_toolbar(show_mode_tabs: bool) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(theme::VIEWPORT_TOOLBAR_HEIGHT),
            min_height: px(theme::VIEWPORT_TOOLBAR_HEIGHT),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(6)),
            column_gap: px(4),
        }
        BackgroundColor(theme::bg_toolbar())
        Children [
            ({workspace_tools_3d()}),
            ({workspace_tools_2d()}),
            ({workspace_mode_tabs(show_mode_tabs)}),
            ({text_label("Snap distance", 10.5, theme::text_muted())}),
            ({snap_value_box()}),
            (Node { flex_grow: 1.0 }),
            ({toolbar_icon("editor/icons/settings.png")})
        ]
    }
}

fn preview_viewport_pane(pane: EditorViewportPane) -> impl Scene {
    bsn! {
        template_value(pane)
        Node {
            width: percent(100),
            height: percent(100),
        }
        template_value(Pickable::IGNORE)
    }
}

fn workspace_mode_tabs(visible: bool) -> impl Scene {
    let display = if visible {
        Display::Flex
    } else {
        Display::None
    };
    bsn! {
        Node {
            height: px(26),
            display,
            align_items: AlignItems::Center,
            column_gap: px(2),
            padding: UiRect::horizontal(px(2)),
            border: px(1),
            border_radius: BorderRadius::all(px(4)),
        }
        BackgroundColor(theme::bg_panel_alt())
        BorderColor::all(theme::border_soft())
        Children [
            ({workspace_mode_tab(WorkspaceMode::TwoD)}),
            ({workspace_mode_tab(WorkspaceMode::ThreeD)})
        ]
    }
}

fn snap_value_box() -> impl Scene {
    bsn! {
        Node {
            width: px(46),
            height: px(22),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: px(1),
            border_radius: BorderRadius::all(px(4)),
        }
        BackgroundColor(theme::bg_panel_alt())
        BorderColor::all(theme::border_soft())
        Children [({text_label("5.00", 10.5, theme::text_muted())})]
    }
}

fn axis_gizmo() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            right: px(12),
            width: px(48),
            height: px(48),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        Children [
            ({text_label("X", 10.0, Color::srgb(0.95, 0.22, 0.28))}),
            ({text_label("Y", 10.0, Color::srgb(0.55, 0.85, 0.20))}),
            ({text_label("Z", 10.0, Color::srgb(0.25, 0.55, 1.0))})
        ]
    }
}

fn asset_browser_panel() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(theme::OUTPUT_PANEL_HEIGHT),
            min_height: px(150),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            margin: UiRect::horizontal(px(4)),
            overflow: Overflow::clip(),
            border_radius: BorderRadius::all(px(6)),
        }
        BackgroundColor(theme::bg_panel_alt())
        Children [
            (
                Node {
                    width: percent(100),
                    height: px(36),
                    min_height: px(36),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(8)),
                    column_gap: px(8),
                    border: UiRect::bottom(px(1)),
                }
                BackgroundColor(theme::bg_toolbar())
                BorderColor::all(theme::border())
                Children [
                    ({toolbar_icon("editor/icons/terminal.png")}),
                    ({toolbar_icon("editor/icons/folder-open.png")}),
                    ({text_label("project / assets / Stanford Dragon.gltf", 11.0, theme::text_muted())}),
                    (Node { flex_grow: 1.0 }),
                    (
                        Node { width: px(230) }
                        Children [({search_box("Search")})]
                    )
                ]
            ),
            (
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(0),
                    padding: UiRect::axes(px(28), px(14)),
                    column_gap: px(24),
                    align_items: AlignItems::FlexStart,
                }
                BackgroundColor(theme::bg_panel_alt())
                Children [
                    ({asset_tile("editor/icons/folder.png", "animation")}),
                    ({asset_tile("editor/icons/folder.png", "audio")}),
                    ({asset_tile("editor/icons/folder.png", "materials")}),
                    ({asset_tile("editor/icons/folder.png", "models")}),
                    ({asset_tile("editor/icons/folder.png", "scenes")}),
                    ({asset_tile("editor/icons/box.png", "Stanford...")})
                ]
            )
        ]
    }
}

fn asset_tile(icon_path: &'static str, label: &'static str) -> impl Scene {
    bsn! {
        Node {
            width: px(62),
            height: px(76),
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: UiRect::all(px(6)),
            border_radius: BorderRadius::all(px(5)),
        }
        BackgroundColor(Color::NONE)
        Children [
            (
                Node {
                    width: px(38),
                    height: px(34),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                Children [({icon(icon_path)})]
            ),
            ({text_label(label, 10.0, theme::text_muted())})
        ]
    }
}

fn game_workspace() -> impl Scene {
    bsn! {
        template_value(EditorViewPanel::Game)
        GameViewportPane
        Node {
            width: percent(100),
            height: percent(100),
            min_width: px(0),
            min_height: px(0),
            display: Display::None,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            overflow: Overflow::clip(),
        }
        BackgroundColor(theme::bg_field())
        template_value(Pickable::IGNORE)
        Children [
            (
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                }
                Children [
                    ({icon("editor/icons/camera.png")}),
                    (
                        GameViewportStatusLabel
                        Text("Run a scene to start the game")
                        TextFont { font_size: FontSize::Px(12.0) }
                        TextColor(theme::text_muted())
                    )
                ]
            )
        ]
    }
}

fn asset_store_workspace() -> impl Scene {
    bsn! {
        template_value(EditorViewPanel::AssetStore)
        Node {
            width: percent(100),
            height: percent(100),
            min_width: px(0),
            min_height: px(0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
        }
        BackgroundColor(theme::bg_field())
        Children [
            (
                Node {
                    width: percent(100),
                    height: px(40),
                    min_height: px(40),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(12)),
                    column_gap: px(9),
                    border: UiRect::bottom(px(1)),
                }
                BackgroundColor(theme::bg_toolbar())
                BorderColor::all(theme::border())
                Children [
                    ({icon("editor/icons/folder-open.png")}),
                    ({text_label("Asset Store", 12.0, theme::text_primary())}),
                    (Node { flex_grow: 1.0 }),
                    (
                        Node {
                            width: px(260),
                            max_width: percent(45),
                        }
                        Children [({search_box("Search packages")})]
                    )
                ]
            ),
            (
                Node {
                    width: percent(100),
                    height: px(0),
                    flex_grow: 1.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                Children [
                    (
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                        }
                        Children [
                            ({icon("editor/icons/box.png")}),
                            ({text_label("No packages available", 12.0, theme::text_muted())})
                        ]
                    )
                ]
            )
        ]
    }
}

fn document_tabs() -> impl Scene {
    bsn! {
        SceneTabBar
        Node {
            width: percent(100),
            height: px(theme::TAB_HEIGHT),
            min_height: px(theme::TAB_HEIGHT),
            display: Display::None,
            align_items: AlignItems::Center,
            border: UiRect::bottom(px(1)),
        }
        BackgroundColor(theme::bg_menu())
        BorderColor::all(theme::border())
        Children [
            (
                Node {
                    height: percent(100),
                    min_width: px(178),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(10)),
                    column_gap: px(8),
                    border: UiRect::right(px(1)),
                }
                BackgroundColor(theme::bg_panel())
                BorderColor::all(theme::border())
                Children [
                    ({icon("editor/icons/box.png")}),
                    (
                        SceneTabLabel
                        Text("Untitled Level")
                        TextFont { font_size: FontSize::Px(11.5) }
                        TextColor(theme::text_primary())
                    ),
                    (Node { flex_grow: 1.0 }),
                    ({close_scene_button()})
                ]
            ),
            ({toolbar_icon("editor/icons/plus.png")})
        ]
    }
}

fn viewport_toolbar() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(theme::VIEWPORT_TOOLBAR_HEIGHT),
            min_height: px(theme::VIEWPORT_TOOLBAR_HEIGHT),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
            column_gap: px(2),
            border: UiRect::bottom(px(1)),
        }
        BackgroundColor(theme::bg_toolbar())
        BorderColor::all(theme::border())
        Children [
            ({workspace_tools_3d()}),
            ({workspace_tools_2d()}),
            (Node { flex_grow: 1.0 }),
            ({viewport_options_3d()}),
            ({viewport_options_2d()})
        ]
    }
}

fn workspace_tools_3d() -> impl Scene {
    bsn! {
        WorkspaceToolbarGroup(WorkspaceMode::ThreeD)
        Node {
            height: percent(100),
            display: Display::Flex,
            align_items: AlignItems::Center,
            column_gap: px(2),
        }
        Children [
            ({toolbar_icon("editor/icons/mouse-pointer-2.png")}),
            ({gizmo_button("editor/icons/move-3d.png", TransformGizmoMode::Translate)}),
            ({gizmo_button("editor/icons/rotate-3d.png", TransformGizmoMode::Rotate)}),
            ({gizmo_button("editor/icons/scaling.png", TransformGizmoMode::Scale)}),
            ({separator(3.0)}),
            ({toolbar_icon("editor/icons/magnet.png")}),
            ({text_label("10 cm", 10.5, theme::text_muted())}),
            ({separator(3.0)}),
            ({toolbar_icon("editor/icons/grid-3x3.png")})
        ]
    }
}

fn workspace_tools_2d() -> impl Scene {
    bsn! {
        WorkspaceToolbarGroup(WorkspaceMode::TwoD)
        Node {
            height: percent(100),
            display: Display::None,
            align_items: AlignItems::Center,
            column_gap: px(2),
        }
        Children [
            ({toolbar_icon("editor/icons/mouse-pointer-2.png")}),
            ({gizmo_button("editor/icons/move-3d.png", TransformGizmoMode::Translate)}),
            ({gizmo_button("editor/icons/rotate-3d.png", TransformGizmoMode::Rotate)}),
            ({gizmo_button("editor/icons/scaling.png", TransformGizmoMode::Scale)}),
            ({separator(3.0)}),
            ({snap_2d_button()}),
            ({snap_2d_step_button()}),
            ({separator(3.0)}),
            ({grid_2d_button()}),
            ({separator(3.0)}),
            ({text_label("Canvas", 10.5, theme::accent())})
        ]
    }
}

fn viewport_options_3d() -> impl Scene {
    bsn! {
        WorkspaceToolbarGroup(WorkspaceMode::ThreeD)
        Node {
            height: percent(100),
            display: Display::Flex,
            align_items: AlignItems::Center,
            column_gap: px(2),
        }
        Children [
            ({toolbar_icon_text("editor/icons/eye.png", "Perspective")}),
            ({toolbar_icon_text("editor/icons/sun.png", "Lit")}),
            ({toolbar_icon("editor/icons/settings.png")})
        ]
    }
}

fn viewport_options_2d() -> impl Scene {
    bsn! {
        WorkspaceToolbarGroup(WorkspaceMode::TwoD)
        Node {
            height: percent(100),
            display: Display::None,
            align_items: AlignItems::Center,
            column_gap: px(2),
        }
        Children [
            ({toolbar_icon_text("editor/icons/eye.png", "Orthographic")}),
            ({toolbar_icon_text("editor/icons/grid-3x3.png", "Canvas")}),
            ({toolbar_icon("editor/icons/settings.png")})
        ]
    }
}

fn scene_panel() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(0),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_height: px(120),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::visible(),
        }
        BackgroundColor(theme::bg_panel_alt())
        Children [
            (
                Node {
                    width: percent(100),
                    height: px(32),
                    min_height: px(32),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(8)),
                    column_gap: px(10),
                    border: UiRect::bottom(px(1)),
                }
                BackgroundColor(theme::bg_toolbar())
                BorderColor::all(theme::border())
                Children [
                    ({panel_tab("Scene Tree", true)}),
                    ({panel_tab("Import", false)}),
                    ({scene_add_entity_button()}),
                    (Node { flex_grow: 1.0 }),
                    ({toolbar_icon("editor/icons/settings.png")})
                ]
            ),
            (
                Node {
                    width: percent(100),
                    height: px(36),
                    min_height: px(36),
                    padding: UiRect::axes(px(6), px(5)),
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                BackgroundColor(theme::bg_panel())
                Children [
                    ({search_box("Filter...")}),
                    ({toolbar_icon("editor/icons/settings.png")})
                ]
            ),
            (
                Node {
                    width: percent(100),
                    height: px(32),
                    min_height: px(32),
                    padding: UiRect::axes(px(6), px(3)),
                    align_items: AlignItems::Center,
                }
                BackgroundColor(theme::bg_panel())
                Children [({scene_add_entity_bar()})]
            ),
            (
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(0),
                    position_type: PositionType::Relative,
                    overflow: Overflow::clip(),
                }
                BackgroundColor(theme::bg_panel_alt())
                Children [
                    (
                        EditorVerticalScrollArea
                        SceneTreeList
                        Button
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(12),
                            top: px(0),
                            bottom: px(0),
                            padding: UiRect::axes(px(5), px(4)),
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(1),
                            overflow: Overflow::scroll_y(),
                        }
                        BackgroundColor(theme::bg_panel_alt())
                    )
                ]
            )
        ]
    }
}

fn scene_add_entity_bar() -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        SceneContextAddEntity
        Node {
            width: percent(100),
            height: px(26),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: px(6),
            border_radius: BorderRadius::all(px(4)),
        }
        BackgroundColor(theme::bg_field())
        Children [
            ({icon("editor/icons/box.png")}),
            ({text_label("Add Entity", 11.5, theme::text_primary())})
        ]
    }
}

fn scene_context_menu() -> impl Scene {
    bsn! {
        SceneContextMenu
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            min_width: px(220),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(4)),
            row_gap: px(1),
            border: px(1),
            border_radius: BorderRadius::all(px(3)),
        }
        GlobalZIndex(2300)
        BackgroundColor(Color::srgb(0.11, 0.115, 0.125))
        BorderColor::all(Color::srgb(0.04, 0.045, 0.05))
        Pickable::default()
        RelativeCursorPosition::default()
        Children [
            ({scene_context_add_entity_button()}),
            (
                Node {
                    width: percent(100),
                    height: px(1),
                    margin: UiRect::vertical(px(3)),
                }
                BackgroundColor(theme::border_soft())
            ),
            ({scene_context_node_action_button(
                SceneNodeAction::MakeRoot,
                "Set as Scene Root",
            )}),
            ({scene_context_node_action_button(SceneNodeAction::Duplicate, "Duplicate")}),
            ({scene_context_node_action_button(SceneNodeAction::Rename, "Rename")}),
            ({scene_context_node_action_button(SceneNodeAction::Delete, "Delete")})
        ]
    }
}

fn details_panel() -> impl Scene {
    bsn! {
        DetailsPanel
        Node {
            width: px(theme::DETAILS_PANEL_WIDTH),
            min_width: px(260),
            max_width: px(680),
            flex_shrink: 0.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
        }
        BackgroundColor(theme::bg_panel_alt())
        Children [
            (
                Node {
                    width: percent(100),
                    height: px(38),
                    min_height: px(38),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: UiRect::horizontal(px(12)),
                    border: UiRect::bottom(px(1)),
                }
                BackgroundColor(theme::bg_toolbar())
                BorderColor::all(theme::border())
                Children [
                    ({text_label("Inspector", 13.0, theme::text_primary())}),
                    (
                        Node {
                            height: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(2),
                        }
                        Children [
                            ({toolbar_icon("editor/icons/lock.png")}),
                            ({toolbar_icon("editor/icons/settings.png")})
                        ]
                    )
                ]
            ),
            (
                Node {
                    width: percent(100),
                    height: px(43),
                    min_height: px(43),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(7)),
                    border: UiRect::bottom(px(1)),
                }
                BackgroundColor(theme::bg_toolbar())
                BorderColor::all(theme::border())
                Children [
                    ({inspector_tab(InspectorTabKind::Components)}),
                    ({inspector_tab(InspectorTabKind::Systems)}),
                    ({toolbar_icon("editor/icons/plus.png")})
                ]
            ),
            (
                DetailsHost
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip(),
                }
            )
        ]
    }
}

fn filesystem_panel() -> impl Scene {
    bsn! {
        FileSystemPanel
        Node {
            width: percent(100),
            min_width: px(0),
            height: px(theme::FILESYSTEM_PANEL_HEIGHT),
            min_height: px(140),
            max_height: px(520),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
        }
        BackgroundColor(theme::bg_panel_alt())
        Children [
            (
                FileSystemHost
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                }
            )
        ]
    }
}

fn bottom_dock() -> impl Scene {
    bsn! {
        BottomDockPanel
        Node {
            width: percent(100),
            height: px(250),
            min_height: px(170),
            max_height: px(520),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::visible(),
        }
        BackgroundColor(theme::bg_panel_alt())
        Children [
            (
                BottomDockContent(BottomDockTab::Output)
                Node {
                    width: percent(100),
                    height: percent(100),
                    min_width: px(0),
                    min_height: px(0),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip(),
                }
                Children [({output_panel()})]
            ),
            (
                BottomDockContent(BottomDockTab::Debugger)
                Node {
                    width: percent(100),
                    height: percent(100),
                    min_width: px(0),
                    min_height: px(0),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    overflow: Overflow::clip(),
                }
                BackgroundColor(theme::bg_panel_alt())
                Children [({text_label(
                    "No active debug session",
                    11.0,
                    theme::text_muted(),
                )})]
            ),
            (
                BottomDockContent(BottomDockTab::Animation)
                AnimationTimelineHost
                Node {
                    width: percent(100),
                    height: percent(100),
                    min_width: px(0),
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip(),
                }
            )
        ]
    }
}

fn bottom_dock_tabs() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(32),
            min_height: px(32),
            align_items: AlignItems::Stretch,
            padding: UiRect::horizontal(px(6)),
            column_gap: px(2),
            border: UiRect::top(px(1)),
        }
        BackgroundColor(theme::bg_menu())
        BorderColor::all(theme::border())
        Children [
            ({bottom_dock_tab(BottomDockTab::Output, "Output")}),
            ({bottom_dock_tab(BottomDockTab::Debugger, "Debugger")}),
            ({bottom_dock_tab(BottomDockTab::Animation, "Animation")}),
            (Node { flex_grow: 1.0 })
        ]
    }
}

fn bottom_dock_tab(tab: BottomDockTab, label: &'static str) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        BottomDockTabButton(tab)
        Node {
            height: percent(100),
            min_width: px(74),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(11)),
            border: UiRect::bottom(px(2)),
        }
        BackgroundColor(Color::NONE)
        BorderColor::all(Color::NONE)
        Children [
            (
                BottomDockTabLabel(tab)
                Text(label)
                TextFont { font_size: FontSize::Px(11.0) }
                TextColor(theme::text_muted())
            )
        ]
    }
}

fn output_panel() -> impl Scene {
    bsn! {
        OutputPanel
        Node {
            width: percent(100),
            min_width: px(0),
            height: percent(100),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
        }
        BackgroundColor(theme::bg_panel_alt())
        Children [
            (
                Node {
                    width: percent(100),
                    height: px(38),
                    min_height: px(38),
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(7), px(5)),
                    column_gap: px(8),
                    border: UiRect::bottom(px(1)),
                }
                BackgroundColor(theme::bg_panel())
                BorderColor::all(theme::border())
                Children [
                    (
                        Node {
                            width: px(260),
                            max_width: percent(55),
                        }
                        Children [({search_box("Filter output")})]
                    ),
                    ({text_label("All", 10.5, theme::accent())}),
                    (
                        OutputInfoLabel
                        Text("Info 2")
                        TextFont { font_size: FontSize::Px(10.5) }
                        TextColor(theme::text_muted())
                    ),
                    (
                        OutputWarningLabel
                        Text("Warnings 0")
                        TextFont { font_size: FontSize::Px(10.5) }
                        TextColor(theme::warning())
                    ),
                    (
                        OutputErrorLabel
                        Text("Errors 0")
                        TextFont { font_size: FontSize::Px(10.5) }
                        TextColor(theme::text_muted())
                    ),
                    (Node { flex_grow: 1.0 }),
                    (
                        OutputTotalLabel
                        Text("2 messages")
                        TextFont { font_size: FontSize::Px(10.5) }
                        TextColor(theme::text_muted())
                    ),
                    ({toolbar_icon("editor/icons/settings.png")})
                ]
            ),
            (
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(0),
                    position_type: PositionType::Relative,
                    overflow: Overflow::clip(),
                }
                BackgroundColor(theme::bg_field())
                Children [
                    (
                        OutputList
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(12),
                            top: px(0),
                            bottom: px(0),
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::axes(px(10), px(7)),
                            row_gap: px(2),
                            overflow: Overflow::scroll_y(),
                        }
                    )
                ]
            )
        ]
    }
}
