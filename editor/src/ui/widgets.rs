//! Small BSN scene functions reused throughout the editor shell.

use bevy::{
    feathers::{
        controls::{ButtonVariant, FeathersToolButton},
        display::icon,
        theme::ThemedText,
    },
    gizmos::transform_gizmo::TransformGizmoMode,
    prelude::*,
};

use crate::editor_menu::{EditorMenuAction, EditorMenuButton, EditorMenuDropdown, EditorMenuItem};
use crate::hierarchy::{SceneContextAddEntity, SceneNodeAction, SceneNodeActionButton};
use crate::inspector::{InspectorTab, InspectorTabChrome, InspectorTabCount, InspectorTabKind};
use crate::play::{GameRunAction, GameRunButton};
use crate::project_settings::{ProjectMenuButton, ProjectMenuDropdown, ProjectSettingsMenuButton};
use crate::scene::{CloseSceneButton, SaveSceneButton};
use crate::undo::{HistoryAction, HistoryActionButton};
use crate::workspace::{EditorViewMode, EditorViewTab, EditorViewTabChrome};
use crate::workspace::{WorkspaceMode, WorkspaceTab, WorkspaceTabChrome};

use super::{
    components::{
        GizmoToolbarButton, Snap2dGridButton, Snap2dStepButton, Snap2dStepLabel,
        Snap2dToolbarButton,
    },
    theme,
};

pub fn text_label(text: impl Into<String>, size: f32, color: Color) -> impl Scene {
    bsn! {
        Text(text)
        TextFont { font_size: FontSize::Px(size) }
        TextColor(color)
        ThemedText
    }
}

pub fn menu_item(text: impl Into<String>) -> impl Scene {
    bsn! {
        Node {
            height: percent(100),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
        }
        Children [({text_label(text, 12.0, theme::text_primary())})]
    }
}

pub fn editor_menu() -> impl Scene {
    bsn! {
        Node {
            height: percent(100),
            position_type: PositionType::Relative,
        }
        Children [
            (
                @FeathersToolButton { @variant: ButtonVariant::Plain }
                EditorMenuButton
                Node {
                    height: percent(100),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(8)),
                    border_radius: BorderRadius::all(px(3)),
                }
                BackgroundColor(Color::NONE)
                Children [({text_label("Edit", 12.0, theme::text_primary())})]
            ),
            (
                EditorMenuDropdown
                template_value(Interaction::None)
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(theme::MENU_HEIGHT),
                    width: px(260),
                    min_width: px(260),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    padding: px(5),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                }
                GlobalZIndex(5000)
                BackgroundColor(theme::bg_menu())
                BorderColor::all(theme::border_soft())
                Children [
                    ({editor_menu_section_label("Layout")}),
                    ({editor_menu_action_item(
                        EditorMenuAction::SetDefaultLayout,
                        "Set Default Layout",
                    )}),
                    ({editor_menu_action_item(
                        EditorMenuAction::ThreeViewportLayout,
                        "Three Viewports",
                    )})
                ]
            )
        ]
    }
}

fn editor_menu_section_label(label: &'static str) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(24),
            min_height: px(24),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(10)),
        }
        Children [({text_label(label, 10.5, theme::text_muted())})]
    }
}

fn editor_menu_action_item(action: EditorMenuAction, label: &'static str) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        EditorMenuItem
        template_value(action)
        Node {
            width: percent(100),
            height: px(32),
            min_height: px(32),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(12)),
            border_radius: BorderRadius::all(px(2)),
        }
        BackgroundColor(Color::NONE)
        Children [({text_label(label, 12.0, theme::text_primary())})]
    }
}

pub fn project_menu() -> impl Scene {
    bsn! {
        Node {
            height: percent(100),
            position_type: PositionType::Relative,
        }
        Children [
            (
                @FeathersToolButton { @variant: ButtonVariant::Plain }
                ProjectMenuButton
                Node {
                    height: percent(100),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(8)),
                    border_radius: BorderRadius::all(px(3)),
                }
                BackgroundColor(Color::NONE)
                Children [({text_label("Project", 12.0, theme::text_primary())})]
            ),
            (
                ProjectMenuDropdown
                template_value(Interaction::None)
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(theme::MENU_HEIGHT),
                    width: px(310),
                    min_width: px(310),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    padding: px(5),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                }
                GlobalZIndex(5000)
                BackgroundColor(theme::bg_menu())
                BorderColor::all(theme::border_soft())
                Children [({project_settings_dropdown_item()})]
            )
        ]
    }
}

