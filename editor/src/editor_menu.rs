//! Editor-level menu actions that affect local editor chrome.

use bevy::{prelude::*, ui_widgets::Activate};

use crate::{
    panels::{ViewportLayout, ViewportLayoutMode},
    ui::theme,
};

#[derive(Component, Clone, Copy, Default)]
pub struct EditorMenuButton;

#[derive(Component, Clone, Copy, Default)]
pub struct EditorMenuDropdown;

#[derive(Component, Clone, Copy, Default)]
pub struct EditorMenuItem;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorMenuAction {
    #[default]
    SetDefaultLayout,
    ThreeViewportLayout,
}

#[derive(Resource, Debug, Default)]
struct EditorMenuState {
    open: bool,
}

pub struct EditorMenuPlugin;

impl Plugin for EditorMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorMenuState>()
            .add_observer(handle_editor_menu_action)
            .add_systems(
                Update,
                (
                    dismiss_editor_menu_on_outside_click,
                    sync_editor_menu_visibility,
                    sync_editor_menu_chrome,
                )
                    .chain(),
            );
    }
}

fn handle_editor_menu_action(
    activate: On<Activate>,
    menu_buttons: Query<(), With<EditorMenuButton>>,
    actions: Query<&EditorMenuAction>,
    mut menu: ResMut<EditorMenuState>,
    mut viewport_layout: ResMut<ViewportLayout>,
) {
    if menu_buttons.contains(activate.entity) {
        menu.open = !menu.open;
        return;
    }

    let Ok(action) = actions.get(activate.entity).copied() else {
        return;
    };
    match action {
        EditorMenuAction::SetDefaultLayout => viewport_layout.reset_to_default(),
        EditorMenuAction::ThreeViewportLayout => {
            viewport_layout.mode = ViewportLayoutMode::ThreeViewport;
        }
    }
    menu.open = false;
}

fn dismiss_editor_menu_on_outside_click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut menu: ResMut<EditorMenuState>,
    surfaces: Query<
        &Interaction,
        Or<(
            With<EditorMenuButton>,
            With<EditorMenuDropdown>,
            With<EditorMenuItem>,
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

fn sync_editor_menu_visibility(
    menu: Res<EditorMenuState>,
    mut dropdowns: Query<&mut Node, With<EditorMenuDropdown>>,
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

fn sync_editor_menu_chrome(
    menu: Res<EditorMenuState>,
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<EditorMenuButton>, Without<EditorMenuItem>),
    >,
    mut items: Query<
        (&Interaction, &mut BackgroundColor),
        (With<EditorMenuItem>, Without<EditorMenuButton>),
    >,
) {
    for (interaction, mut background) in &mut buttons {
        background.0 = if menu.open || *interaction != Interaction::None {
            theme::bg_hover()
        } else {
            Color::NONE
        };
    }
    for (interaction, mut background) in &mut items {
        background.0 = if *interaction != Interaction::None {
            theme::bg_hover()
        } else {
            Color::NONE
        };
    }
}
