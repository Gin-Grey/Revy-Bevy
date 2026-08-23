use bevy::{gizmos::transform_gizmo::TransformGizmoFocus, prelude::*, ui_widgets::Activate};

use crate::selection::{EditableObject, Selection};
use crate::ui::theme;

/// Which scene workspace and camera are currently active.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceMode {
    #[default]
    TwoD,
    ThreeD,
}

impl WorkspaceMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::TwoD => "2D",
            Self::ThreeD => "3D",
        }
    }
}

/// The editor surface shown in the center dock. Scene dimension stays in
/// `WorkspaceMode` so runtime views never leak into scene serialization.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorViewMode {
    #[default]
    TwoD,
    ThreeD,
    Game,
    AssetStore,
}

impl EditorViewMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::TwoD => "2D",
            Self::ThreeD => "3D",
            Self::Game => "Game",
            Self::AssetStore => "AssetStore",
        }
    }

    fn workspace_mode(self) -> Option<WorkspaceMode> {
        match self {
            Self::TwoD => Some(WorkspaceMode::TwoD),
            Self::ThreeD => Some(WorkspaceMode::ThreeD),
            Self::Game | Self::AssetStore => None,
        }
    }
}

/// Remembers each workspace's selection while the user moves between modes.
#[derive(Resource, Debug, Clone, Default)]
pub struct WorkspaceSelections {
    two_d: Option<Entity>,
    three_d: Option<Entity>,
}

impl WorkspaceSelections {
    pub fn get(&self, mode: WorkspaceMode) -> Option<Entity> {
        match mode {
            WorkspaceMode::TwoD => self.two_d,
            WorkspaceMode::ThreeD => self.three_d,
        }
    }

    pub fn set(&mut self, mode: WorkspaceMode, entity: Option<Entity>) {
        match mode {
            WorkspaceMode::TwoD => self.two_d = entity,
            WorkspaceMode::ThreeD => self.three_d = entity,
        }
    }
}

/// Marks a scene object as belonging to the 2D or 3D workspace.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneSpace {
    TwoD,
    ThreeD,
}

/// Clickable top-bar workspace tab.
#[derive(Component, Clone, Copy, Default)]
pub struct WorkspaceTab(pub WorkspaceMode);

/// Visual chrome for a workspace tab.
#[derive(Component, Clone, Copy, Default)]
pub struct WorkspaceTabChrome {
    pub mode: WorkspaceMode,
}

/// Clickable editor view in the centered main toolbar navigation.
#[derive(Component, Clone, Copy, Default)]
pub struct EditorViewTab(pub EditorViewMode);

/// Visual chrome for a main toolbar editor view.
#[derive(Component, Clone, Copy, Default)]
pub struct EditorViewTabChrome(pub EditorViewMode);

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorViewPanel {
    #[default]
    Scene,
    Game,
    AssetStore,
}

/// Non-editable helpers only shown in 3D (ground, lights).
#[derive(Component)]
pub struct SceneOnly3d;

/// Non-editable helpers only shown in 2D.
#[derive(Component)]
pub struct SceneOnly2d;

pub struct WorkspacePlugin;

impl Plugin for WorkspacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorkspaceMode>()
            .init_resource::<EditorViewMode>()
            .init_resource::<WorkspaceSelections>()
            .add_observer(handle_workspace_tab_activation)
            .add_observer(handle_editor_view_activation)
            .add_systems(
                Update,
                (
                    remember_workspace_selection,
                    sync_gizmo_focus,
                    sync_workspace_tab_styles,
                    sync_editor_view_tab_styles,
                    sync_editor_view_panels,
                    apply_workspace_visibility,
                )
                    .chain(),
            );
    }
}

fn handle_workspace_tab_activation(
    activate: On<Activate>,
    tabs: Query<&WorkspaceTab>,
    mut mode: ResMut<WorkspaceMode>,
    mut view: ResMut<EditorViewMode>,
    mut selection: ResMut<Selection>,
    mut saved_selections: ResMut<WorkspaceSelections>,
    spaces: Query<&SceneSpace>,
) {
    let Ok(tab) = tabs.get(activate.entity) else {
        return;
    };
    if *mode == tab.0 {
        return;
    }
    saved_selections.set(*mode, selection.0);
    *mode = tab.0;
    *view = match tab.0 {
        WorkspaceMode::TwoD => EditorViewMode::TwoD,
        WorkspaceMode::ThreeD => EditorViewMode::ThreeD,
    };

    selection.0 = saved_selections.get(tab.0).filter(|entity| {
        spaces
            .get(*entity)
            .is_ok_and(|space| matches_mode(tab.0, *space))
    });
}