fn project_settings_dropdown_item() -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        ProjectSettingsMenuButton
        Node {
            width: percent(100),
            height: px(34),
            min_height: px(34),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(12)),
            border_radius: BorderRadius::all(px(2)),
        }
        BackgroundColor(Color::NONE)
        Children [({text_label("Project Settings...", 12.0, theme::text_primary())})]
    }
}

pub fn separator(vertical_margin: f32) -> impl Scene {
    bsn! {
        Node {
            width: px(1),
            height: px(22),
            margin: UiRect::vertical(px(vertical_margin)),
        }
        BackgroundColor(theme::border_soft())
    }
}

pub fn toolbar_icon(image: &'static str) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        Node {
            width: px(30),
            min_width: px(30),
            height: px(28),
            padding: px(6),
        }
        Children [({icon(image)})]
    }
}

pub fn game_run_button(action: GameRunAction, image: &'static str) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        GameRunButton(action)
        Node {
            width: px(30),
            min_width: px(30),
            height: px(28),
            padding: px(6),
            border: px(1),
        }
        BorderColor::all(Color::NONE)
        Children [({icon(image)})]
    }
}

pub fn history_button(action: HistoryAction, image: &'static str) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        HistoryActionButton(action)
        Node {
            width: px(30),
            min_width: px(30),
            height: px(28),
            padding: px(6),
        }
        Children [({icon(image)})]
    }
}

pub fn scene_context_add_entity_button() -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        SceneContextAddEntity
        Node {
            width: percent(100),
            height: px(34),
            min_height: px(34),
            padding: UiRect::horizontal(px(10)),
            align_items: AlignItems::Center,
            column_gap: px(9),
        }
        Children [
            ({icon("editor/icons/plus.png")}),
            ({text_label("Add Entity...", 12.0, theme::text_primary())})
        ]
    }
}

pub fn scene_add_entity_button() -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        SceneContextAddEntity
        Node {
            width: px(30),
            min_width: px(30),
            height: px(28),
            padding: px(6),
        }
        Children [({icon("editor/icons/plus.png")})]
    }
}

pub fn scene_context_node_action_button(
    action: SceneNodeAction,
    label: &'static str,
) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        SceneNodeActionButton(action)
        Node {
            width: percent(100),
            height: px(32),
            min_height: px(32),
            padding: UiRect::horizontal(px(10)),
            align_items: AlignItems::Center,
        }
        Children [({text_label(label, 12.0, theme::text_primary())})]
    }
}

pub fn scene_node_action_button(action: SceneNodeAction, image: &'static str) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        SceneNodeActionButton(action)
        Node {
            width: px(30),
            min_width: px(30),
            height: px(28),
            padding: px(6),
        }
        Children [({icon(image)})]
    }
}

pub fn toolbar_icon_text(image: &'static str, text: impl Into<String>) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        Node {
            height: px(30),
            padding: UiRect::axes(px(7), px(5)),
            column_gap: px(7),
        }
        Children [
            ({icon(image)}),
            ({text_label(text, 11.5, theme::text_primary())})
        ]
    }
}

pub fn editor_view_tab(mode: EditorViewMode, image: &'static str) -> impl Scene {
    let width = match mode {
        EditorViewMode::TwoD | EditorViewMode::ThreeD => 68.0,
        EditorViewMode::Game => 84.0,
        EditorViewMode::AssetStore => 120.0,
    };
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        EditorViewTab(mode)
        EditorViewTabChrome(mode)
        Node {
            height: px(38),
            width: px(width),
            min_width: px(width),
            max_width: px(width),
            flex_basis: px(width),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(12)),
            column_gap: px(7),
            border: UiRect::bottom(px(2)),
            border_radius: BorderRadius::new(px(4), px(4), px(0), px(0)),
        }
        BackgroundColor(Color::NONE)
        BorderColor::all(Color::NONE)
        Children [
            ({icon(image)}),
            ({text_label(mode.label(), 12.0, theme::text_muted())})
        ]
    }
}

