//! Resizable dock layout used by the editor shell.

use bevy::{
    feathers::cursor::{EntityCursor, OverrideCursor},
    prelude::*,
    ui_widgets::Activate,
    window::SystemCursorIcon,
};

use crate::ui::theme;

/// Widths for the editor side panels that users can resize.
#[derive(Resource, Debug, Clone)]
pub struct PanelLayout {
    pub scene_width: f32,
    pub details_width: f32,
    pub filesystem_height: f32,
    pub bottom_dock_height: f32,
    pub viewport_preview_width: f32,
    pub viewport_preview_split: f32,
    pub viewport_mode: ViewportLayoutMode,
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            scene_width: theme::SCENE_PANEL_WIDTH,
            details_width: theme::DETAILS_PANEL_WIDTH,
            filesystem_height: theme::FILESYSTEM_PANEL_HEIGHT,
            bottom_dock_height: 250.0,
            viewport_preview_width: 330.0,
            viewport_preview_split: 0.5,
            viewport_mode: ViewportLayoutMode::SingleViewport,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BottomDockTab {
    Output,
    Debugger,
    #[default]
    Animation,
}

#[derive(Resource, Debug, Clone)]
pub struct BottomDockState {
    pub active: BottomDockTab,
    pub open: bool,
}

impl Default for BottomDockState {
    fn default() -> Self {
        Self {
            active: BottomDockTab::Animation,
            open: true,
        }
    }
}

impl BottomDockState {
    fn activate(&mut self, tab: BottomDockTab) {
        if self.active == tab {
            self.open = !self.open;
        } else {
            self.active = tab;
            self.open = true;
        }
    }

    pub fn show(&mut self, tab: BottomDockTab) {
        self.active = tab;
        self.open = true;
    }
}

#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ViewportLayout {
    pub mode: ViewportLayoutMode,
    pub preview_width: f32,
    pub preview_split: f32,
}

impl Default for ViewportLayout {
    fn default() -> Self {
        Self {
            mode: ViewportLayoutMode::SingleViewport,
            preview_width: 330.0,
            preview_split: 0.5,
        }
    }
}