fn handle_editor_view_activation(
    activate: On<Activate>,
    tabs: Query<&EditorViewTab>,
    mut view: ResMut<EditorViewMode>,
    mut mode: ResMut<WorkspaceMode>,
    mut selection: ResMut<Selection>,
    mut saved_selections: ResMut<WorkspaceSelections>,
    spaces: Query<&SceneSpace>,
) {
    let Ok(tab) = tabs.get(activate.entity) else {
        return;
    };
    if *view == tab.0 {
        return;
    }
    *view = tab.0;

    let Some(next_mode) = tab.0.workspace_mode() else {
        return;
    };
    if *mode == next_mode {
        return;
    }
    saved_selections.set(*mode, selection.0);
    *mode = next_mode;
    selection.0 = saved_selections.get(next_mode).filter(|entity| {
        spaces
            .get(*entity)
            .is_ok_and(|space| matches_mode(next_mode, *space))
    });
}

fn remember_workspace_selection(
    mode: Res<WorkspaceMode>,
    selection: Res<Selection>,
    mut saved_selections: ResMut<WorkspaceSelections>,
) {
    if selection.is_changed() {
        saved_selections.set(*mode, selection.0);
    }
}

fn sync_gizmo_focus(
    mut commands: Commands,
    mode: Res<WorkspaceMode>,
    selection: Res<Selection>,
    focused: Query<Entity, With<TransformGizmoFocus>>,
) {
    if !mode.is_changed() && !selection.is_changed() {
        return;
    }

    for entity in &focused {
        commands.entity(entity).remove::<TransformGizmoFocus>();
    }

    if *mode == WorkspaceMode::ThreeD
        && let Some(entity) = selection.0
    {
        commands.entity(entity).insert(TransformGizmoFocus);
    }
}

fn matches_mode(mode: WorkspaceMode, space: SceneSpace) -> bool {
    matches!(
        (mode, space),
        (WorkspaceMode::TwoD, SceneSpace::TwoD) | (WorkspaceMode::ThreeD, SceneSpace::ThreeD)
    )
}

fn sync_workspace_tab_styles(
    mode: Res<WorkspaceMode>,
    mut tabs: Query<(
        &WorkspaceTabChrome,
        &mut BorderColor,
        &mut BackgroundColor,
        &Children,
    )>,
    mut texts: Query<&mut TextColor>,
) {
    if !mode.is_changed() {
        return;
    }
    for (chrome, mut border, mut background, children) in &mut tabs {
        let active = chrome.mode == *mode;
        *border = BorderColor::all(if active { theme::accent() } else { Color::NONE });
        background.0 = if active {
            theme::bg_field()
        } else {
            Color::NONE
        };
        for child in children {
            if let Ok(mut color) = texts.get_mut(*child) {
                color.0 = if active {
                    theme::text_primary()
                } else {
                    theme::text_muted()
                };
            }
        }
    }
}

fn sync_editor_view_tab_styles(
    view: Res<EditorViewMode>,
    mut tabs: Query<(
        &EditorViewTabChrome,
        &mut BorderColor,
        &mut BackgroundColor,
        &Children,
    )>,
    mut texts: Query<&mut TextColor>,
) {
    if !view.is_changed() {
        return;
    }
    for (chrome, mut border, mut background, children) in &mut tabs {
        let active = chrome.0 == *view;
        *border = BorderColor::all(if active { theme::accent() } else { Color::NONE });
        background.0 = if active {
            theme::bg_panel()
        } else {
            Color::NONE
        };
        for child in children {
            if let Ok(mut color) = texts.get_mut(*child) {
                color.0 = if active {
                    theme::text_primary()
                } else {
                    theme::text_muted()
                };
            }
        }
    }
}