pub fn workspace_mode_tab(mode: WorkspaceMode) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        WorkspaceTab(mode)
        WorkspaceTabChrome { mode }
        Node {
            height: px(24),
            min_width: px(38),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(8)),
            border: UiRect::bottom(px(2)),
            border_radius: BorderRadius::all(px(3)),
        }
        BackgroundColor(Color::NONE)
        BorderColor::all(Color::NONE)
        Children [({text_label(mode.label(), 10.5, theme::text_muted())})]
    }
}

pub fn save_scene_button() -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        SaveSceneButton
        Node {
            height: px(30),
            padding: UiRect::axes(px(7), px(5)),
            column_gap: px(7),
        }
        Children [
            ({icon("editor/icons/save.png")}),
            ({text_label("Save", 11.5, theme::text_primary())})
        ]
    }
}

pub fn close_scene_button() -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        CloseSceneButton
        Node {
            width: px(24),
            min_width: px(24),
            height: px(28),
            padding: px(5),
        }
        Children [({icon("editor/icons/x.png")})]
    }
}

pub fn gizmo_button(image: &'static str, mode: TransformGizmoMode) -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        GizmoToolbarButton(mode)
        Node {
            width: px(30),
            min_width: px(30),
            height: px(28),
            padding: px(6),
            border: px(1),
        }
        BorderColor::all(Color::NONE)
        Children [({icon(image)})]
    }
}

pub fn snap_2d_button() -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        Snap2dToolbarButton
        Node {
            width: px(30),
            min_width: px(30),
            height: px(28),
            padding: px(6),
            border: px(1),
        }
        BorderColor::all(Color::NONE)
        Children [({icon("editor/icons/magnet.png")})]
    }
}

pub fn snap_2d_step_button() -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        Snap2dStepButton
        Node {
            height: px(28),
            min_width: px(50),
            padding: UiRect::horizontal(px(6)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        Children [
            (
                Snap2dStepLabel
                Text("32 px")
                TextFont { font_size: FontSize::Px(10.5) }
                TextColor(theme::text_muted())
                ThemedText
            )
        ]
    }
}

pub fn grid_2d_button() -> impl Scene {
    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        Snap2dGridButton
        Node {
            width: px(30),
            min_width: px(30),
            height: px(28),
            padding: px(6),
            border: px(1),
        }
        BorderColor::all(Color::NONE)
        Children [({icon("editor/icons/grid-3x3.png")})]
    }
}

pub fn search_box(placeholder: impl Into<String>) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(28),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            column_gap: px(7),
            border: px(1),
            border_radius: BorderRadius::all(px(3)),
        }
        BackgroundColor(theme::bg_field())
        BorderColor::all(theme::border_soft())
        Children [
            ({icon("editor/icons/search.png")}),
            ({text_label(placeholder, 11.0, theme::text_muted())})
        ]
    }
}

pub fn panel_tab(text: impl Into<String>, active: bool) -> impl Scene {
    let border_color = if active { theme::accent() } else { Color::NONE };
    let text_color = if active {
        theme::text_primary()
    } else {
        theme::text_muted()
    };

    bsn! {
        Node {
            height: percent(100),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(10)),
            border: UiRect::bottom(px(2)),
        }
        BorderColor::all(border_color)
        Children [({text_label(text, 11.5, text_color)})]
    }
}

pub fn inspector_tab(kind: InspectorTabKind) -> impl Scene {
    let active = kind == InspectorTabKind::Components;
    let border_color = if active { theme::accent() } else { Color::NONE };
    let text_color = if active {
        theme::text_primary()
    } else {
        theme::text_muted()
    };

    bsn! {
        @FeathersToolButton { @variant: ButtonVariant::Plain }
        InspectorTab(kind)
        InspectorTabChrome(kind)
        Node {
            height: percent(100),
            flex_grow: 1.0,
            min_width: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(10)),
            column_gap: px(6),
            border: UiRect::bottom(px(2)),
        }
        BackgroundColor(Color::NONE)
        BorderColor::all(border_color)
        Children [
            ({icon(kind.icon_path())}),
            ({text_label(kind.label(), 11.5, text_color)}),
            (
                InspectorTabCount(kind)
                Text("0")
                TextFont { font_size: FontSize::Px(10.5) }
                TextColor(theme::accent())
            )
        ]
    }
}
