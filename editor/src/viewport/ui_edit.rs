use std::collections::{HashMap, HashSet};

use arisna_engine::{scene_image_asset_path, scene_ui_transform};
use bevy::{
    camera::Projection,
    feathers::cursor::EntityCursor,
    gizmos::transform_gizmo::{TransformGizmoMode, TransformGizmoSettings},
    picking::{Pickable, pointer::PointerButton},
    prelude::*,
    ui::{UiGlobalTransform, UiSystems, UiTargetCamera, UiTransform, Val2},
    window::SystemCursorIcon,
};

use crate::{
    entities::{EntityKind, SceneUiContent, SceneUiLayout, UiAlignment},
    hierarchy::{SceneNodeId, SceneParentId, SceneSiblingOrder},
    scene::SceneDocument,
    selection::{Selection, SelectionSet},
    undo::{SceneHistory, SceneSnapshot, SceneSnapshotQuery, capture_scene_snapshot},
    workspace::{EditorViewMode, SceneSpace, WorkspaceMode},
};

use super::{
    EditorSceneCamera2d, MainViewportCamera, PrimaryPointerOwner, PrimaryPointerOwnership,
    navigation::{CameraNavigation2d, ViewportNavigationState},
    snap::{Snap2dSettings, shift_pressed, smart_snap_2d, snap_scalar},
};

const SELECTION_COLOR: Color = Color::srgb(0.09, 0.61, 0.82);
const X_AXIS_COLOR: Color = Color::srgb(0.96, 0.24, 0.31);
const Y_AXIS_COLOR: Color = Color::srgb(0.42, 0.78, 0.20);
const HANDLE_SIZE: f32 = 8.0;

#[derive(Component)]
pub(super) struct UiPreviewCanvas;

/// A world-aligned coordinate layer whose top-left corner is UI position `(0, 0)`.
#[derive(Component)]
struct UiPreviewContent;

#[derive(Component, Clone)]
pub(super) struct UiPreviewNode {
    pub(super) source: Entity,
    data: UiPreviewData,
    parent_target: Entity,
}

/// Marks UI preview geometry that owns primary-pointer editing over 2D sprites.
#[derive(Component)]
pub(super) struct UiEditorHitTarget;

#[derive(Clone, Debug, PartialEq)]
struct UiPreviewData {
    id: SceneNodeId,
    parent: Option<SceneNodeId>,
    order: u32,
    kind: EntityKind,
    layout: SceneUiLayout,
    content: SceneUiContent,
}

#[derive(Resource, Default)]
struct UiPreviewRegistry {
    nodes: HashMap<Entity, Entity>,
}

#[derive(Component)]
struct UiSelectionOverlay;

#[derive(Component)]
struct UiSecondarySelectionOverlay {
    source: Entity,
}

#[derive(Component)]
pub(super) struct UiSelectionMarquee;

#[derive(Component)]
struct UiMoveGizmo;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum UiSnapGuideAxis {
    X,
    Y,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum UiMoveAxis {
    X,
    Y,
}

impl UiMoveAxis {
    fn constrain(self, delta: Vec2) -> Vec2 {
        match self {
            Self::X => Vec2::new(delta.x, 0.0),
            Self::Y => Vec2::new(0.0, delta.y),
        }
    }
}

#[derive(Component, Clone, Copy)]
struct UiAnchorIndicator(u8);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum UiResizeHandle {
    NorthWest,
    North,
    NorthEast,
    West,
    East,
    SouthWest,
    South,
    SouthEast,
}

impl UiResizeHandle {
    const ALL: [Self; 8] = [
        Self::NorthWest,
        Self::North,
        Self::NorthEast,
        Self::West,
        Self::East,
        Self::SouthWest,
        Self::South,
        Self::SouthEast,
    ];

    const fn west(self) -> bool {
        matches!(self, Self::NorthWest | Self::West | Self::SouthWest)
    }

    const fn east(self) -> bool {
        matches!(self, Self::NorthEast | Self::East | Self::SouthEast)
    }

    const fn north(self) -> bool {
        matches!(self, Self::NorthWest | Self::North | Self::NorthEast)
    }

    const fn south(self) -> bool {
        matches!(self, Self::SouthWest | Self::South | Self::SouthEast)
    }

    const fn cursor(self) -> SystemCursorIcon {
        match self {
            Self::North | Self::South => SystemCursorIcon::NsResize,
            Self::West | Self::East => SystemCursorIcon::EwResize,
            Self::NorthWest | Self::SouthEast => SystemCursorIcon::NwseResize,
            Self::NorthEast | Self::SouthWest => SystemCursorIcon::NeswResize,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum UiEditOperation {
    Move {
        source: Entity,
        start_layout: SceneUiLayout,
        axis: Option<UiMoveAxis>,
    },
    Resize {
        source: Entity,
        handle: UiResizeHandle,
        start_layout: SceneUiLayout,
        start_rendered_size: Vec2,
    },
}

#[derive(Resource, Default)]
pub(super) struct UiEditState {
    operation: Option<UiEditOperation>,
    primary_pointer_captured: bool,
    before: Option<SceneSnapshot>,
    snap_context: Option<UiSnapContext>,
    guides: UiSnapGuides,
    group_move: Vec<UiGroupMove>,
    skip_primary_move: bool,
}

impl UiEditState {
    pub(super) fn is_active(&self) -> bool {
        self.primary_pointer_captured || self.operation.is_some()
    }
}

#[derive(Clone, Copy, Debug)]
struct UiGroupMove {
    source: Entity,
    start_layout: SceneUiLayout,
    screen_to_layout: Vec2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct UiSnapGuides {
    x: Option<f32>,
    y: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct UiScreenBounds {
    min: Vec2,
    max: Vec2,
}

impl UiScreenBounds {
    fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    fn x_values(self) -> [f32; 3] {
        [self.min.x, (self.min.x + self.max.x) * 0.5, self.max.x]
    }

    fn y_values(self) -> [f32; 3] {
        [self.min.y, (self.min.y + self.max.y) * 0.5, self.max.y]
    }

    fn translated(self, delta: Vec2) -> Self {
        Self {
            min: self.min + delta,
            max: self.max + delta,
        }
    }

    fn resized(self, handle: UiResizeHandle, delta: Vec2) -> Self {
        let mut next = self;
        if handle.west() {
            next.min.x = (next.min.x + delta.x).min(next.max.x - 1.0);
        } else if handle.east() {
            next.max.x = (next.max.x + delta.x).max(next.min.x + 1.0);
        }
        if handle.north() {
            next.min.y = (next.min.y + delta.y).min(next.max.y - 1.0);
        } else if handle.south() {
            next.max.y = (next.max.y + delta.y).max(next.min.y + 1.0);
        }
        next
    }
}

#[derive(Debug)]
struct UiSnapContext {
    start_bounds: UiScreenBounds,
    target_x: Vec<f32>,
    target_y: Vec<f32>,
    grid_origin: Vec2,
    grid_to_screen: Vec2,
    screen_to_layout: Vec2,
}

pub(super) struct UiEditorPlugin;

impl Plugin for UiEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiPreviewRegistry>()
            .init_resource::<UiEditState>()
            .init_resource::<SelectionSet>()
            .init_resource::<ViewportNavigationState>()
            .init_resource::<PrimaryPointerOwnership>()
            .init_resource::<TransformGizmoSettings>()
            .add_systems(
                Update,
                (
                    (sync_ui_preview_nodes, sync_ui_canvas_visibility).chain(),
                    sync_ui_preview_content_transform.after(CameraNavigation2d),
                    cancel_ui_edit_during_navigation.after(CameraNavigation2d),
                    release_ui_pointer_capture.after(CameraNavigation2d),
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    sync_ui_selection_overlay.after(UiSystems::Layout),
                    sync_ui_secondary_selection_overlays.after(sync_ui_selection_overlay),
                    sync_ui_move_gizmo_visibility.after(sync_ui_selection_overlay),
                    sync_ui_snap_guides.after(sync_ui_selection_overlay),
                ),
            );
    }
}

pub(super) fn spawn_ui_preview_canvas(commands: &mut Commands, camera: Entity) {
    let canvas = commands
        .spawn((
            UiPreviewCanvas,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                overflow: Overflow::clip(),
                ..default()
            },
            UiTargetCamera(camera),
            Pickable::IGNORE,
        ))
        .id();

    commands.entity(canvas).with_children(|parent| {
        parent.spawn((
            UiPreviewContent,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            UiTransform::default(),
            Pickable::IGNORE,
        ));

        parent
            .spawn((
                UiSelectionOverlay,
                UiEditorHitTarget,
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(SELECTION_COLOR),
                BackgroundColor(Color::NONE),
                GlobalZIndex(100_000),
                Pickable::default(),
            ))
            .observe(capture_ui_primary_pointer)
            .observe(begin_ui_move_drag)
            .observe(move_selected_ui)
            .observe(finish_ui_edit_drag)
            .with_children(|overlay| {
                for handle in UiResizeHandle::ALL {
                    overlay
                        .spawn((
                            handle,
                            UiEditorHitTarget,
                            resize_handle_node(handle),
                            BackgroundColor(Color::srgb(0.88, 0.94, 0.98)),
                            BorderColor::all(SELECTION_COLOR),
                            EntityCursor::System(handle.cursor()),
                            Pickable::default(),
                        ))
                        .observe(capture_ui_primary_pointer)
                        .observe(begin_ui_resize_drag)
                        .observe(resize_selected_ui)
                        .observe(finish_ui_edit_drag);
                }

                overlay
                    .spawn((
                        UiMoveGizmo,
                        Node {
                            position_type: PositionType::Absolute,
                            display: Display::None,
                            left: Val::Px(-1.0),
                            top: Val::Px(-1.0),
                            width: Val::Px(1.0),
                            height: Val::Px(1.0),
                            overflow: Overflow::visible(),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|gizmo| {
                        spawn_ui_move_axis(gizmo, UiMoveAxis::X);
                        spawn_ui_move_axis(gizmo, UiMoveAxis::Y);
                    });
            });

        parent.spawn((
            UiSelectionMarquee,
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.09, 0.61, 0.82, 0.12)),
            BorderColor::all(Color::srgba(0.12, 0.82, 0.94, 0.95)),
            GlobalZIndex(100_001),
            Pickable::IGNORE,
        ));

        for index in 0..2 {
            parent.spawn((
                UiAnchorIndicator(index),
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    width: Val::Px(7.0),
                    height: Val::Px(7.0),
                    margin: UiRect::all(Val::Px(-3.5)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.12, 0.15, 0.18)),
                BorderColor::all(SELECTION_COLOR),
                GlobalZIndex(99_999),
                Pickable::IGNORE,
            ));
        }

        for axis in [UiSnapGuideAxis::X, UiSnapGuideAxis::Y] {
            parent.spawn((
                axis,
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.12, 0.82, 0.94, 0.95)),
                GlobalZIndex(99_998),
                Pickable::IGNORE,
            ));
        }
    });
}