fn sync_editor_view_panels(
    view: Res<EditorViewMode>,
    mut panels: Query<(&EditorViewPanel, &mut Node)>,
) {
    if !view.is_changed() {
        return;
    }
    for (panel, mut node) in &mut panels {
        let visible = matches!(
            (*panel, *view),
            (
                EditorViewPanel::Scene,
                EditorViewMode::TwoD | EditorViewMode::ThreeD
            ) | (EditorViewPanel::Game, EditorViewMode::Game)
                | (EditorViewPanel::AssetStore, EditorViewMode::AssetStore)
        );
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
}

// Bevy system parameters encode disjoint ECS access in their types.
#[allow(clippy::type_complexity)]
fn apply_workspace_visibility(
    mode: Res<WorkspaceMode>,
    view: Res<EditorViewMode>,
    mut vis_set: ParamSet<(
        Query<(&SceneSpace, &mut Visibility), With<EditableObject>>,
        Query<
            &mut Visibility,
            (
                With<SceneOnly3d>,
                Without<EditableObject>,
                Without<SceneOnly2d>,
            ),
        >,
        Query<
            &mut Visibility,
            (
                With<SceneOnly2d>,
                Without<EditableObject>,
                Without<SceneOnly3d>,
            ),
        >,
    )>,
) {
    if !mode.is_changed() && !view.is_changed() {
        return;
    }
    let show_3d = *view == EditorViewMode::ThreeD;
    let show_2d = *view == EditorViewMode::TwoD;

    for (space, mut vis) in &mut vis_set.p0() {
        *vis = if matches_mode(*mode, *space) && (show_2d || show_3d) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    for mut vis in &mut vis_set.p1() {
        *vis = if show_3d {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for mut vis in &mut vis_set.p2() {
        *vis = if show_2d {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_selections_are_independent() {
        let entity_2d = Entity::from_raw_u32(2).unwrap();
        let entity_3d = Entity::from_raw_u32(3).unwrap();
        let mut selections = WorkspaceSelections::default();

        selections.set(WorkspaceMode::TwoD, Some(entity_2d));
        selections.set(WorkspaceMode::ThreeD, Some(entity_3d));

        assert_eq!(selections.get(WorkspaceMode::TwoD), Some(entity_2d));
        assert_eq!(selections.get(WorkspaceMode::ThreeD), Some(entity_3d));
    }

    #[test]
    fn activating_tabs_restores_each_workspaces_selection() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .add_plugins(WorkspacePlugin);
        *app.world_mut().resource_mut::<WorkspaceMode>() = WorkspaceMode::ThreeD;

        let entity_2d = app.world_mut().spawn(SceneSpace::TwoD).id();
        let entity_3d = app.world_mut().spawn(SceneSpace::ThreeD).id();
        let tab_2d = app
            .world_mut()
            .spawn(WorkspaceTab(WorkspaceMode::TwoD))
            .id();
        let tab_3d = app
            .world_mut()
            .spawn(WorkspaceTab(WorkspaceMode::ThreeD))
            .id();

        app.world_mut().resource_mut::<Selection>().0 = Some(entity_3d);
        {
            let mut selections = app.world_mut().resource_mut::<WorkspaceSelections>();
            selections.set(WorkspaceMode::TwoD, Some(entity_2d));
            selections.set(WorkspaceMode::ThreeD, Some(entity_3d));
        }

        app.world_mut().trigger(Activate { entity: tab_2d });
        assert_eq!(
            *app.world().resource::<WorkspaceMode>(),
            WorkspaceMode::TwoD
        );
        assert_eq!(app.world().resource::<Selection>().0, Some(entity_2d));

        app.world_mut().trigger(Activate { entity: tab_3d });
        assert_eq!(
            *app.world().resource::<WorkspaceMode>(),
            WorkspaceMode::ThreeD
        );
        assert_eq!(app.world().resource::<Selection>().0, Some(entity_3d));
    }

    #[test]
    fn main_editor_views_switch_scene_dimension_without_losing_selection() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .add_plugins(WorkspacePlugin);

        let entity_2d = app.world_mut().spawn(SceneSpace::TwoD).id();
        let entity_3d = app.world_mut().spawn(SceneSpace::ThreeD).id();
        let tab_3d = app
            .world_mut()
            .spawn(EditorViewTab(EditorViewMode::ThreeD))
            .id();
        let tab_game = app
            .world_mut()
            .spawn(EditorViewTab(EditorViewMode::Game))
            .id();
        {
            let mut selections = app.world_mut().resource_mut::<WorkspaceSelections>();
            selections.set(WorkspaceMode::TwoD, Some(entity_2d));
            selections.set(WorkspaceMode::ThreeD, Some(entity_3d));
        }
        app.world_mut().resource_mut::<Selection>().0 = Some(entity_2d);

        app.world_mut().trigger(Activate { entity: tab_3d });
        assert_eq!(
            *app.world().resource::<EditorViewMode>(),
            EditorViewMode::ThreeD
        );
        assert_eq!(
            *app.world().resource::<WorkspaceMode>(),
            WorkspaceMode::ThreeD
        );
        assert_eq!(app.world().resource::<Selection>().0, Some(entity_3d));

        app.world_mut().trigger(Activate { entity: tab_game });
        assert_eq!(
            *app.world().resource::<EditorViewMode>(),
            EditorViewMode::Game
        );
        assert_eq!(
            *app.world().resource::<WorkspaceMode>(),
            WorkspaceMode::ThreeD
        );
        assert_eq!(app.world().resource::<Selection>().0, Some(entity_3d));
    }
}