impl ViewportLayout {
    pub fn reset_to_default(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewportLayoutMode {
    #[default]
    SingleViewport,
    ThreeViewport,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum SplitterKind {
    SceneHorizontal,
    DetailsHorizontal,
    FileSystemVertical,
    BottomDockVertical,
    ViewportSideHorizontal,
    ViewportPreviewVertical,
}

impl SplitterKind {
    fn cursor_icon(self) -> SystemCursorIcon {
        match self {
            Self::SceneHorizontal | Self::DetailsHorizontal | Self::ViewportSideHorizontal => {
                SystemCursorIcon::EwResize
            }
            Self::FileSystemVertical | Self::BottomDockVertical | Self::ViewportPreviewVertical => {
                SystemCursorIcon::NsResize
            }
        }
    }

    fn apply_delta(self, layout: &mut PanelLayout, delta: Vec2) {
        match self {
            Self::SceneHorizontal => {
                layout.scene_width = (layout.scene_width + delta.x).clamp(220.0, 560.0);
            }
            Self::DetailsHorizontal => {
                layout.details_width = (layout.details_width - delta.x).clamp(260.0, 680.0);
            }
            Self::FileSystemVertical => {
                layout.filesystem_height = (layout.filesystem_height - delta.y).clamp(140.0, 520.0);
            }
            Self::BottomDockVertical => {
                layout.bottom_dock_height =
                    (layout.bottom_dock_height - delta.y).clamp(170.0, 520.0);
            }
            Self::ViewportSideHorizontal => {
                layout.viewport_mode = ViewportLayoutMode::ThreeViewport;
                layout.viewport_preview_width =
                    (layout.viewport_preview_width - delta.x).clamp(220.0, 720.0);
            }
            Self::ViewportPreviewVertical => {
                layout.viewport_mode = ViewportLayoutMode::ThreeViewport;
                layout.viewport_preview_split =
                    (layout.viewport_preview_split + delta.y / 500.0).clamp(0.22, 0.78);
            }
        }
    }
}

#[derive(Component, Clone, Copy, Default)]
pub struct ScenePanel;

#[derive(Component, Clone, Copy, Default)]
pub struct DetailsPanel;

#[derive(Component, Clone, Copy, Default)]
pub struct FileSystemPanel;

#[derive(Component, Clone, Copy, Default)]
pub struct OutputPanel;

#[derive(Component, Clone, Copy, Default)]
pub struct BottomDockPanel;

#[derive(Component, Clone, Copy, Default)]
pub struct BottomDockSplitter;

#[derive(Component, Clone, Copy, Default)]
pub struct BottomDockContent(pub BottomDockTab);

#[derive(Component, Clone, Copy, Default)]
pub struct BottomDockTabButton(pub BottomDockTab);

#[derive(Component, Clone, Copy, Default)]
pub struct BottomDockTabLabel(pub BottomDockTab);

#[derive(Component, Clone, Copy, Default)]
pub struct ViewportPreviewColumn;

#[derive(Component, Clone, Copy, Default)]
pub struct ViewportPreviewTop;

#[derive(Component, Clone, Copy, Default)]
pub struct ViewportPreviewBottom;

#[derive(Component, Clone, Copy, Default)]
pub struct ViewportSideSplitter;

#[derive(Component, Clone, Copy, Default)]
pub struct ViewportPreviewSplitter;

/// Marker retained by the content browser implementation.
#[derive(Component, Clone, Copy, Default)]
pub struct FileSystemSection;

pub struct PanelsPlugin;

impl Plugin for PanelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanelLayout>()
            .init_resource::<ViewportLayout>()
            .init_resource::<BottomDockState>()
            .add_observer(activate_bottom_dock_tab)
            .add_systems(
                Update,
                (
                    apply_scene_panel_layout,
                    apply_details_panel_layout,
                    apply_filesystem_panel_layout,
                    apply_bottom_dock_layout,
                    sync_bottom_dock,
                    sync_viewport_layout_resource,
                    apply_viewport_layout,
                ),
            );
    }
}

fn activate_bottom_dock_tab(
    activate: On<Activate>,
    buttons: Query<&BottomDockTabButton>,
    mut state: ResMut<BottomDockState>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        state.activate(button.0);
    }
}

/// Turns a BSN host node into a draggable splitter without rebuilding layout.
pub fn attach_splitter(commands: &mut Commands, entity: Entity, kind: SplitterKind) {
    commands
        .entity(entity)
        .insert((kind, Button, EntityCursor::System(kind.cursor_icon())))
        .observe(on_splitter_drag_start)
        .observe(on_splitter_drag)
        .observe(on_splitter_drag_end)
        .observe(on_splitter_over)
        .observe(on_splitter_out);
}

fn on_splitter_drag_start(
    drag: On<Pointer<DragStart>>,
    kinds: Query<&SplitterKind>,
    mut cursor: ResMut<OverrideCursor>,
) {
    let Ok(kind) = kinds.get(drag.entity) else {
        return;
    };
    cursor.0 = Some(EntityCursor::System(kind.cursor_icon()));
}

fn on_splitter_drag_end(_drag: On<Pointer<DragEnd>>, mut cursor: ResMut<OverrideCursor>) {
    cursor.0 = None;
}

fn on_splitter_over(
    over: On<Pointer<Over>>,
    mut backgrounds: Query<&mut BackgroundColor, With<SplitterKind>>,
) {
    if let Ok(mut background) = backgrounds.get_mut(over.entity) {
        *background = BackgroundColor(theme::accent());
    }
}

fn on_splitter_out(
    out: On<Pointer<Out>>,
    mut backgrounds: Query<&mut BackgroundColor, With<SplitterKind>>,
) {
    if let Ok(mut background) = backgrounds.get_mut(out.entity) {
        *background = BackgroundColor(theme::border());
    }
}

fn on_splitter_drag(
    drag: On<Pointer<Drag>>,
    kinds: Query<&SplitterKind>,
    mut layout: ResMut<PanelLayout>,
    mut viewport_layout: ResMut<ViewportLayout>,
) {
    let Ok(kind) = kinds.get(drag.entity) else {
        return;
    };

    kind.apply_delta(&mut layout, drag.delta);
    if matches!(
        *kind,
        SplitterKind::ViewportSideHorizontal | SplitterKind::ViewportPreviewVertical
    ) {
        viewport_layout.mode = layout.viewport_mode;
        viewport_layout.preview_width = layout.viewport_preview_width;
        viewport_layout.preview_split = layout.viewport_preview_split;
    }
}