fn spawn_ui_move_axis(parent: &mut ChildSpawnerCommands, axis: UiMoveAxis) {
    let (node, color) = match axis {
        UiMoveAxis::X => (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(5.0),
                top: Val::Px(-6.0),
                width: Val::Px(58.0),
                height: Val::Px(12.0),
                ..default()
            },
            X_AXIS_COLOR,
        ),
        UiMoveAxis::Y => (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(-6.0),
                top: Val::Px(5.0),
                width: Val::Px(12.0),
                height: Val::Px(58.0),
                ..default()
            },
            Y_AXIS_COLOR,
        ),
    };
    parent
        .spawn((
            axis,
            UiEditorHitTarget,
            node,
            EntityCursor::System(SystemCursorIcon::Move),
            Pickable::default(),
        ))
        .observe(capture_ui_primary_pointer)
        .observe(begin_ui_axis_move_drag)
        .observe(move_selected_ui)
        .observe(finish_ui_edit_drag)
        .with_children(|axis_root| {
            let (line, head_a, head_b) = match axis {
                UiMoveAxis::X => (
                    axis_segment(0.0, 5.0, 51.0, 2.0, 0.0),
                    axis_segment(45.0, 2.0, 10.0, 2.0, -45.0),
                    axis_segment(45.0, 8.0, 10.0, 2.0, 45.0),
                ),
                UiMoveAxis::Y => (
                    axis_segment(5.0, 0.0, 2.0, 51.0, 0.0),
                    axis_segment(2.0, 45.0, 10.0, 2.0, 45.0),
                    axis_segment(8.0, 45.0, 10.0, 2.0, -45.0),
                ),
            };
            for (node, transform) in [line, head_a, head_b] {
                axis_root.spawn((node, transform, BackgroundColor(color), Pickable::IGNORE));
            }
        });
}

fn axis_segment(
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    rotation_degrees: f32,
) -> (Node, UiTransform) {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(left),
            top: Val::Px(top),
            width: Val::Px(width),
            height: Val::Px(height),
            ..default()
        },
        UiTransform::from_rotation(Rot2::degrees(rotation_degrees)),
    )
}

fn sync_ui_preview_content_transform(
    camera: Query<(&Transform, &Projection), (With<EditorSceneCamera2d>, With<MainViewportCamera>)>,
    canvas: Query<&ComputedNode, With<UiPreviewCanvas>>,
    mut content: Query<&mut UiTransform, With<UiPreviewContent>>,
) {
    let (Ok((camera_transform, Projection::Orthographic(projection))), Ok(canvas), Ok(mut content)) =
        (camera.single(), canvas.single(), content.single_mut())
    else {
        return;
    };
    let canvas_size = canvas.size() * canvas.inverse_scale_factor();
    if canvas_size.min_element() < 1.0 {
        return;
    }

    let (translation, scale) = ui_preview_content_transform(
        canvas_size,
        camera_transform.translation.truncate(),
        projection.scale,
    );
    let next = UiTransform {
        translation: Val2::px(translation.x, translation.y),
        scale,
        ..default()
    };
    if *content != next {
        *content = next;
    }
}

/// Maps UI coordinates onto the 2D world: X grows right, Y grows down, and `(0, 0)`
/// stays on the grid origin while the camera pans and zooms.
fn ui_preview_content_transform(
    canvas_size: Vec2,
    camera_center: Vec2,
    world_units_per_pixel: f32,
) -> (Vec2, Vec2) {
    let pixels_per_world_unit = world_units_per_pixel.max(0.000_001).recip();
    let scale = Vec2::splat(pixels_per_world_unit);
    let camera_offset = Vec2::new(-camera_center.x, camera_center.y) * scale;
    (canvas_size * 0.5 * scale + camera_offset, scale)
}

fn sync_ui_canvas_visibility(
    mode: Res<WorkspaceMode>,
    view: Res<EditorViewMode>,
    mut canvases: Query<&mut Node, With<UiPreviewCanvas>>,
) {
    let display = if *mode == WorkspaceMode::TwoD && *view == EditorViewMode::TwoD {
        Display::Flex
    } else {
        Display::None
    };
    for mut canvas in &mut canvases {
        if canvas.display != display {
            canvas.display = display;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_ui_preview_nodes(
    mut commands: Commands,
    content_root: Query<Entity, With<UiPreviewContent>>,
    sources: Query<(
        Entity,
        &EntityKind,
        &SceneSpace,
        &SceneNodeId,
        &SceneParentId,
        &SceneSiblingOrder,
        &SceneUiLayout,
        &SceneUiContent,
    )>,
    previews: Query<&UiPreviewNode>,
    asset_server: Res<AssetServer>,
    mut registry: ResMut<UiPreviewRegistry>,
) {
    let Ok(content_root) = content_root.single() else {
        return;
    };
    let mut source_entities = HashSet::new();
    let mut id_to_preview = HashMap::new();
    let mut pending = Vec::new();

    for (source, kind, space, id, parent, order, layout, content) in &sources {
        if *space != SceneSpace::TwoD || !kind.is_ui() {
            continue;
        }
        source_entities.insert(source);
        let data = UiPreviewData {
            id: *id,
            parent: parent.0,
            order: order.0,
            kind: *kind,
            layout: *layout,
            content: content.clone(),
        };
        let preview = registry
            .nodes
            .get(&source)
            .copied()
            .filter(|preview| previews.get(*preview).is_ok())
            .unwrap_or_else(|| {
                let preview = commands.spawn_empty().id();
                commands
                    .entity(preview)
                    .insert(UiEditorHitTarget)
                    .observe(capture_ui_primary_pointer)
                    .observe(select_ui_preview)
                    .observe(begin_ui_move_drag)
                    .observe(move_selected_ui)
                    .observe(finish_ui_edit_drag);
                registry.nodes.insert(source, preview);
                preview
            });
        id_to_preview.insert(*id, preview);
        pending.push((source, preview, data));
    }

    let stale: Vec<_> = registry
        .nodes
        .iter()
        .filter(|(source, _)| !source_entities.contains(source))
        .map(|(source, preview)| (*source, *preview))
        .collect();
    for (source, preview) in stale {
        commands.entity(preview).remove::<ChildOf>().despawn();
        registry.nodes.remove(&source);
    }

    for (source, preview, data) in pending {
        let parent_target = data
            .parent
            .and_then(|parent| id_to_preview.get(&parent).copied())
            .unwrap_or(content_root);
        let cached = previews.get(preview).ok();
        if cached.is_none_or(|cached| cached.data != data) {
            apply_preview_style(&mut commands, preview, &data, &asset_server);
        }
        if cached.is_none_or(|cached| cached.parent_target != parent_target) {
            commands.entity(preview).insert(ChildOf(parent_target));
        }
        if cached.is_none_or(|cached| {
            cached.source != source || cached.data != data || cached.parent_target != parent_target
        }) {
            commands.entity(preview).insert(UiPreviewNode {
                source,
                data,
                parent_target,
            });
        }
    }
}

fn apply_preview_style(
    commands: &mut Commands,
    preview: Entity,
    data: &UiPreviewData,
    asset_server: &AssetServer,
) {
    let mut node = node_from_layout(data.layout);
    let (background, border) = match data.kind {
        EntityKind::EmptyUi => {
            node.border = UiRect::all(Val::Px(1.0));
            (
                Color::srgba(0.09, 0.61, 0.82, 0.05),
                Color::srgba(0.09, 0.61, 0.82, 0.45),
            )
        }
        EntityKind::Panel | EntityKind::Button | EntityKind::Image => (
            scene_color(data.content.panel_color),
            Color::srgba(0.0, 0.0, 0.0, 0.18),
        ),
        EntityKind::Text => (Color::NONE, Color::NONE),
        _ => (Color::NONE, Color::NONE),
    };
    commands.entity(preview).insert((
        node,
        scene_ui_transform(data.layout),
        BackgroundColor(background),
        BorderColor::all(border),
        ZIndex(data.order.min(i32::MAX as u32) as i32),
        Pickable::default(),
    ));
    commands
        .entity(preview)
        .remove::<Text>()
        .remove::<TextFont>()
        .remove::<TextColor>()
        .remove::<ImageNode>();

    match data.kind {
        EntityKind::Text | EntityKind::Button => {
            commands.entity(preview).insert((
                Text::new(data.content.text.clone()),
                TextFont {
                    font_size: FontSize::Px(if data.kind == EntityKind::Button {
                        16.0
                    } else {
                        18.0
                    }),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        }
        EntityKind::Image => {
            if let Some(path) = project_image_asset_path(&data.content.image_path) {
                commands
                    .entity(preview)
                    .insert(ImageNode::new(asset_server.load(path)));
            }
        }
        _ => {}
    }
}

fn apply_live_preview_layout(
    preview: &mut UiPreviewNode,
    node: &mut Node,
    transform: &mut UiTransform,
    layout: SceneUiLayout,
) {
    preview.data.layout = layout;
    let mut updated = node_from_layout(layout);
    if preview.data.kind == EntityKind::EmptyUi {
        updated.border = UiRect::all(Val::Px(1.0));
    }
    *node = updated;
    *transform = scene_ui_transform(layout);
}

fn node_from_layout(layout: SceneUiLayout) -> Node {
    let stretch_x = layout.anchor_max.0 > layout.anchor_min.0;
    let stretch_y = layout.anchor_max.1 > layout.anchor_min.1;
    Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(layout.anchor_min.0 * 100.0),
        top: Val::Percent(layout.anchor_min.1 * 100.0),
        right: if stretch_x {
            Val::Percent((1.0 - layout.anchor_max.0) * 100.0)
        } else {
            Val::Auto
        },
        bottom: if stretch_y {
            Val::Percent((1.0 - layout.anchor_max.1) * 100.0)
        } else {
            Val::Auto
        },
        width: if stretch_x {
            Val::Auto
        } else {
            Val::Px(layout.size.0.max(1.0))
        },
        height: if stretch_y {
            Val::Auto
        } else {
            Val::Px(layout.size.1.max(1.0))
        },
        min_width: Val::Px(layout.minimum_size.0.max(0.0)),
        min_height: Val::Px(layout.minimum_size.1.max(0.0)),
        margin: UiRect::new(
            Val::Px(layout.margin.0 + layout.offset.0),
            Val::Px(layout.margin.2),
            Val::Px(layout.margin.1 + layout.offset.1),
            Val::Px(layout.margin.3),
        ),
        align_items: alignment_to_items(layout.horizontal_alignment),
        justify_content: alignment_to_justify(layout.vertical_alignment),
        overflow: if layout.clip_contents {
            Overflow::clip()
        } else {
            Overflow::visible()
        },
        ..default()
    }
}

const fn alignment_to_items(alignment: UiAlignment) -> AlignItems {
    match alignment {
        UiAlignment::Start => AlignItems::FlexStart,
        UiAlignment::Center => AlignItems::Center,
        UiAlignment::End => AlignItems::FlexEnd,
        UiAlignment::Stretch => AlignItems::Stretch,
    }
}

const fn alignment_to_justify(alignment: UiAlignment) -> JustifyContent {
    match alignment {
        UiAlignment::Start => JustifyContent::FlexStart,
        UiAlignment::Center => JustifyContent::Center,
        UiAlignment::End => JustifyContent::FlexEnd,
        UiAlignment::Stretch => JustifyContent::SpaceBetween,
    }
}

fn project_image_asset_path(value: &str) -> Option<String> {
    scene_image_asset_path(value).map(|path| format!("project://{path}"))
}

fn scene_color((r, g, b, a): (f32, f32, f32, f32)) -> Color {
    Color::srgba(
        r.clamp(0.0, 1.0),
        g.clamp(0.0, 1.0),
        b.clamp(0.0, 1.0),
        a.clamp(0.0, 1.0),
    )
}

fn resize_handle_node(handle: UiResizeHandle) -> Node {
    let mut node = Node {
        position_type: PositionType::Absolute,
        width: Val::Px(HANDLE_SIZE),
        height: Val::Px(HANDLE_SIZE),
        border: UiRect::all(Val::Px(1.0)),
        ..default()
    };
    if handle.west() {
        node.left = Val::Px(-HANDLE_SIZE * 0.5);
    } else if handle.east() {
        node.right = Val::Px(-HANDLE_SIZE * 0.5);
    } else {
        node.left = Val::Percent(50.0);
        node.margin.left = Val::Px(-HANDLE_SIZE * 0.5);
    }
    if handle.north() {
        node.top = Val::Px(-HANDLE_SIZE * 0.5);
    } else if handle.south() {
        node.bottom = Val::Px(-HANDLE_SIZE * 0.5);
    } else {
        node.top = Val::Percent(50.0);
        node.margin.top = Val::Px(-HANDLE_SIZE * 0.5);
    }
    node
}

fn select_ui_preview(
    mut click: On<Pointer<Click>>,
    previews: Query<&UiPreviewNode>,
    keyboard: Res<ButtonInput<KeyCode>>,
    navigation: Res<ViewportNavigationState>,
    ownership: Res<PrimaryPointerOwnership>,
    mut selection: ResMut<Selection>,
    mut selection_set: ResMut<SelectionSet>,
) {
    if click.button != PointerButton::Primary
        || camera_modifier_pressed(&keyboard)
        || navigation.is_navigating()
        || navigation.blocks_primary_selection()
        || !ownership.is_owned_by(PrimaryPointerOwner::Ui)
    {
        return;
    }
    let Ok(preview) = previews.get(click.entity) else {
        return;
    };
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    selection_set.select_from_click(&mut selection, preview.source, shift, control);
    click.propagate(false);
}

fn capture_ui_primary_pointer(
    press: On<Pointer<Press>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<UiEditState>,
    mut ownership: ResMut<PrimaryPointerOwnership>,
) {
    if press.button == PointerButton::Primary
        && !camera_modifier_pressed(&keyboard)
        && ownership.claim(PrimaryPointerOwner::Ui)
    {
        state.primary_pointer_captured = true;
    }
}

fn release_ui_pointer_capture(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut state: ResMut<UiEditState>,
) {
    if mouse
        .as_deref()
        .is_none_or(|mouse| !mouse.pressed(MouseButton::Left))
    {
        state.primary_pointer_captured = false;
    }
}

fn begin_ui_move_drag(
    mut drag: On<Pointer<DragStart>>,
    previews: Query<(Entity, &UiPreviewNode, &ComputedNode, &UiGlobalTransform)>,
    overlays: Query<(), With<UiSelectionOverlay>>,
    content: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<UiPreviewContent>>,
    layouts: Query<&SceneUiLayout>,
    keyboard: Res<ButtonInput<KeyCode>>,
    nodes: Query<SceneSnapshotQuery>,
    mode: Res<WorkspaceMode>,
    navigation: Res<ViewportNavigationState>,
    ownership: Res<PrimaryPointerOwnership>,
    registry: Res<UiPreviewRegistry>,
    mut selection: ResMut<Selection>,
    mut selection_set: ResMut<SelectionSet>,
    mut state: ResMut<UiEditState>,
) {
    if drag.button != PointerButton::Primary
        || camera_modifier_pressed(&keyboard)
        || navigation.is_navigating()
        || navigation.blocks_primary_selection()
        || !ownership.is_owned_by(PrimaryPointerOwner::Ui)
    {
        return;
    }
    let (preview_entity, source) =
        if let Ok((preview_entity, preview, _, _)) = previews.get(drag.entity) {
            (preview_entity, preview.source)
        } else if overlays.get(drag.entity).is_ok() {
            let Some(source) = selection.0 else { return };
            let Some(preview_entity) = registry.nodes.get(&source).copied() else {
                return;
            };
            (preview_entity, source)
        } else {
            return;
        };
    let Ok(layout) = layouts.get(source) else {
        return;
    };
    if control_pressed(&keyboard) {
        selection_set.select_from_click(&mut selection, source, true, false);
    } else {
        selection_set.select_only(&mut selection, source);
    }
    state.before = Some(capture_scene_snapshot(&nodes, &selection, *mode));
    state.operation = Some(UiEditOperation::Move {
        source,
        start_layout: *layout,
        axis: None,
    });
    let selected_sources: Vec<_> = selection_set
        .entities(&selection)
        .filter(|source| registry.nodes.contains_key(source))
        .collect();
    let moving_sources = top_level_ui_sources(&selected_sources, &registry, &previews);
    state.skip_primary_move = !moving_sources.contains(&source);
    state.snap_context =
        capture_ui_snap_context(preview_entity, &selected_sources, &previews, &content);
    let group_move = capture_ui_group_move(
        source,
        &moving_sources,
        &selected_sources,
        &registry,
        &layouts,
        &previews,
        &content,
        state.snap_context.as_mut(),
    );
    state.group_move = group_move;
    state.guides = default();
    drag.propagate(false);
}

fn begin_ui_axis_move_drag(
    mut drag: On<Pointer<DragStart>>,
    axes: Query<&UiMoveAxis>,
    keyboard: Res<ButtonInput<KeyCode>>,
    layouts: Query<&SceneUiLayout>,
    registry: Res<UiPreviewRegistry>,
    previews: Query<(Entity, &UiPreviewNode, &ComputedNode, &UiGlobalTransform)>,
    content: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<UiPreviewContent>>,
    nodes: Query<SceneSnapshotQuery>,
    mode: Res<WorkspaceMode>,
    navigation: Res<ViewportNavigationState>,
    ownership: Res<PrimaryPointerOwnership>,
    mut selection: ResMut<Selection>,
    mut selection_set: ResMut<SelectionSet>,
    mut state: ResMut<UiEditState>,
) {
    if drag.button != PointerButton::Primary
        || camera_modifier_pressed(&keyboard)
        || navigation.is_navigating()
        || navigation.blocks_primary_selection()
        || !ownership.is_owned_by(PrimaryPointerOwner::Ui)
    {
        return;
    }
    let Ok(axis) = axes.get(drag.entity) else {
        return;
    };
    let Some(source) = selection.0 else { return };
    let Ok(layout) = layouts.get(source) else {
        return;
    };
    if !control_pressed(&keyboard) {
        selection_set.select_only(&mut selection, source);
    }
    state.before = Some(capture_scene_snapshot(&nodes, &selection, *mode));
    state.operation = Some(UiEditOperation::Move {
        source,
        start_layout: *layout,
        axis: Some(*axis),
    });
    let selected_sources: Vec<_> = selection_set
        .entities(&selection)
        .filter(|selected| registry.nodes.contains_key(selected))
        .collect();
    let moving_sources = top_level_ui_sources(&selected_sources, &registry, &previews);
    state.skip_primary_move = !moving_sources.contains(&source);
    state.snap_context = registry.nodes.get(&source).copied().and_then(|preview| {
        capture_ui_snap_context(preview, &selected_sources, &previews, &content)
    });
    let group_move = capture_ui_group_move(
        source,
        &moving_sources,
        &selected_sources,
        &registry,
        &layouts,
        &previews,
        &content,
        state.snap_context.as_mut(),
    );
    state.group_move = group_move;
    state.guides = default();
    drag.propagate(false);
}

fn move_selected_ui(
    mut drag: On<Pointer<Drag>>,
    mut previews: Query<
        (
            &mut UiPreviewNode,
            &mut Node,
            &ComputedNode,
            &mut UiTransform,
        ),
        Without<UiPreviewContent>,
    >,
    content: Query<&UiTransform, With<UiPreviewContent>>,
    registry: Res<UiPreviewRegistry>,
    mut layouts: Query<&mut SceneUiLayout>,
    snap: Res<Snap2dSettings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<UiEditState>,
    navigation: Res<ViewportNavigationState>,
    ownership: Res<PrimaryPointerOwnership>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if navigation.is_navigating() || !ownership.is_owned_by(PrimaryPointerOwner::Ui) {
        return;
    }
    let Some(UiEditOperation::Move {
        source,
        start_layout,
        axis,
    }) = state.operation
    else {
        return;
    };
    let Some(preview_entity) = registry.nodes.get(&source).copied() else {
        return;
    };
    let Ok((mut preview, mut preview_node, computed, mut preview_transform)) =
        previews.get_mut(preview_entity)
    else {
        return;
    };
    if drag.button != PointerButton::Primary || preview.source != source {
        return;
    }
    let Ok(mut layout) = layouts.get_mut(source) else {
        return;
    };
    let snapping = snap.effective(shift_pressed(&keyboard));
    let snapped = state.snap_context.as_ref().map(|context| {
        let (screen_delta, guides) =
            snap_ui_move_distance(context, drag.distance, axis, snapping, *snap);
        (
            screen_delta,
            screen_delta * context.screen_to_layout,
            guides,
        )
    });
    let (screen_delta, delta) = if let Some((screen_delta, delta, guides)) = snapped {
        state.guides = guides;
        (screen_delta, delta)
    } else {
        state.guides = default();
        (
            axis.map_or(drag.distance, |axis| axis.constrain(drag.distance)),
            drag_delta_in_layout_space(drag.distance, computed, content.single().ok()),
        )
    };
    let next = move_layout_from_start(start_layout, delta, axis);
    let mut changed = false;
    if !state.skip_primary_move && *layout != next {
        *layout = next;
        apply_live_preview_layout(
            &mut preview,
            &mut preview_node,
            &mut preview_transform,
            *layout,
        );
        changed = true;
    }
    drop((preview, preview_node, preview_transform, layout));

    for member in state.group_move.iter().copied() {
        let Some(preview_entity) = registry.nodes.get(&member.source).copied() else {
            continue;
        };
        let Ok((mut preview, mut node, _, mut transform)) = previews.get_mut(preview_entity) else {
            continue;
        };
        let Ok(mut layout) = layouts.get_mut(member.source) else {
            continue;
        };
        let next = move_layout_from_start(
            member.start_layout,
            screen_delta * member.screen_to_layout,
            axis,
        );
        if *layout == next {
            continue;
        }
        *layout = next;
        apply_live_preview_layout(&mut preview, &mut node, &mut transform, *layout);
        changed = true;
    }
    if changed {
        mark_document_changed(document.as_deref_mut());
    }
    drag.propagate(false);
}

fn begin_ui_resize_drag(
    mut drag: On<Pointer<DragStart>>,
    handles: Query<&UiResizeHandle>,
    keyboard: Res<ButtonInput<KeyCode>>,
    layouts: Query<&SceneUiLayout>,
    registry: Res<UiPreviewRegistry>,
    previews: Query<(Entity, &UiPreviewNode, &ComputedNode, &UiGlobalTransform)>,
    content_geometry: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<UiPreviewContent>>,
    content_transform: Query<&UiTransform, With<UiPreviewContent>>,
    nodes: Query<SceneSnapshotQuery>,
    mode: Res<WorkspaceMode>,
    navigation: Res<ViewportNavigationState>,
    ownership: Res<PrimaryPointerOwnership>,
    mut selection: ResMut<Selection>,
    mut selection_set: ResMut<SelectionSet>,
    mut state: ResMut<UiEditState>,
) {
    if drag.button != PointerButton::Primary
        || camera_modifier_pressed(&keyboard)
        || navigation.is_navigating()
        || navigation.blocks_primary_selection()
        || !ownership.is_owned_by(PrimaryPointerOwner::Ui)
    {
        return;
    }
    let Ok(handle) = handles.get(drag.entity) else {
        return;
    };
    let Some(source) = selection.0 else { return };
    let (Ok(layout), Some(preview)) = (layouts.get(source), registry.nodes.get(&source).copied())
    else {
        return;
    };
    if !control_pressed(&keyboard) {
        selection_set.select_only(&mut selection, source);
    }
    let Ok((_, _, computed, _)) = previews.get(preview) else {
        return;
    };
    let content_scale = content_transform
        .single()
        .map(|transform| transform.scale)
        .unwrap_or(Vec2::ONE)
        .max(Vec2::splat(0.000_001));
    let rendered_size = computed.size() * computed.inverse_scale_factor() / content_scale;
    state.before = Some(capture_scene_snapshot(&nodes, &selection, *mode));
    state.operation = Some(UiEditOperation::Resize {
        source,
        handle: *handle,
        start_layout: *layout,
        start_rendered_size: rendered_size,
    });
    state.snap_context = capture_ui_snap_context(preview, &[source], &previews, &content_geometry);
    state.group_move.clear();
    state.skip_primary_move = false;
    state.guides = default();
    drag.propagate(false);
}

fn resize_selected_ui(
    mut drag: On<Pointer<Drag>>,
    handles: Query<(&UiResizeHandle, &ComputedNode)>,
    mut previews: Query<
        (&mut Node, &mut UiPreviewNode, &mut UiTransform),
        Without<UiPreviewContent>,
    >,
    content: Query<&UiTransform, With<UiPreviewContent>>,
    registry: Res<UiPreviewRegistry>,
    mut layouts: Query<&mut SceneUiLayout>,
    snap: Res<Snap2dSettings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<UiEditState>,
    navigation: Res<ViewportNavigationState>,
    ownership: Res<PrimaryPointerOwnership>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if navigation.is_navigating() || !ownership.is_owned_by(PrimaryPointerOwner::Ui) {
        return;
    }
    let Some(UiEditOperation::Resize {
        source,
        handle: active_handle,
        start_layout,
        start_rendered_size,
    }) = state.operation
    else {
        return;
    };
    let Ok((handle, handle_node)) = handles.get(drag.entity) else {
        return;
    };
    if drag.button != PointerButton::Primary || *handle != active_handle {
        return;
    }
    let Some(preview) = registry.nodes.get(&source).copied() else {
        return;
    };
    let Ok((mut preview_node, mut preview_data, mut preview_transform)) = previews.get_mut(preview)
    else {
        return;
    };
    let Ok(mut layout) = layouts.get_mut(source) else {
        return;
    };
    let snapping = snap.effective(shift_pressed(&keyboard));
    let snapped = state.snap_context.as_ref().map(|context| {
        let (screen_delta, guides) =
            snap_ui_resize_distance(context, drag.distance, active_handle, snapping, *snap);
        (screen_delta * context.screen_to_layout, guides)
    });
    let delta = if let Some((delta, guides)) = snapped {
        state.guides = guides;
        delta
    } else {
        state.guides = default();
        drag_delta_in_layout_space(drag.distance, handle_node, content.single().ok())
    };
    let next = resize_layout_from_start(start_layout, active_handle, delta, start_rendered_size);
    if *layout == next {
        drag.propagate(false);
        return;
    }
    *layout = next;
    apply_live_preview_layout(
        &mut preview_data,
        &mut preview_node,
        &mut preview_transform,
        *layout,
    );
    mark_document_changed(document.as_deref_mut());
    drag.propagate(false);
}

fn finish_ui_edit_drag(
    mut drag: On<Pointer<DragEnd>>,
    nodes: Query<SceneSnapshotQuery>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    ownership: Res<PrimaryPointerOwnership>,
    mut state: ResMut<UiEditState>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if drag.button != PointerButton::Primary
        || !ownership.is_owned_by(PrimaryPointerOwner::Ui)
        || state.operation.take().is_none()
    {
        return;
    }
    state.snap_context = None;
    state.guides = default();
    state.group_move.clear();
    state.skip_primary_move = false;
    let before = state.before.take();
    if let (Some(before), Some(history), Some(document)) =
        (before, history.as_deref_mut(), document.as_deref_mut())
    {
        let after = capture_scene_snapshot(&nodes, &selection, *mode);
        history.commit("Edit UI Rect", before, after, document);
    }
    drag.propagate(false);
}

fn camera_modifier_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::Space)
        || keyboard.pressed(KeyCode::AltLeft)
        || keyboard.pressed(KeyCode::AltRight)
}

fn control_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight)
}

fn drag_delta_in_layout_space(
    pointer_delta: Vec2,
    computed: &ComputedNode,
    content: Option<&UiTransform>,
) -> Vec2 {
    let content_scale = content
        .map(|transform| transform.scale)
        .unwrap_or(Vec2::ONE)
        .max(Vec2::splat(0.000_001));
    pointer_delta * computed.inverse_scale_factor() / content_scale
}

fn capture_ui_snap_context(
    selected_preview: Entity,
    excluded_sources: &[Entity],
    previews: &Query<(Entity, &UiPreviewNode, &ComputedNode, &UiGlobalTransform)>,
    content: &Query<(Entity, &ComputedNode, &UiGlobalTransform), With<UiPreviewContent>>,
) -> Option<UiSnapContext> {
    let (_, selected, selected_node, selected_transform) = previews.get(selected_preview).ok()?;
    let (_, content_node, content_transform) = content.single().ok()?;
    let start_bounds = ui_screen_bounds(selected_node, selected_transform)?;
    let mut target_x = Vec::new();
    let mut target_y = Vec::new();

    for (entity, preview, node, transform) in previews.iter() {
        if entity == selected_preview
            || excluded_sources.contains(&preview.source)
            || preview.parent_target != selected.parent_target
        {
            continue;
        }
        if let Some(bounds) = ui_screen_bounds(node, transform) {
            target_x.extend_from_slice(&bounds.x_values());
            target_y.extend_from_slice(&bounds.y_values());
        }
    }

    let parent_geometry = previews
        .get(selected.parent_target)
        .ok()
        .map(|(_, _, node, transform)| (node, transform))
        .or_else(|| {
            content
                .get(selected.parent_target)
                .ok()
                .map(|(_, node, transform)| (node, transform))
        });
    if let Some((node, transform)) = parent_geometry {
        if let Some(bounds) = ui_screen_bounds(node, transform) {
            target_x.extend_from_slice(&bounds.x_values());
            target_y.extend_from_slice(&bounds.y_values());
        }
    }

    let content_bounds = ui_screen_bounds(content_node, content_transform)?;
    target_x.push(content_bounds.min.x);
    target_y.push(content_bounds.min.y);

    let parent_scale = parent_geometry
        .map(|(_, transform)| transform.to_scale_angle_translation().0.abs())
        .unwrap_or(Vec2::ONE)
        .max(Vec2::splat(0.000_001));
    let content_scale = content_transform
        .to_scale_angle_translation()
        .0
        .abs()
        .max(Vec2::splat(0.000_001));
    Some(UiSnapContext {
        start_bounds,
        target_x,
        target_y,
        grid_origin: content_bounds.min,
        grid_to_screen: content_scale / content_node.inverse_scale_factor(),
        screen_to_layout: Vec2::splat(selected_node.inverse_scale_factor()) / parent_scale,
    })
}

fn capture_ui_group_move(
    primary: Entity,
    moving_sources: &[Entity],
    excluded_sources: &[Entity],
    registry: &UiPreviewRegistry,
    layouts: &Query<&SceneUiLayout>,
    previews: &Query<(Entity, &UiPreviewNode, &ComputedNode, &UiGlobalTransform)>,
    content: &Query<(Entity, &ComputedNode, &UiGlobalTransform), With<UiPreviewContent>>,
    mut primary_context: Option<&mut UiSnapContext>,
) -> Vec<UiGroupMove> {
    let mut members = Vec::new();
    for source in moving_sources.iter().copied() {
        let Some(preview) = registry.nodes.get(&source).copied() else {
            continue;
        };
        let Some(context) = capture_ui_snap_context(preview, excluded_sources, previews, content)
        else {
            continue;
        };
        if let Some(primary_context) = primary_context.as_deref_mut() {
            primary_context.start_bounds = primary_context.start_bounds.union(context.start_bounds);
        }
        if source == primary {
            continue;
        }
        let Ok(layout) = layouts.get(source) else {
            continue;
        };
        members.push(UiGroupMove {
            source,
            start_layout: *layout,
            screen_to_layout: context.screen_to_layout,
        });
    }
    members
}

fn top_level_ui_sources(
    selected_sources: &[Entity],
    registry: &UiPreviewRegistry,
    previews: &Query<(Entity, &UiPreviewNode, &ComputedNode, &UiGlobalTransform)>,
) -> Vec<Entity> {
    let selected_ids: HashSet<_> = selected_sources
        .iter()
        .filter_map(|source| {
            registry
                .nodes
                .get(source)
                .and_then(|preview| previews.get(*preview).ok())
                .map(|(_, preview, _, _)| preview.data.id)
        })
        .collect();
    let parents: HashMap<_, _> = previews
        .iter()
        .map(|(_, preview, _, _)| (preview.data.id, preview.data.parent))
        .collect();
    selected_sources
        .iter()
        .copied()
        .filter(|source| {
            let mut parent = registry
                .nodes
                .get(source)
                .and_then(|preview| previews.get(*preview).ok())
                .and_then(|(_, preview, _, _)| preview.data.parent);
            while let Some(parent_id) = parent {
                if selected_ids.contains(&parent_id) {
                    return false;
                }
                parent = parents.get(&parent_id).copied().flatten();
            }
            true
        })
        .collect()
}

fn ui_screen_bounds(node: &ComputedNode, transform: &UiGlobalTransform) -> Option<UiScreenBounds> {
    if node.size().min_element() < 0.5 {
        return None;
    }
    let (min, max) = transformed_ui_bounds(node, transform);
    min.is_finite()
        .then_some(())
        .filter(|_| max.is_finite())
        .map(|_| UiScreenBounds { min, max })
}

pub(super) fn ui_preview_logical_rect(
    node: &ComputedNode,
    transform: &UiGlobalTransform,
) -> Option<Rect> {
    let bounds = ui_screen_bounds(node, transform)?;
    let scale = node.inverse_scale_factor();
    Some(Rect::from_corners(bounds.min * scale, bounds.max * scale))
}

pub(super) fn ui_editor_window_rect(
    camera: &Camera,
    node: &ComputedNode,
    transform: &UiGlobalTransform,
) -> Option<Rect> {
    let rect = ui_preview_logical_rect(node, transform)?;
    let viewport_origin = camera.logical_viewport_rect()?.min;
    Some(Rect::from_corners(
        rect.min + viewport_origin,
        rect.max + viewport_origin,
    ))
}

fn snap_ui_move_distance(
    context: &UiSnapContext,
    pointer_distance: Vec2,
    axis: Option<UiMoveAxis>,
    snapping: bool,
    settings: Snap2dSettings,
) -> (Vec2, UiSnapGuides) {
    let mut distance = axis.map_or(pointer_distance, |axis| axis.constrain(pointer_distance));
    if !snapping {
        return (distance, default());
    }

    let raw_bounds = context.start_bounds.translated(distance);
    let x_values = raw_bounds.x_values();
    let y_values = raw_bounds.y_values();
    let snap = smart_snap_2d(
        if axis == Some(UiMoveAxis::Y) {
            &[]
        } else {
            &x_values
        },
        if axis == Some(UiMoveAxis::X) {
            &[]
        } else {
            &y_values
        },
        &context.target_x,
        &context.target_y,
        settings.smart_distance_px,
    );
    let mut guides = UiSnapGuides {
        x: snap.x.map(|axis| axis.guide),
        y: snap.y.map(|axis| axis.guide),
    };

    if let Some(x) = snap.x {
        distance.x += x.delta;
    } else if axis != Some(UiMoveAxis::Y) {
        distance.x += grid_snap_correction(raw_bounds.min.x, context, settings, true);
        guides.x = None;
    }
    if let Some(y) = snap.y {
        distance.y += y.delta;
    } else if axis != Some(UiMoveAxis::X) {
        distance.y += grid_snap_correction(raw_bounds.min.y, context, settings, false);
        guides.y = None;
    }
    (distance, guides)
}

fn snap_ui_resize_distance(
    context: &UiSnapContext,
    pointer_distance: Vec2,
    handle: UiResizeHandle,
    snapping: bool,
    settings: Snap2dSettings,
) -> (Vec2, UiSnapGuides) {
    let mut distance = Vec2::new(
        if handle.west() || handle.east() {
            pointer_distance.x
        } else {
            0.0
        },
        if handle.north() || handle.south() {
            pointer_distance.y
        } else {
            0.0
        },
    );
    if !snapping {
        return (distance, default());
    }

    let raw_bounds = context.start_bounds.resized(handle, distance);
    let moving_x = [if handle.west() {
        raw_bounds.min.x
    } else {
        raw_bounds.max.x
    }];
    let moving_y = [if handle.north() {
        raw_bounds.min.y
    } else {
        raw_bounds.max.y
    }];
    let snap = smart_snap_2d(
        if handle.west() || handle.east() {
            &moving_x
        } else {
            &[]
        },
        if handle.north() || handle.south() {
            &moving_y
        } else {
            &[]
        },
        &context.target_x,
        &context.target_y,
        settings.smart_distance_px,
    );
    let guides = UiSnapGuides {
        x: snap.x.map(|axis| axis.guide),
        y: snap.y.map(|axis| axis.guide),
    };

    if let Some(x) = snap.x {
        distance.x += x.delta;
    } else if handle.west() || handle.east() {
        distance.x += grid_snap_correction(moving_x[0], context, settings, true);
    }
    if let Some(y) = snap.y {
        distance.y += y.delta;
    } else if handle.north() || handle.south() {
        distance.y += grid_snap_correction(moving_y[0], context, settings, false);
    }
    (distance, guides)
}

fn grid_snap_correction(
    value: f32,
    context: &UiSnapContext,
    settings: Snap2dSettings,
    x_axis: bool,
) -> f32 {
    let grid_scale = if x_axis {
        context.grid_to_screen.x
    } else {
        context.grid_to_screen.y
    };
    let origin = if x_axis {
        context.grid_origin.x + settings.grid_offset.x * grid_scale
    } else {
        context.grid_origin.y + settings.grid_offset.y * grid_scale
    };
    snap_scalar(value, settings.grid_size * grid_scale, origin) - value
}

fn cancel_ui_edit_during_navigation(
    navigation: Res<ViewportNavigationState>,
    mut state: ResMut<UiEditState>,
) {
    if navigation.is_navigating() {
        state.operation = None;
        state.primary_pointer_captured = false;
        state.before = None;
        state.snap_context = None;
        state.guides = default();
        state.group_move.clear();
        state.skip_primary_move = false;
    }
}

fn sync_ui_move_gizmo_visibility(
    settings: Res<TransformGizmoSettings>,
    overlays: Query<&Node, (With<UiSelectionOverlay>, Without<UiMoveGizmo>)>,
    mut gizmos: Query<&mut Node, (With<UiMoveGizmo>, Without<UiSelectionOverlay>)>,
) {
    let visible = overlays
        .single()
        .is_ok_and(|overlay| overlay.display != Display::None)
        && settings.mode == TransformGizmoMode::Translate;
    for mut gizmo in &mut gizmos {
        gizmo.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn sync_ui_snap_guides(
    state: Res<UiEditState>,
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<UiPreviewCanvas>>,
    mut guides: Query<(&UiSnapGuideAxis, &mut Node)>,
) {
    let canvas_geometry = canvas.single().ok().and_then(|(node, transform)| {
        ui_screen_bounds(node, transform).map(|bounds| {
            (
                bounds,
                node.size() * node.inverse_scale_factor(),
                node.inverse_scale_factor(),
            )
        })
    });

    for (axis, mut node) in &mut guides {
        let position = match axis {
            UiSnapGuideAxis::X => state.guides.x,
            UiSnapGuideAxis::Y => state.guides.y,
        };
        let Some((position, (bounds, canvas_size, scale))) = position.zip(canvas_geometry) else {
            node.display = Display::None;
            continue;
        };

        node.display = Display::Flex;
        match axis {
            UiSnapGuideAxis::X => {
                node.left = Val::Px((position - bounds.min.x) * scale);
                node.top = Val::Px(0.0);
                node.width = Val::Px(1.0);
                node.height = Val::Px(canvas_size.y);
            }
            UiSnapGuideAxis::Y => {
                node.left = Val::Px(0.0);
                node.top = Val::Px((position - bounds.min.y) * scale);
                node.width = Val::Px(canvas_size.x);
                node.height = Val::Px(1.0);
            }
        }
    }
}

fn apply_move_delta(layout: &mut SceneUiLayout, delta: Vec2) {
    if layout.anchor_max.0 > layout.anchor_min.0 {
        layout.offset.0 += delta.x;
        layout.margin.2 -= delta.x;
    } else {
        layout.offset.0 += delta.x;
    }
    if layout.anchor_max.1 > layout.anchor_min.1 {
        layout.offset.1 += delta.y;
        layout.margin.3 -= delta.y;
    } else {
        layout.offset.1 += delta.y;
    }
}

fn move_layout_from_start(
    start: SceneUiLayout,
    delta: Vec2,
    axis: Option<UiMoveAxis>,
) -> SceneUiLayout {
    let mut layout = start;
    apply_move_delta(
        &mut layout,
        axis.map_or(delta, |axis| axis.constrain(delta)),
    );
    layout
}

fn apply_resize_delta(
    layout: &mut SceneUiLayout,
    handle: UiResizeHandle,
    delta: Vec2,
    rendered_size: Vec2,
) {
    let stretch_x = layout.anchor_max.0 > layout.anchor_min.0;
    let stretch_y = layout.anchor_max.1 > layout.anchor_min.1;

    if handle.west() {
        let width = if stretch_x {
            rendered_size.x
        } else {
            layout.size.0
        };
        let applied = delta.x.min((width - 1.0).max(0.0));
        layout.offset.0 += applied;
        if !stretch_x {
            layout.size.0 = (width - applied).max(1.0);
        }
    } else if handle.east() {
        let width = if stretch_x {
            rendered_size.x
        } else {
            layout.size.0
        };
        let applied = delta.x.max(-(width - 1.0).max(0.0));
        if stretch_x {
            layout.margin.2 -= applied;
        } else {
            layout.size.0 = (width + applied).max(1.0);
        }
    }

    if handle.north() {
        let height = if stretch_y {
            rendered_size.y
        } else {
            layout.size.1
        };
        let applied = delta.y.min((height - 1.0).max(0.0));
        layout.offset.1 += applied;
        if !stretch_y {
            layout.size.1 = (height - applied).max(1.0);
        }
    } else if handle.south() {
        let height = if stretch_y {
            rendered_size.y
        } else {
            layout.size.1
        };
        let applied = delta.y.max(-(height - 1.0).max(0.0));
        if stretch_y {
            layout.margin.3 -= applied;
        } else {
            layout.size.1 = (height + applied).max(1.0);
        }
    }
}

fn resize_layout_from_start(
    start: SceneUiLayout,
    handle: UiResizeHandle,
    delta: Vec2,
    rendered_size: Vec2,
) -> SceneUiLayout {
    let mut layout = start;
    apply_resize_delta(&mut layout, handle, delta, rendered_size);
    layout
}

fn mark_document_changed(document: Option<&mut SceneDocument>) {
    if let Some(document) = document {
        let was_dirty = document.dirty;
        document.open = true;
        document.dirty = true;
        if !was_dirty {
            document.bump_revision();
        }
    }
}

#[allow(clippy::type_complexity)]
fn sync_ui_selection_overlay(
    selection: Res<Selection>,
    registry: Res<UiPreviewRegistry>,
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<UiPreviewCanvas>>,
    content: Query<
        (&ComputedNode, &UiGlobalTransform),
        (With<UiPreviewContent>, Without<UiPreviewCanvas>),
    >,
    sources: Query<(&SceneUiLayout, &SceneParentId, &SceneNodeId)>,
    preview_geometry: Query<(&ComputedNode, &UiGlobalTransform), With<UiPreviewNode>>,
    mut overlays: Query<&mut Node, (With<UiSelectionOverlay>, Without<UiAnchorIndicator>)>,
    mut indicators: Query<(&UiAnchorIndicator, &mut Node), Without<UiSelectionOverlay>>,
) {
    let Ok(mut overlay) = overlays.single_mut() else {
        return;
    };
    let hide = |overlay: &mut Node,
                indicators: &mut Query<
        (&UiAnchorIndicator, &mut Node),
        Without<UiSelectionOverlay>,
    >| {
        overlay.display = Display::None;
        for (_, mut indicator) in indicators.iter_mut() {
            indicator.display = Display::None;
        }
    };
    let Some(source) = selection.0 else {
        hide(&mut overlay, &mut indicators);
        return;
    };
    let (Ok((layout, parent, _)), Some(preview)) =
        (sources.get(source), registry.nodes.get(&source).copied())
    else {
        hide(&mut overlay, &mut indicators);
        return;
    };
    let (Ok((canvas_node, canvas_transform)), Ok((node, transform))) =
        (canvas.single(), preview_geometry.get(preview))
    else {
        hide(&mut overlay, &mut indicators);
        return;
    };
    if node.size().min_element() < 0.5 || canvas_node.size().min_element() < 0.5 {
        hide(&mut overlay, &mut indicators);
        return;
    }

    let scale = canvas_node.inverse_scale_factor();
    let (canvas_min, _) = transformed_ui_bounds(canvas_node, canvas_transform);
    let (node_min, node_max) = transformed_ui_bounds(node, transform);
    overlay.display = Display::Flex;
    overlay.left = Val::Px((node_min.x - canvas_min.x) * scale);
    overlay.top = Val::Px((node_min.y - canvas_min.y) * scale);
    overlay.width = Val::Px((node_max.x - node_min.x) * scale);
    overlay.height = Val::Px((node_max.y - node_min.y) * scale);

    let parent_preview = parent.0.and_then(|parent_id| {
        sources
            .iter()
            .find(|(_, _, id)| **id == parent_id)
            .and_then(|_| {
                registry.nodes.iter().find_map(|(source, preview)| {
                    sources
                        .get(*source)
                        .ok()
                        .filter(|(_, _, id)| **id == parent_id)
                        .map(|_| *preview)
                })
            })
    });
    let (parent_min, parent_size) = parent_preview
        .and_then(|preview| preview_geometry.get(preview).ok())
        .map(|(computed, transform)| {
            let (min, max) = transformed_ui_bounds(computed, transform);
            (min, max - min)
        })
        .or_else(|| {
            content.single().ok().map(|(computed, transform)| {
                let (min, max) = transformed_ui_bounds(computed, transform);
                (min, max - min)
            })
        })
        .unwrap_or_else(|| {
            let (_, canvas_max) = transformed_ui_bounds(canvas_node, canvas_transform);
            (canvas_min, canvas_max - canvas_min)
        });
    let anchors = [layout.anchor_min, layout.anchor_max];
    for (indicator, mut indicator_node) in &mut indicators {
        if indicator.0 == 1 && layout.anchor_min == layout.anchor_max {
            indicator_node.display = Display::None;
            continue;
        }
        let anchor = anchors[indicator.0.min(1) as usize];
        let position = parent_min + parent_size * Vec2::new(anchor.0, anchor.1);
        indicator_node.display = Display::Flex;
        indicator_node.left = Val::Px((position.x - canvas_min.x) * scale);
        indicator_node.top = Val::Px((position.y - canvas_min.y) * scale);
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_ui_secondary_selection_overlays(
    mut commands: Commands,
    selection: Res<Selection>,
    selection_set: Res<SelectionSet>,
    registry: Res<UiPreviewRegistry>,
    canvas: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<UiPreviewCanvas>>,
    preview_geometry: Query<(&ComputedNode, &UiGlobalTransform), With<UiPreviewNode>>,
    mut overlays: Query<(Entity, &UiSecondarySelectionOverlay, &mut Node)>,
) {
    let Ok((canvas_entity, canvas_node, canvas_transform)) = canvas.single() else {
        return;
    };
    let Some(canvas_bounds) = ui_screen_bounds(canvas_node, canvas_transform) else {
        return;
    };
    let scale = canvas_node.inverse_scale_factor();
    let desired: HashSet<_> = selection_set
        .entities(&selection)
        .filter(|source| Some(*source) != selection.0 && registry.nodes.contains_key(source))
        .collect();
    let mut existing = HashSet::new();

    for (entity, overlay, mut node) in &mut overlays {
        if !desired.contains(&overlay.source) {
            commands.entity(entity).despawn();
            continue;
        }
        let Some(preview) = registry.nodes.get(&overlay.source).copied() else {
            commands.entity(entity).despawn();
            continue;
        };
        let Ok((computed, transform)) = preview_geometry.get(preview) else {
            node.display = Display::None;
            continue;
        };
        let Some(bounds) = ui_screen_bounds(computed, transform) else {
            node.display = Display::None;
            continue;
        };
        existing.insert(overlay.source);
        node.display = Display::Flex;
        node.left = Val::Px((bounds.min.x - canvas_bounds.min.x) * scale);
        node.top = Val::Px((bounds.min.y - canvas_bounds.min.y) * scale);
        node.width = Val::Px((bounds.max.x - bounds.min.x) * scale);
        node.height = Val::Px((bounds.max.y - bounds.min.y) * scale);
    }

    for source in desired.difference(&existing).copied() {
        commands.spawn((
            UiSecondarySelectionOverlay { source },
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(SELECTION_COLOR),
            BackgroundColor(Color::NONE),
            GlobalZIndex(99_997),
            Pickable::IGNORE,
            ChildOf(canvas_entity),
        ));
    }
}

fn transformed_ui_bounds(node: &ComputedNode, transform: &UiGlobalTransform) -> (Vec2, Vec2) {
    let half_size = node.size() * 0.5;
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for point in [
        Vec2::new(-half_size.x, -half_size.y),
        Vec2::new(half_size.x, -half_size.y),
        Vec2::new(half_size.x, half_size.y),
        Vec2::new(-half_size.x, half_size.y),
    ] {
        let point = transform.transform_point2(point);
        min = min.min(point);
        max = max.max(point);
    }
    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        app::TaskPoolPlugin,
        asset::{AssetApp, AssetPlugin, io::AssetSourceBuilder},
    };

    fn test_snap_context(bounds: UiScreenBounds) -> UiSnapContext {
        UiSnapContext {
            start_bounds: bounds,
            target_x: Vec::new(),
            target_y: Vec::new(),
            grid_origin: Vec2::ZERO,
            grid_to_screen: Vec2::ONE,
            screen_to_layout: Vec2::ONE,
        }
    }

    #[test]
    fn ui_pointer_press_owns_selection_before_drag_start() {
        let mut state = UiEditState::default();
        assert!(!state.is_active());

        state.primary_pointer_captured = true;
        assert!(state.is_active());

        state.primary_pointer_captured = false;
        assert!(!state.is_active());
    }

    #[test]
    fn fixed_ui_move_changes_offsets() {
        let mut layout = SceneUiLayout::sized(100.0, 40.0);
        apply_move_delta(&mut layout, Vec2::new(12.0, -5.0));
        assert_eq!(layout.offset, (12.0, -5.0));
        assert_eq!(layout.margin, (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn ui_drag_recomputes_from_start_instead_of_accumulating_events() {
        let start = SceneUiLayout::sized(100.0, 40.0);
        let first = move_layout_from_start(start, Vec2::new(8.0, 3.0), None);
        let second = move_layout_from_start(start, Vec2::new(12.0, 5.0), None);

        assert_eq!(first.offset, (8.0, 3.0));
        assert_eq!(second.offset, (12.0, 5.0));
    }

    #[test]
    fn ui_move_arrows_constrain_the_drag_axis() {
        let start = SceneUiLayout::sized(100.0, 40.0);
        let x = move_layout_from_start(start, Vec2::new(12.0, 5.0), Some(UiMoveAxis::X));
        let y = move_layout_from_start(start, Vec2::new(12.0, 5.0), Some(UiMoveAxis::Y));

        assert_eq!(x.offset, (12.0, 0.0));
        assert_eq!(y.offset, (0.0, 5.0));
    }

    #[test]
    fn ui_move_smart_snaps_edges_and_centers_to_siblings() {
        let mut edge_context = test_snap_context(UiScreenBounds {
            min: Vec2::ZERO,
            max: Vec2::splat(100.0),
        });
        edge_context.target_x.push(150.0);
        let (edge_distance, edge_guides) = snap_ui_move_distance(
            &edge_context,
            Vec2::new(47.0, 0.0),
            Some(UiMoveAxis::X),
            true,
            Snap2dSettings::default(),
        );
        assert_eq!(edge_distance, Vec2::new(50.0, 0.0));
        assert_eq!(edge_guides.x, Some(150.0));

        let mut center_context = test_snap_context(UiScreenBounds {
            min: Vec2::ZERO,
            max: Vec2::splat(100.0),
        });
        center_context.target_x.push(100.0);
        let (center_distance, center_guides) = snap_ui_move_distance(
            &center_context,
            Vec2::new(47.0, 0.0),
            Some(UiMoveAxis::X),
            true,
            Snap2dSettings::default(),
        );
        assert_eq!(center_distance, Vec2::new(50.0, 0.0));
        assert_eq!(center_guides.x, Some(100.0));
    }

    #[test]
    fn ui_move_arrow_only_snaps_its_active_axis() {
        let mut context = test_snap_context(UiScreenBounds {
            min: Vec2::ZERO,
            max: Vec2::splat(100.0),
        });
        context.target_x.push(150.0);
        context.target_y.push(120.0);

        let (distance, guides) = snap_ui_move_distance(
            &context,
            Vec2::new(47.0, 18.0),
            Some(UiMoveAxis::X),
            true,
            Snap2dSettings::default(),
        );

        assert_eq!(distance, Vec2::new(50.0, 0.0));
        assert_eq!(guides.x, Some(150.0));
        assert_eq!(guides.y, None);
    }

    #[test]
    fn ui_resize_smart_snap_keeps_the_opposite_edge_fixed() {
        let mut context = test_snap_context(UiScreenBounds {
            min: Vec2::ZERO,
            max: Vec2::splat(100.0),
        });
        context.target_x.push(150.0);
        let (distance, guides) = snap_ui_resize_distance(
            &context,
            Vec2::new(47.0, 12.0),
            UiResizeHandle::East,
            true,
            Snap2dSettings::default(),
        );
        let layout = resize_layout_from_start(
            SceneUiLayout::sized(100.0, 100.0),
            UiResizeHandle::East,
            distance,
            Vec2::splat(100.0),
        );

        assert_eq!(distance, Vec2::new(50.0, 0.0));
        assert_eq!(guides.x, Some(150.0));
        assert_eq!(layout.offset.0, 0.0);
        assert_eq!(layout.size.0, 150.0);
    }

    #[test]
    fn ui_smart_snap_tolerance_stays_in_screen_pixels_at_preview_zoom() {
        let mut context = test_snap_context(UiScreenBounds {
            min: Vec2::ZERO,
            max: Vec2::splat(50.0),
        });
        context.target_x.push(60.0);
        context.screen_to_layout = Vec2::splat(2.0);
        context.grid_to_screen = Vec2::splat(0.5);

        let (screen_distance, guides) = snap_ui_move_distance(
            &context,
            Vec2::new(3.0, 0.0),
            Some(UiMoveAxis::X),
            true,
            Snap2dSettings::default(),
        );

        assert_eq!(screen_distance, Vec2::new(10.0, 0.0));
        assert_eq!(
            screen_distance * context.screen_to_layout,
            Vec2::new(20.0, 0.0)
        );
        assert_eq!(guides.x, Some(60.0));
    }

    #[test]
    fn ui_snap_uses_grid_when_no_smart_target_matches() {
        let context = test_snap_context(UiScreenBounds {
            min: Vec2::new(5.0, 0.0),
            max: Vec2::new(105.0, 100.0),
        });
        let mut settings = Snap2dSettings::default();
        settings.grid_size = 32.0;

        let (distance, guides) = snap_ui_move_distance(
            &context,
            Vec2::new(20.0, 0.0),
            Some(UiMoveAxis::X),
            true,
            settings,
        );

        assert_eq!(distance, Vec2::new(27.0, 0.0));
        assert_eq!(guides, UiSnapGuides::default());
    }

    #[test]
    fn disabling_ui_snap_keeps_raw_distance_and_clears_guides() {
        let mut context = test_snap_context(UiScreenBounds {
            min: Vec2::ZERO,
            max: Vec2::splat(100.0),
        });
        context.target_x.push(150.0);

        let (distance, guides) = snap_ui_move_distance(
            &context,
            Vec2::new(47.0, 3.0),
            None,
            false,
            Snap2dSettings::default(),
        );

        assert_eq!(distance, Vec2::new(47.0, 3.0));
        assert_eq!(guides, UiSnapGuides::default());
    }

    #[test]
    fn every_new_ui_node_starts_at_top_left_zero() {
        for kind in [
            EntityKind::EmptyUi,
            EntityKind::Panel,
            EntityKind::Text,
            EntityKind::Button,
            EntityKind::Image,
        ] {
            let layout = kind.default_ui_layout().unwrap();
            assert_eq!(layout.anchor_min, (0.0, 0.0));
            assert_eq!(layout.anchor_max, (0.0, 0.0));
            assert_eq!(layout.offset, (0.0, 0.0));
            assert_eq!(layout.margin, (0.0, 0.0, 0.0, 0.0));
        }
    }

    #[test]
    fn ui_zero_tracks_the_world_origin_during_pan_and_zoom() {
        let canvas_size = Vec2::new(800.0, 600.0);
        let camera_center = Vec2::new(40.0, -20.0);
        let (translation, scale) = ui_preview_content_transform(canvas_size, camera_center, 2.0);
        let canvas_center = canvas_size * 0.5;
        let rendered_origin = canvas_center + translation - canvas_center * scale;

        assert_eq!(scale, Vec2::splat(0.5));
        assert_eq!(rendered_origin, Vec2::new(380.0, 290.0));
    }

    #[test]
    fn drag_delta_is_converted_back_from_preview_zoom() {
        let computed = ComputedNode::default();
        let content = UiTransform::from_scale(Vec2::splat(0.5));

        assert_eq!(
            drag_delta_in_layout_space(Vec2::new(8.0, -3.0), &computed, Some(&content)),
            Vec2::new(16.0, -6.0)
        );
    }

    #[test]
    fn stretched_ui_move_preserves_rect_size() {
        let mut layout = SceneUiLayout::sized(100.0, 40.0);
        layout.anchor_max = (1.0, 1.0);
        apply_move_delta(&mut layout, Vec2::new(12.0, -5.0));
        assert_eq!(layout.offset, (12.0, -5.0));
        assert_eq!(layout.margin.2, -12.0);
        assert_eq!(layout.margin.3, 5.0);
    }

    #[test]
    fn north_west_resize_moves_origin_and_clamps_size() {
        let mut layout = SceneUiLayout::sized(100.0, 40.0);
        apply_resize_delta(
            &mut layout,
            UiResizeHandle::NorthWest,
            Vec2::new(150.0, 15.0),
            Vec2::new(100.0, 40.0),
        );
        assert_eq!(layout.offset, (99.0, 15.0));
        assert_eq!(layout.size, (1.0, 25.0));
    }

    #[test]
    fn ui_resize_recomputes_from_the_drag_start_size() {
        let start = SceneUiLayout::sized(100.0, 40.0);
        let first = resize_layout_from_start(
            start,
            UiResizeHandle::East,
            Vec2::new(8.0, 0.0),
            Vec2::new(100.0, 40.0),
        );
        let second = resize_layout_from_start(
            start,
            UiResizeHandle::East,
            Vec2::new(12.0, 0.0),
            Vec2::new(100.0, 40.0),
        );

        assert_eq!(first.size.0, 108.0);
        assert_eq!(second.size.0, 112.0);
    }

    #[test]
    fn image_paths_use_the_project_asset_source() {
        assert_eq!(
            project_image_asset_path("res://assets/ui/icon.png").as_deref(),
            Some("project://ui/icon.png")
        );
        assert_eq!(project_image_asset_path("res://"), None);
    }

    #[test]
    fn preview_sync_mirrors_ui_hierarchy_and_content() {
        let mut app = App::new();
        app.register_asset_source("project", AssetSourceBuilder::platform_default(".", None));
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Image>()
            .init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<EditorViewMode>()
            .add_plugins(UiEditorPlugin);
        app.finish();
        app.cleanup();

        let canvas = app
            .world_mut()
            .spawn((UiPreviewCanvas, Node::default()))
            .id();
        let content_root = app
            .world_mut()
            .spawn((
                UiPreviewContent,
                Node::default(),
                UiTransform::default(),
                ChildOf(canvas),
            ))
            .id();
        let panel_id = SceneNodeId::new();
        let mut panel_layout = SceneUiLayout::sized(240.0, 160.0);
        panel_layout.minimum_size = (260.0, 170.0);
        panel_layout.clip_contents = true;
        panel_layout.rotation = 15.0;
        panel_layout.scale = (1.2, 0.8);
        panel_layout.pivot_ratio = (0.5, 0.5);
        let panel = app
            .world_mut()
            .spawn((
                EntityKind::Panel,
                SceneSpace::TwoD,
                panel_id,
                SceneParentId(None),
                SceneSiblingOrder(0),
                panel_layout,
                SceneUiContent::default(),
            ))
            .id();
        let mut button_content = EntityKind::Button.default_ui_content().unwrap();
        button_content.text = "Play".into();
        let button = app
            .world_mut()
            .spawn((
                EntityKind::Button,
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(Some(panel_id)),
                SceneSiblingOrder(0),
                EntityKind::Button.default_ui_layout().unwrap(),
                button_content,
            ))
            .id();
        let image = app
            .world_mut()
            .spawn((
                EntityKind::Image,
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(Some(panel_id)),
                SceneSiblingOrder(1),
                EntityKind::Image.default_ui_layout().unwrap(),
                SceneUiContent {
                    image_path: "res://ui/logo.png".into(),
                    ..default()
                },
            ))
            .id();

        app.update();

        let registry = app.world().resource::<UiPreviewRegistry>();
        let panel_preview = registry.nodes[&panel];
        let button_preview = registry.nodes[&button];
        let image_preview = registry.nodes[&image];
        assert_eq!(
            app.world().get::<ChildOf>(panel_preview).unwrap().parent(),
            content_root
        );
        let panel_node = app.world().get::<Node>(panel_preview).unwrap();
        assert_eq!(panel_node.min_width, Val::Px(260.0));
        assert_eq!(panel_node.min_height, Val::Px(170.0));
        assert_eq!(panel_node.overflow, Overflow::clip());
        assert_eq!(
            app.world().get::<UiTransform>(panel_preview).unwrap(),
            &scene_ui_transform(panel_layout)
        );
        assert_eq!(
            app.world().get::<ChildOf>(button_preview).unwrap().parent(),
            panel_preview
        );
        assert_eq!(app.world().get::<Text>(button_preview).unwrap().0, "Play");
        let preview_image = app.world().get::<ImageNode>(image_preview).unwrap();
        assert_eq!(
            preview_image.image.path().unwrap().to_string(),
            "project://ui/logo.png"
        );

        app.world_mut()
            .entity_mut(button)
            .get_mut::<SceneUiContent>()
            .unwrap()
            .text = "Continue".into();
        app.update();
        assert_eq!(
            app.world().get::<Text>(button_preview).unwrap().0,
            "Continue"
        );
    }
}