fn apply_scene_panel_layout(
    layout: Res<PanelLayout>,
    mut panels: Query<&mut Node, With<ScenePanel>>,
) {
    if !layout.is_changed() {
        return;
    }

    for mut node in &mut panels {
        node.width = Val::Px(layout.scene_width);
    }
}

fn apply_details_panel_layout(
    layout: Res<PanelLayout>,
    mut panels: Query<&mut Node, With<DetailsPanel>>,
) {
    if !layout.is_changed() {
        return;
    }

    for mut node in &mut panels {
        node.width = Val::Px(layout.details_width);
    }
}

fn apply_filesystem_panel_layout(
    layout: Res<PanelLayout>,
    mut panels: Query<&mut Node, With<FileSystemPanel>>,
) {
    if !layout.is_changed() {
        return;
    }

    for mut node in &mut panels {
        node.height = Val::Px(layout.filesystem_height);
    }
}

fn apply_bottom_dock_layout(
    layout: Res<PanelLayout>,
    mut panels: Query<&mut Node, With<BottomDockPanel>>,
) {
    if !layout.is_changed() {
        return;
    }

    for mut node in &mut panels {
        node.height = Val::Px(layout.bottom_dock_height);
    }
}

#[allow(clippy::type_complexity)]
fn sync_bottom_dock(
    state: Res<BottomDockState>,
    mut panels: Query<&mut Node, (With<BottomDockPanel>, Without<BottomDockContent>)>,
    mut splitters: Query<&mut Node, (With<BottomDockSplitter>, Without<BottomDockPanel>)>,
    mut contents: Query<
        (&BottomDockContent, &mut Node),
        (Without<BottomDockPanel>, Without<BottomDockSplitter>),
    >,
    mut buttons: Query<
        (&BottomDockTabButton, &mut BackgroundColor, &mut BorderColor),
        Without<BottomDockContent>,
    >,
    mut labels: Query<(&BottomDockTabLabel, &mut TextColor)>,
) {
    if !state.is_changed() {
        return;
    }

    let dock_display = if state.open {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut panels {
        node.display = dock_display;
    }
    for mut node in &mut splitters {
        node.display = dock_display;
    }
    for (content, mut node) in &mut contents {
        node.display = if state.open && content.0 == state.active {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (button, mut background, mut border) in &mut buttons {
        let active = state.active == button.0;
        *background = BackgroundColor(if active {
            theme::bg_selected()
        } else {
            Color::NONE
        });
        *border = BorderColor::all(if active { theme::accent() } else { Color::NONE });
    }
    for (label, mut color) in &mut labels {
        *color = TextColor(if state.active == label.0 {
            theme::text_primary()
        } else {
            theme::text_muted()
        });
    }
}

fn sync_viewport_layout_resource(
    viewport_layout: Res<ViewportLayout>,
    mut panel_layout: ResMut<PanelLayout>,
) {
    if !viewport_layout.is_changed() {
        return;
    }
    panel_layout.viewport_mode = viewport_layout.mode;
    panel_layout.viewport_preview_width = viewport_layout.preview_width;
    panel_layout.viewport_preview_split = viewport_layout.preview_split;
}

#[allow(clippy::type_complexity)]
fn apply_viewport_layout(
    layout: Res<PanelLayout>,
    mut columns: Query<&mut Node, With<ViewportPreviewColumn>>,
    mut top_panes: Query<
        &mut Node,
        (
            With<ViewportPreviewTop>,
            Without<ViewportPreviewBottom>,
            Without<ViewportPreviewColumn>,
            Without<ViewportSideSplitter>,
            Without<ViewportPreviewSplitter>,
        ),
    >,
    mut bottom_panes: Query<
        &mut Node,
        (
            With<ViewportPreviewBottom>,
            Without<ViewportPreviewTop>,
            Without<ViewportPreviewColumn>,
            Without<ViewportSideSplitter>,
            Without<ViewportPreviewSplitter>,
        ),
    >,
    mut side_splitters: Query<
        &mut Node,
        (
            With<ViewportSideSplitter>,
            Without<ViewportPreviewSplitter>,
            Without<ViewportPreviewColumn>,
            Without<ViewportPreviewTop>,
            Without<ViewportPreviewBottom>,
        ),
    >,
    mut preview_splitters: Query<
        &mut Node,
        (
            With<ViewportPreviewSplitter>,
            Without<ViewportSideSplitter>,
            Without<ViewportPreviewColumn>,
            Without<ViewportPreviewTop>,
            Without<ViewportPreviewBottom>,
        ),
    >,
) {
    if !layout.is_changed() {
        return;
    }

    let show_preview = layout.viewport_mode == ViewportLayoutMode::ThreeViewport;
    for mut node in &mut columns {
        node.display = if show_preview {
            Display::Flex
        } else {
            Display::None
        };
        node.width = Val::Px(layout.viewport_preview_width);
    }
    for mut node in &mut side_splitters {
        node.display = if show_preview {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut preview_splitters {
        node.display = if show_preview {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut top_panes {
        node.flex_grow = layout.viewport_preview_split;
    }
    for mut node in &mut bottom_panes {
        node.flex_grow = 1.0 - layout.viewport_preview_split;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_dock_splitters_resize_independently() {
        let mut layout = PanelLayout::default();
        layout.filesystem_height = 300.0;
        layout.bottom_dock_height = 300.0;
        let original_filesystem_height = layout.filesystem_height;
        let original_bottom_dock_height = layout.bottom_dock_height;

        SplitterKind::FileSystemVertical.apply_delta(&mut layout, Vec2::new(0.0, -40.0));
        assert_eq!(layout.filesystem_height, original_filesystem_height + 40.0);
        assert_eq!(layout.bottom_dock_height, original_bottom_dock_height);

        SplitterKind::BottomDockVertical.apply_delta(&mut layout, Vec2::new(0.0, 30.0));
        assert_eq!(layout.filesystem_height, original_filesystem_height + 40.0);
        assert_eq!(
            layout.bottom_dock_height,
            original_bottom_dock_height - 30.0
        );
    }

    #[test]
    fn lower_dock_splitters_respect_height_limits() {
        let mut layout = PanelLayout::default();

        SplitterKind::FileSystemVertical.apply_delta(&mut layout, Vec2::new(0.0, -1000.0));
        SplitterKind::BottomDockVertical.apply_delta(&mut layout, Vec2::new(0.0, 1000.0));

        assert_eq!(layout.filesystem_height, 520.0);
        assert_eq!(layout.bottom_dock_height, 170.0);
    }

    #[test]
    fn bottom_dock_defaults_to_animation_and_active_tab_toggles_visibility() {
        let mut state = BottomDockState::default();
        assert_eq!(state.active, BottomDockTab::Animation);
        assert!(state.open);

        state.activate(BottomDockTab::Animation);
        assert!(!state.open);

        state.activate(BottomDockTab::Output);
        assert_eq!(state.active, BottomDockTab::Output);
        assert!(state.open);
    }

    #[test]
    fn viewport_splitters_enable_and_resize_three_viewport_layout() {
        let mut layout = PanelLayout::default();

        SplitterKind::ViewportSideHorizontal.apply_delta(&mut layout, Vec2::new(-50.0, 0.0));
        assert_eq!(layout.viewport_mode, ViewportLayoutMode::ThreeViewport);
        assert_eq!(layout.viewport_preview_width, 380.0);

        SplitterKind::ViewportPreviewVertical.apply_delta(&mut layout, Vec2::new(0.0, 100.0));
        assert_eq!(layout.viewport_preview_split, 0.7);
    }

    #[test]
    fn default_viewport_layout_is_single_viewport() {
        let mut layout = ViewportLayout {
            mode: ViewportLayoutMode::ThreeViewport,
            preview_width: 500.0,
            preview_split: 0.7,
        };

        layout.reset_to_default();

        assert_eq!(layout.mode, ViewportLayoutMode::SingleViewport);
        assert_eq!(layout.preview_width, 330.0);
        assert_eq!(layout.preview_split, 0.5);
    }
}
