use bevy::{
    camera::Projection,
    gizmos::transform_gizmo::{TransformGizmoMode, TransformGizmoSettings, TransformGizmoSpace},
    math::Affine3A,
    prelude::*,
    ui::UiGlobalTransform,
    window::PrimaryWindow,
};

use crate::{
    entities::{SceneSprite2D, SceneUiLayout},
    hierarchy::{SceneNodeId, SceneParentId},
    scene::SceneDocument,
    selection::{EditableObject, Selection, SelectionSet, on_mesh_click},
    undo::{SceneHistory, SceneSnapshot, SceneSnapshotQuery, capture_scene_snapshot},
    workspace::{EditorViewMode, SceneSpace, WorkspaceMode},
};

use super::{
    EditorSceneCamera2d, MainViewportCamera, PrimaryPointerOwner, PrimaryPointerOwnership,
    navigation::{CameraNavigation2d, ViewportNavigationState},
    snap::{
        SmartSnap2dMatch, Snap2dSettings, shift_pressed, smart_snap_2d, snap_angle_radians,
        snap_scalar, snap_vec2,
    },
    ui_edit::{UiEditorHitTarget, ui_editor_window_rect},
};

const SELECTION_COLOR: Color = Color::srgb(0.09, 0.61, 0.82);
const X_AXIS_COLOR: Color = Color::srgb(0.96, 0.24, 0.31);
const Y_AXIS_COLOR: Color = Color::srgb(0.42, 0.78, 0.20);
const SMART_GUIDE_COLOR: Color = Color::srgba(0.12, 0.82, 0.94, 0.95);
const HANDLE_RADIUS_PX: f32 = 4.5;
const HANDLE_HIT_RADIUS_PX: f32 = 10.0;
const AXIS_START_PX: f32 = 8.0;
const AXIS_LENGTH_PX: f32 = 58.0;
const ROTATE_RADIUS_PX: f32 = 48.0;
const MIN_SCALE: f32 = 0.01;

pub(super) struct SpriteEditorPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct SpriteEditing2d;

impl Plugin for SpriteEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpriteEditState>()
            .init_resource::<SelectionSet>()
            .init_resource::<PrimaryPointerOwnership>()
            .add_systems(Update, attach_sprite_selection)
            .add_systems(
                Update,
                edit_selected_2d
                    .after(CameraNavigation2d)
                    .in_set(SpriteEditing2d),
            )
            .add_systems(
                PostUpdate,
                draw_selected_2d_gizmo.after(TransformSystems::Propagate),
            );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpriteHandle {
    NorthWest,
    North,
    NorthEast,
    West,
    East,
    SouthWest,
    South,
    SouthEast,
}

impl SpriteHandle {
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

    const fn horizontal(self) -> i8 {
        match self {
            Self::NorthWest | Self::West | Self::SouthWest => -1,
            Self::North | Self::South => 0,
            Self::NorthEast | Self::East | Self::SouthEast => 1,
        }
    }

    const fn vertical(self) -> i8 {
        match self {
            Self::NorthWest | Self::North | Self::NorthEast => -1,
            Self::West | Self::East => 0,
            Self::SouthWest | Self::South | Self::SouthEast => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis2d {
    X,
    Y,
}

#[derive(Clone, Copy, Debug)]
enum SpriteEditOperation {
    Move {
        start_cursor_parent: Vec2,
        axis_parent: Option<Vec2>,
    },
    Resize {
        handle: SpriteHandle,
        fixed_local: Vec2,
        grabbed_local: Vec2,
        fixed_parent: Vec2,
        cursor_offset_parent: Vec2,
    },
    Rotate {
        start_angle: f32,
    },
    AxisScale {
        axis: Axis2d,
        axis_parent: Vec2,
        start_projection: f32,
    },
}

#[derive(Clone, Copy, Debug)]
struct ActiveSpriteEdit {
    entity: Entity,
    start_transform: Transform,
    parent_inverse: Option<Affine3A>,
    world_z: f32,
    local_bounds: Option<SpriteBounds>,
    operation: SpriteEditOperation,
}

#[derive(Clone, Copy, Debug)]
struct GroupSpriteMove {
    entity: Entity,
    start_transform: Transform,
    parent_inverse: Option<Affine3A>,
}

impl GroupSpriteMove {
    fn world_vector_to_parent(self, vector: Vec2) -> Vec2 {
        self.parent_inverse
            .map(|inverse| inverse.transform_vector3(vector.extend(0.0)).truncate())
            .unwrap_or(vector)
    }
}

impl ActiveSpriteEdit {
    fn cursor_parent(self, cursor_world: Vec2) -> Vec2 {
        self.parent_inverse
            .map(|inverse| {
                inverse
                    .transform_point3(cursor_world.extend(self.world_z))
                    .truncate()
            })
            .unwrap_or(cursor_world)
    }

    fn parent_point_to_world(self, point: Vec2) -> Vec2 {
        self.parent_inverse
            .map(|inverse| {
                inverse
                    .inverse()
                    .transform_point3(point.extend(self.start_transform.translation.z))
                    .truncate()
            })
            .unwrap_or(point)
    }

    fn world_point_to_parent(self, point: Vec2) -> Vec2 {
        self.parent_inverse
            .map(|inverse| {
                inverse
                    .transform_point3(point.extend(self.world_z))
                    .truncate()
            })
            .unwrap_or(point)
    }

    fn world_vector_to_parent(self, vector: Vec2) -> Vec2 {
        self.parent_inverse
            .map(|inverse| inverse.transform_vector3(vector.extend(0.0)).truncate())
            .unwrap_or(vector)
    }
}

#[derive(Resource, Default)]
pub(super) struct SpriteEditState {
    active: Option<ActiveSpriteEdit>,
    before: Option<SceneSnapshot>,
    guides: SmartSnapGuides,
    group_move: Vec<GroupSpriteMove>,
    skip_primary_move: bool,
}

impl SpriteEditState {
    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SmartSnapGuides {
    x: Option<f32>,
    y: Option<f32>,
}

impl From<SmartSnap2dMatch> for SmartSnapGuides {
    fn from(value: SmartSnap2dMatch) -> Self {
        Self {
            x: value.x.map(|snap| snap.guide),
            y: value.y.map(|snap| snap.guide),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WorldSpriteBounds {
    min: Vec2,
    max: Vec2,
}

impl WorldSpriteBounds {
    fn from_local(bounds: SpriteBounds, affine: Affine3A) -> Self {
        let points = bounds
            .corners()
            .map(|point| affine.transform_point3(point.extend(0.0)).truncate());
        let mut min = points[0];
        let mut max = points[0];
        for point in points.into_iter().skip(1) {
            min = min.min(point);
            max = max.max(point);
        }
        Self { min, max }
    }

    fn x_values(self) -> [f32; 3] {
        [self.min.x, (self.min.x + self.max.x) * 0.5, self.max.x]
    }

    fn y_values(self) -> [f32; 3] {
        [self.min.y, (self.min.y + self.max.y) * 0.5, self.max.y]
    }
}

#[derive(Debug)]
struct SmartSnapTargets {
    x: Vec<f32>,
    y: Vec<f32>,
}

impl Default for SmartSnapTargets {
    fn default() -> Self {
        Self {
            x: vec![0.0],
            y: vec![0.0],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SpriteBounds {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl SpriteBounds {
    pub(super) fn corners(self) -> [Vec2; 4] {
        [
            Vec2::new(self.left, self.top),
            Vec2::new(self.right, self.top),
            Vec2::new(self.right, self.bottom),
            Vec2::new(self.left, self.bottom),
        ]
    }

    fn handle(self, handle: SpriteHandle) -> Vec2 {
        let center = Vec2::new(
            (self.left + self.right) * 0.5,
            (self.top + self.bottom) * 0.5,
        );
        Vec2::new(
            match handle.horizontal() {
                -1 => self.left,
                1 => self.right,
                _ => center.x,
            },
            match handle.vertical() {
                -1 => self.top,
                1 => self.bottom,
                _ => center.y,
            },
        )
    }

    fn opposite(self, handle: SpriteHandle) -> Vec2 {
        let center = Vec2::new(
            (self.left + self.right) * 0.5,
            (self.top + self.bottom) * 0.5,
        );
        Vec2::new(
            match handle.horizontal() {
                -1 => self.right,
                1 => self.left,
                _ => center.x,
            },
            match handle.vertical() {
                -1 => self.bottom,
                1 => self.top,
                _ => center.y,
            },
        )
    }

    fn contains(self, point: Vec2) -> bool {
        point.x >= self.left
            && point.x <= self.right
            && point.y <= self.top
            && point.y >= self.bottom
    }
}

fn ui_editor_target_blocks_sprite(
    visibility: &InheritedVisibility,
    rect: Option<Rect>,
    cursor_screen: Vec2,
) -> bool {
    visibility.get() && rect.is_some_and(|rect| rect.contains(cursor_screen))
}

fn attach_sprite_selection(mut commands: Commands, sprites: Query<Entity, Added<SceneSprite2D>>) {
    for entity in &sprites {
        commands.entity(entity).observe(on_mesh_click);
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn edit_selected_2d(
    mode: Res<WorkspaceMode>,
    view: Res<EditorViewMode>,
    mut selection: ResMut<Selection>,
    navigation: Res<ViewportNavigationState>,
    settings: Res<TransformGizmoSettings>,
    snap: Res<Snap2dSettings>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<
        (&Camera, &GlobalTransform, &Projection),
        (With<EditorSceneCamera2d>, With<MainViewportCamera>),
    >,
    images: Option<Res<Assets<Image>>>,
    parent_globals: Query<&GlobalTransform>,
    mut state: ResMut<SpriteEditState>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<
            (
                &SceneSpace,
                &mut Transform,
                &GlobalTransform,
                Option<&SceneSprite2D>,
                Option<&Sprite>,
                Option<&ChildOf>,
            ),
            With<EditableObject>,
        >,
        Query<
            (
                Entity,
                &SceneSpace,
                &GlobalTransform,
                &SceneSprite2D,
                &Sprite,
            ),
            With<EditableObject>,
        >,
        ResMut<SelectionSet>,
        Query<(&SceneNodeId, &SceneParentId)>,
        Query<(), With<SceneUiLayout>>,
        Query<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), With<UiEditorHitTarget>>,
        ResMut<PrimaryPointerOwnership>,
    )>,
) {
    if state.active.is_some() && !mouse.pressed(MouseButton::Left) {
        let active = state.active.take();
        let before = state.before.take();
        state.guides = default();
        state.group_move.clear();
        state.skip_primary_move = false;
        if let (Some(_), Some(before), Some(history), Some(document)) = (
            active,
            before,
            history.as_deref_mut(),
            document.as_deref_mut(),
        ) {
            let after = {
                let history_nodes = nodes.p0();
                capture_scene_snapshot(&history_nodes, &selection, *mode)
            };
            history.commit("Edit 2D Transform", before, after, document);
        }
        return;
    }

    if *mode != WorkspaceMode::TwoD
        || *view != EditorViewMode::TwoD
        || navigation.is_navigating()
        || !navigation.pointer_over()
    {
        state.guides = default();
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_global, projection)) = camera.single() else {
        return;
    };
    let Some(cursor_screen) = window.cursor_position() else {
        return;
    };
    let Ok(cursor_world) = camera.viewport_to_world_2d(camera_global, cursor_screen) else {
        return;
    };
    let world_per_pixel = projection_world_per_pixel(projection);

    if let Some(active) = state.active {
        let snapping = snap.effective(shift_pressed(&keyboard));
        let excluded: Vec<_> = nodes.p3().entities(&selection).collect();
        let smart_targets = snapping.then(|| {
            let candidates = nodes.p2();
            collect_smart_snap_targets(&excluded, &candidates, images.as_deref())
        });
        let mut editable = nodes.p1();
        let primary_result = {
            let Ok((space, mut transform, _, _, _, _)) = editable.get_mut(active.entity) else {
                state.active = None;
                state.before = None;
                state.guides = default();
                state.group_move.clear();
                return;
            };
            if *space != SceneSpace::TwoD {
                state.active = None;
                state.before = None;
                state.guides = default();
                state.group_move.clear();
                return;
            }
            let cursor_parent = active.cursor_parent(cursor_world);
            let previous = *transform;
            apply_active_edit(&mut transform, active, cursor_parent, snapping, *snap);
            state.guides = smart_targets
                .as_ref()
                .map(|targets| {
                    apply_smart_snap(
                        &mut transform,
                        active,
                        targets,
                        snap.smart_distance_px * world_per_pixel,
                    )
                })
                .unwrap_or_default();
            let world_delta = active.parent_point_to_world(transform.translation.truncate())
                - active.parent_point_to_world(active.start_transform.translation.truncate());
            let changed = !state.skip_primary_move && *transform != previous;
            if state.skip_primary_move {
                *transform = previous;
            }
            (world_delta, changed)
        };
        let mut changed = primary_result.1;
        if matches!(active.operation, SpriteEditOperation::Move { .. }) {
            for member in state.group_move.iter().copied() {
                let Ok((SceneSpace::TwoD, mut transform, _, _, _, _)) =
                    editable.get_mut(member.entity)
                else {
                    continue;
                };
                let mut next = member.start_transform;
                let delta = member.world_vector_to_parent(primary_result.0);
                next.translation.x += delta.x;
                next.translation.y += delta.y;
                if *transform != next {
                    *transform = next;
                    changed = true;
                }
            }
        }
        if changed {
            mark_document_changed(document.as_deref_mut());
        }
        return;
    }

    if !mouse.just_pressed(MouseButton::Left)
        || keyboard.pressed(KeyCode::Space)
        || keyboard.pressed(KeyCode::AltLeft)
        || keyboard.pressed(KeyCode::AltRight)
        || navigation.blocks_primary_selection()
    {
        return;
    }
    {
        let ownership = nodes.p7();
        if ownership.is_claimed() && !ownership.is_owned_by(PrimaryPointerOwner::Sprite) {
            return;
        }
    }
    if nodes.p6().iter().any(|(node, transform, visibility)| {
        ui_editor_target_blocks_sprite(
            visibility,
            ui_editor_window_rect(camera, node, transform),
            cursor_screen,
        )
    }) {
        return;
    }
    // An already-selected object owns its visible body and transform gizmo.
    // Only fall back to Z-ordered picking when the current selection was not hit.
    let selected_edit_hit = selection.0.is_some_and(|selected| {
        let mut editable = nodes.p1();
        editable
            .get_mut(selected)
            .ok()
            .is_some_and(|(space, _, global, sprite_data, sprite, _)| {
                if *space != SceneSpace::TwoD {
                    return false;
                }
                let bounds = sprite_data.zip(sprite).map(|(data, sprite)| {
                    local_sprite_bounds(sprite_size(sprite, images.as_deref()), data.anchor)
                });
                selected_sprite_edit_hit(cursor_world, world_per_pixel, global, bounds, &settings)
            })
    });
    let clicked_sprite = if selected_edit_hit {
        selection.0
    } else {
        let candidates = nodes.p2();
        candidates
            .iter()
            .filter(|(_, space, _, data, _)| **space == SceneSpace::TwoD && data.visible)
            .filter_map(|(entity, _, global, data, sprite)| {
                let bounds =
                    local_sprite_bounds(sprite_size(sprite, images.as_deref()), data.anchor);
                let local = global
                    .affine()
                    .inverse()
                    .transform_point3(cursor_world.extend(global.translation().z))
                    .truncate();
                bounds
                    .contains(local)
                    .then_some((entity, global.translation().z))
            })
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(entity, _)| entity)
    };
    if let Some(clicked) = clicked_sprite {
        {
            let mut ownership = nodes.p7();
            if !ownership.claim(PrimaryPointerOwner::Sprite) {
                return;
            }
            if selected_edit_hit {
                ownership.lock_selection(PrimaryPointerOwner::Sprite);
            }
        }
        let extend_selection = control_pressed(&keyboard);
        nodes
            .p3()
            .select_from_click(&mut selection, clicked, extend_selection, false);
    }
    let Some(entity) = selection.0 else { return };

    let active = {
        let mut editable = nodes.p1();
        let Ok((space, transform, global, sprite_data, sprite, parent)) = editable.get_mut(entity)
        else {
            return;
        };
        if *space != SceneSpace::TwoD {
            return;
        }
        let parent_global = parent.and_then(|parent| parent_globals.get(parent.parent()).ok());
        let cursor_parent = world_to_parent(cursor_world, global.translation().z, parent_global);
        let bounds = sprite_data.zip(sprite).map(|(data, sprite)| {
            local_sprite_bounds(sprite_size(sprite, images.as_deref()), data.anchor)
        });
        let (axis_x_world, axis_y_world) = gizmo_axes(global, settings.mode, settings.space);
        let origin_world = global.translation().truncate();
        let axis_start = AXIS_START_PX * world_per_pixel;
        let axis_length = AXIS_LENGTH_PX * world_per_pixel;
        let hit_radius = HANDLE_HIT_RADIUS_PX * world_per_pixel;

        let resize = bounds.and_then(|bounds| {
            if settings.mode == TransformGizmoMode::Rotate {
                return None;
            }
            SpriteHandle::ALL.into_iter().find(|handle| {
                let world = global
                    .transform_point(bounds.handle(*handle).extend(0.0))
                    .truncate();
                cursor_world.distance(world) <= hit_radius
            })
        });

        let operation = if let (Some(bounds), Some(handle)) = (bounds, resize) {
            let grabbed_local = bounds.handle(handle);
            let fixed_local = bounds.opposite(handle);
            let grabbed_parent = transform
                .transform_point(grabbed_local.extend(0.0))
                .truncate();
            let fixed_parent = transform
                .transform_point(fixed_local.extend(0.0))
                .truncate();
            SpriteEditOperation::Resize {
                handle,
                fixed_local,
                grabbed_local,
                fixed_parent,
                cursor_offset_parent: cursor_parent - grabbed_parent,
            }
        } else {
            match settings.mode {
                TransformGizmoMode::Translate => {
                    let x_start = origin_world + axis_x_world * axis_start;
                    let x_end = origin_world + axis_x_world * axis_length;
                    let y_start = origin_world + axis_y_world * axis_start;
                    let y_end = origin_world + axis_y_world * axis_length;
                    let axis_parent = if point_segment_distance(cursor_world, x_start, x_end)
                        <= hit_radius
                    {
                        Some(world_vector_to_parent(axis_x_world, parent_global))
                    } else if point_segment_distance(cursor_world, y_start, y_end) <= hit_radius {
                        Some(world_vector_to_parent(axis_y_world, parent_global))
                    } else {
                        None
                    };
                    let inside_sprite = bounds.is_some_and(|bounds| {
                        let local = global
                            .affine()
                            .inverse()
                            .transform_point3(cursor_world.extend(global.translation().z))
                            .truncate();
                        bounds.contains(local)
                    });
                    if axis_parent.is_none() && !inside_sprite {
                        return;
                    }
                    SpriteEditOperation::Move {
                        start_cursor_parent: cursor_parent,
                        axis_parent,
                    }
                }
                TransformGizmoMode::Rotate => {
                    let radius = ROTATE_RADIUS_PX * world_per_pixel;
                    if (cursor_world.distance(origin_world) - radius).abs() > hit_radius {
                        return;
                    }
                    SpriteEditOperation::Rotate {
                        start_angle: (cursor_parent - transform.translation.truncate()).to_angle(),
                    }
                }
                TransformGizmoMode::Scale => {
                    let x_end = origin_world + axis_x_world * axis_length;
                    let y_end = origin_world + axis_y_world * axis_length;
                    let (axis, axis_world) = if cursor_world.distance(x_end) <= hit_radius {
                        (Axis2d::X, axis_x_world)
                    } else if cursor_world.distance(y_end) <= hit_radius {
                        (Axis2d::Y, axis_y_world)
                    } else {
                        return;
                    };
                    let axis_parent = world_vector_to_parent(axis_world, parent_global);
                    let start_projection = (cursor_parent - transform.translation.truncate())
                        .dot(axis_parent)
                        .abs()
                        .max(0.0001);
                    SpriteEditOperation::AxisScale {
                        axis,
                        axis_parent,
                        start_projection,
                    }
                }
            }
        };

        ActiveSpriteEdit {
            entity,
            start_transform: *transform,
            parent_inverse: parent_global.map(|parent| parent.affine().inverse()),
            world_z: global.translation().z,
            local_bounds: bounds,
            operation,
        }
    };

    {
        let mut ownership = nodes.p7();
        if !ownership.claim(PrimaryPointerOwner::Sprite) {
            return;
        }
    }

    if !control_pressed(&keyboard) {
        nodes.p3().select_only(&mut selection, active.entity);
    }

    let before = {
        let history_nodes = nodes.p0();
        capture_scene_snapshot(&history_nodes, &selection, *mode)
    };
    state.group_move.clear();
    state.skip_primary_move = false;
    if matches!(active.operation, SpriteEditOperation::Move { .. }) {
        let selected_entities: Vec<_> = nodes.p3().entities(&selection).collect();
        let moving_entities = {
            let hierarchy = nodes.p4();
            top_level_sprite_selection(&selected_entities, &hierarchy)
        };
        state.skip_primary_move = !moving_entities.contains(&active.entity);
        let ui_entities: std::collections::HashSet<_> = {
            let ui = nodes.p5();
            moving_entities
                .iter()
                .copied()
                .filter(|entity| ui.get(*entity).is_ok())
                .collect()
        };
        let mut editable = nodes.p1();
        for entity in moving_entities {
            if entity == active.entity {
                continue;
            }
            if ui_entities.contains(&entity) {
                continue;
            }
            let Ok((SceneSpace::TwoD, transform, _, _, _, parent)) = editable.get_mut(entity)
            else {
                continue;
            };
            let parent_inverse = parent
                .and_then(|parent| parent_globals.get(parent.parent()).ok())
                .map(|parent| parent.affine().inverse());
            state.group_move.push(GroupSpriteMove {
                entity,
                start_transform: *transform,
                parent_inverse,
            });
        }
    }
    state.active = Some(active);
    state.before = Some(before);
}

fn control_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight)
}

fn apply_active_edit(
    transform: &mut Transform,
    active: ActiveSpriteEdit,
    cursor_parent: Vec2,
    snapping: bool,
    snap: Snap2dSettings,
) {
    match active.operation {
        SpriteEditOperation::Move {
            start_cursor_parent,
            axis_parent,
        } => {
            let mut delta = cursor_parent - start_cursor_parent;
            if let Some(axis) = axis_parent {
                delta = axis * delta.dot(axis);
            }
            let start = active.start_transform.translation.truncate();
            let mut target = start + delta;
            if snapping {
                target = if let Some(axis) = axis_parent {
                    let offset = snap.grid_offset.dot(axis);
                    let projection = snap_scalar(target.dot(axis), snap.grid_size, offset);
                    start + axis * (projection - start.dot(axis))
                } else {
                    snap_vec2(target, snap.grid_size, snap.grid_offset)
                };
            }
            transform.translation.x = target.x;
            transform.translation.y = target.y;
        }
        SpriteEditOperation::Resize {
            handle,
            fixed_local,
            grabbed_local,
            fixed_parent,
            cursor_offset_parent,
        } => apply_resize(
            transform,
            active.start_transform,
            handle,
            fixed_local,
            grabbed_local,
            fixed_parent,
            cursor_offset_parent,
            cursor_parent,
            snapping.then_some(snap.grid_size),
        ),
        SpriteEditOperation::Rotate { start_angle } => {
            let angle = (cursor_parent - active.start_transform.translation.truncate()).to_angle();
            let delta = angle - start_angle;
            let (x, y, start_z) = active.start_transform.rotation.to_euler(EulerRot::XYZ);
            let target = start_z + delta;
            let target = if snapping {
                snap_angle_radians(target, snap.rotation_step_degrees)
            } else {
                target
            };
            transform.rotation = Quat::from_euler(EulerRot::XYZ, x, y, target);
        }
        SpriteEditOperation::AxisScale {
            axis,
            axis_parent,
            start_projection,
        } => {
            let projection =
                (cursor_parent - active.start_transform.translation.truncate()).dot(axis_parent);
            let factor = (projection / start_projection).max(MIN_SCALE);
            transform.scale = active.start_transform.scale;
            match axis {
                Axis2d::X => {
                    let value = active.start_transform.scale.x * factor;
                    transform.scale.x = if snapping {
                        snapped_scale(value, active.start_transform.scale.x, snap.scale_step)
                    } else {
                        clamp_scale(value, active.start_transform.scale.x)
                    };
                }
                Axis2d::Y => {
                    let value = active.start_transform.scale.y * factor;
                    transform.scale.y = if snapping {
                        snapped_scale(value, active.start_transform.scale.y, snap.scale_step)
                    } else {
                        clamp_scale(value, active.start_transform.scale.y)
                    };
                }
            }
        }
    }
}

fn collect_smart_snap_targets(
    excluded: &[Entity],
    candidates: &Query<
        (
            Entity,
            &SceneSpace,
            &GlobalTransform,
            &SceneSprite2D,
            &Sprite,
        ),
        With<EditableObject>,
    >,
    images: Option<&Assets<Image>>,
) -> SmartSnapTargets {
    let mut targets = SmartSnapTargets::default();
    for (entity, space, global, data, sprite) in candidates {
        if excluded.contains(&entity) || *space != SceneSpace::TwoD || !data.visible {
            continue;
        }
        let local = local_sprite_bounds(sprite_size(sprite, images), data.anchor);
        let world = WorldSpriteBounds::from_local(local, global.affine());
        targets.x.extend_from_slice(&world.x_values());
        targets.y.extend_from_slice(&world.y_values());
    }
    targets
}

fn top_level_sprite_selection(
    selected: &[Entity],
    hierarchy: &Query<(&SceneNodeId, &SceneParentId)>,
) -> Vec<Entity> {
    let selected_ids: std::collections::HashSet<_> = selected
        .iter()
        .filter_map(|entity| hierarchy.get(*entity).ok().map(|(id, _)| *id))
        .collect();
    let parents: std::collections::HashMap<_, _> = hierarchy
        .iter()
        .map(|(id, parent)| (*id, parent.0))
        .collect();
    selected
        .iter()
        .copied()
        .filter(|entity| {
            let mut parent = hierarchy.get(*entity).ok().and_then(|(_, parent)| parent.0);
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

fn apply_smart_snap(
    transform: &mut Transform,
    active: ActiveSpriteEdit,
    targets: &SmartSnapTargets,
    tolerance: f32,
) -> SmartSnapGuides {
    match active.operation {
        SpriteEditOperation::Move {
            axis_parent: None, ..
        } => {
            let Some(local_bounds) = active.local_bounds else {
                return default();
            };
            let world =
                WorldSpriteBounds::from_local(local_bounds, active_world_affine(active, transform));
            let snap = smart_snap_2d(
                &world.x_values(),
                &world.y_values(),
                &targets.x,
                &targets.y,
                tolerance,
            );
            let correction = Vec2::new(
                snap.x.map_or(0.0, |axis| axis.delta),
                snap.y.map_or(0.0, |axis| axis.delta),
            );
            let parent_correction = active.world_vector_to_parent(correction);
            transform.translation.x += parent_correction.x;
            transform.translation.y += parent_correction.y;
            snap.into()
        }
        SpriteEditOperation::Resize {
            handle,
            fixed_local,
            grabbed_local,
            fixed_parent,
            cursor_offset_parent,
        } => {
            let grabbed_parent = transform
                .transform_point(grabbed_local.extend(0.0))
                .truncate();
            let grabbed_world = active.parent_point_to_world(grabbed_parent);
            let moving_x = [grabbed_world.x];
            let moving_y = [grabbed_world.y];
            let snap = smart_snap_2d(
                if handle.horizontal() == 0 {
                    &[]
                } else {
                    &moving_x
                },
                if handle.vertical() == 0 {
                    &[]
                } else {
                    &moving_y
                },
                &targets.x,
                &targets.y,
                tolerance,
            );
            if snap == SmartSnap2dMatch::default() {
                return default();
            }
            let corrected_world = grabbed_world
                + Vec2::new(
                    snap.x.map_or(0.0, |axis| axis.delta),
                    snap.y.map_or(0.0, |axis| axis.delta),
                );
            let corrected_parent = active.world_point_to_parent(corrected_world);
            apply_resize(
                transform,
                active.start_transform,
                handle,
                fixed_local,
                grabbed_local,
                fixed_parent,
                cursor_offset_parent,
                corrected_parent + cursor_offset_parent,
                None,
            );
            snap.into()
        }
        SpriteEditOperation::Move {
            axis_parent: Some(_),
            ..
        }
        | SpriteEditOperation::Rotate { .. }
        | SpriteEditOperation::AxisScale { .. } => default(),
    }
}

fn active_world_affine(active: ActiveSpriteEdit, transform: &Transform) -> Affine3A {
    let local = transform.compute_affine();
    active
        .parent_inverse
        .map(|inverse| inverse.inverse() * local)
        .unwrap_or(local)
}

#[allow(clippy::too_many_arguments)]
fn apply_resize(
    transform: &mut Transform,
    start: Transform,
    handle: SpriteHandle,
    fixed_local: Vec2,
    grabbed_local: Vec2,
    fixed_parent: Vec2,
    cursor_offset_parent: Vec2,
    cursor_parent: Vec2,
    snap_step: Option<f32>,
) {
    let target_parent = cursor_parent - cursor_offset_parent;
    let delta_parent = target_parent - fixed_parent;
    let mut delta_local = (start.rotation.inverse() * delta_parent.extend(0.0)).truncate();
    if let Some(step) = snap_step {
        if handle.horizontal() != 0 {
            delta_local.x = snap_scalar(delta_local.x, step, 0.0);
        }
        if handle.vertical() != 0 {
            delta_local.y = snap_scalar(delta_local.y, step, 0.0);
        }
    }
    let span = grabbed_local - fixed_local;
    let mut scale = start.scale;
    if handle.horizontal() != 0 && span.x.abs() > f32::EPSILON {
        scale.x = clamp_scale(delta_local.x / span.x, start.scale.x);
    }
    if handle.vertical() != 0 && span.y.abs() > f32::EPSILON {
        scale.y = clamp_scale(delta_local.y / span.y, start.scale.y);
    }

    let fixed_offset = start.rotation * (scale * fixed_local.extend(0.0));
    transform.translation.x = fixed_parent.x - fixed_offset.x;
    transform.translation.y = fixed_parent.y - fixed_offset.y;
    transform.scale = scale;
}

fn snapped_scale(value: f32, original: f32, step: f32) -> f32 {
    clamp_scale(snap_scalar(value, step, 0.0), original)
}

fn clamp_scale(value: f32, original: f32) -> f32 {
    if original < 0.0 {
        value.min(-MIN_SCALE)
    } else {
        value.max(MIN_SCALE)
    }
}

#[allow(clippy::type_complexity)]
fn draw_selected_2d_gizmo(
    mode: Res<WorkspaceMode>,
    view: Res<EditorViewMode>,
    selection: Res<Selection>,
    selection_set: Res<SelectionSet>,
    settings: Res<TransformGizmoSettings>,
    state: Res<SpriteEditState>,
    camera: Query<
        (&GlobalTransform, &Projection),
        (With<EditorSceneCamera2d>, With<MainViewportCamera>),
    >,
    images: Option<Res<Assets<Image>>>,
    selected: Query<
        (
            Entity,
            &SceneSpace,
            &GlobalTransform,
            Option<&SceneSprite2D>,
            Option<&Sprite>,
        ),
        With<EditableObject>,
    >,
    mut gizmos: Gizmos,
) {
    if *mode != WorkspaceMode::TwoD || *view != EditorViewMode::TwoD {
        return;
    }
    let Some(entity) = selection.0 else { return };
    let Ok((_, SceneSpace::TwoD, global, sprite_data, sprite)) = selected.get(entity) else {
        return;
    };
    let Ok((camera_global, projection)) = camera.single() else {
        return;
    };
    let world_per_pixel = projection_world_per_pixel(projection);
    let origin = global.translation().truncate();

    for (candidate, space, candidate_global, data, sprite) in &selected {
        if candidate == entity
            || *space != SceneSpace::TwoD
            || !selection_set.contains(&selection, candidate)
        {
            continue;
        }
        let Some((data, sprite)) = data.zip(sprite) else {
            continue;
        };
        let bounds = local_sprite_bounds(sprite_size(sprite, images.as_deref()), data.anchor);
        let corners = bounds.corners().map(|point| {
            candidate_global
                .transform_point(point.extend(0.0))
                .truncate()
        });
        for index in 0..4 {
            gizmos.line_2d(corners[index], corners[(index + 1) % 4], SELECTION_COLOR);
        }
    }

    if state.active.is_some() {
        let view_size = match projection {
            Projection::Orthographic(projection) => projection.area.size(),
            Projection::Perspective(_) | Projection::Custom(_) => Vec2::splat(2048.0),
        };
        let center = camera_global.translation().truncate();
        let half_extent = view_size * 0.55;
        if let Some(x) = state.guides.x {
            gizmos.line_2d(
                Vec2::new(x, center.y - half_extent.y),
                Vec2::new(x, center.y + half_extent.y),
                SMART_GUIDE_COLOR,
            );
        }
        if let Some(y) = state.guides.y {
            gizmos.line_2d(
                Vec2::new(center.x - half_extent.x, y),
                Vec2::new(center.x + half_extent.x, y),
                SMART_GUIDE_COLOR,
            );
        }
    }

    if let Some((data, sprite)) = sprite_data.zip(sprite) {
        let bounds = local_sprite_bounds(sprite_size(sprite, images.as_deref()), data.anchor);
        let corners = bounds
            .corners()
            .map(|point| global.transform_point(point.extend(0.0)).truncate());
        for index in 0..4 {
            gizmos.line_2d(corners[index], corners[(index + 1) % 4], SELECTION_COLOR);
        }
        let radius = HANDLE_RADIUS_PX * world_per_pixel;
        for handle in SpriteHandle::ALL {
            let point = global
                .transform_point(bounds.handle(handle).extend(0.0))
                .truncate();
            gizmos.circle_2d(
                Isometry2d::from_translation(point),
                radius + world_per_pixel,
                Color::WHITE,
            );
            gizmos.circle_2d(Isometry2d::from_translation(point), radius, SELECTION_COLOR);
        }
    }

    let (axis_x, axis_y) = gizmo_axes(global, settings.mode, settings.space);
    let axis_start = AXIS_START_PX * world_per_pixel;
    let axis_length = AXIS_LENGTH_PX * world_per_pixel;
    match settings.mode {
        TransformGizmoMode::Translate => {
            gizmos.arrow_2d(
                origin + axis_x * axis_start,
                origin + axis_x * axis_length,
                X_AXIS_COLOR,
            );
            gizmos.arrow_2d(
                origin + axis_y * axis_start,
                origin + axis_y * axis_length,
                Y_AXIS_COLOR,
            );
            gizmos.rect_2d(
                Isometry2d::from_translation(origin),
                Vec2::splat(7.0 * world_per_pixel),
                Color::WHITE,
            );
        }
        TransformGizmoMode::Rotate => {
            gizmos.circle_2d(
                Isometry2d::from_translation(origin),
                ROTATE_RADIUS_PX * world_per_pixel,
                Color::srgb(0.98, 0.72, 0.20),
            );
        }
        TransformGizmoMode::Scale => {
            let x_end = origin + axis_x * axis_length;
            let y_end = origin + axis_y * axis_length;
            gizmos.line_2d(origin + axis_x * axis_start, x_end, X_AXIS_COLOR);
            gizmos.line_2d(origin + axis_y * axis_start, y_end, Y_AXIS_COLOR);
            let tip_size = Vec2::splat(8.0 * world_per_pixel);
            gizmos.rect_2d(Isometry2d::from_translation(x_end), tip_size, X_AXIS_COLOR);
            gizmos.rect_2d(Isometry2d::from_translation(y_end), tip_size, Y_AXIS_COLOR);
        }
    }
}

pub(super) fn sprite_size(sprite: &Sprite, images: Option<&Assets<Image>>) -> Vec2 {
    sprite
        .custom_size
        .or_else(|| sprite.rect.map(|rect| rect.size()))
        .or_else(|| images.and_then(|images| images.get(&sprite.image).map(Image::size_f32)))
        .unwrap_or(Vec2::splat(64.0))
        .max(Vec2::ONE)
}

pub(super) fn local_sprite_bounds(size: Vec2, anchor: (f32, f32)) -> SpriteBounds {
    // Collision rectangles use an extended anchor to represent a local offset.
    // Normal Sprite2D pivots remain constrained by the Inspector to 0..=1.
    let anchor = Vec2::new(anchor.0, anchor.1);
    SpriteBounds {
        left: -anchor.x * size.x,
        right: (1.0 - anchor.x) * size.x,
        top: anchor.y * size.y,
        bottom: -(1.0 - anchor.y) * size.y,
    }
}

fn projection_world_per_pixel(projection: &Projection) -> f32 {
    match projection {
        Projection::Orthographic(projection) => projection.scale.max(0.0001),
        Projection::Perspective(_) | Projection::Custom(_) => 1.0,
    }
}

fn gizmo_axes(
    global: &GlobalTransform,
    mode: TransformGizmoMode,
    space: TransformGizmoSpace,
) -> (Vec2, Vec2) {
    if mode == TransformGizmoMode::Translate && space == TransformGizmoSpace::World {
        return (Vec2::X, Vec2::Y);
    }
    let rotation = global.rotation();
    (
        (rotation * Vec3::X).truncate().normalize_or_zero(),
        (rotation * Vec3::Y).truncate().normalize_or_zero(),
    )
}

fn world_to_parent(world: Vec2, world_z: f32, parent_global: Option<&GlobalTransform>) -> Vec2 {
    parent_global
        .map(|parent| {
            parent
                .affine()
                .inverse()
                .transform_point3(world.extend(world_z))
                .truncate()
        })
        .unwrap_or(world)
}

fn world_vector_to_parent(vector: Vec2, parent_global: Option<&GlobalTransform>) -> Vec2 {
    parent_global
        .map(|parent| {
            parent
                .affine()
                .inverse()
                .transform_vector3(vector.extend(0.0))
                .truncate()
                .normalize_or_zero()
        })
        .unwrap_or(vector.normalize_or_zero())
}

fn point_segment_distance(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    if segment.length_squared() <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / segment.length_squared()).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

fn selected_sprite_edit_hit(
    cursor_world: Vec2,
    world_per_pixel: f32,
    global: &GlobalTransform,
    bounds: Option<SpriteBounds>,
    settings: &TransformGizmoSettings,
) -> bool {
    let hit_radius = HANDLE_HIT_RADIUS_PX * world_per_pixel;
    if settings.mode != TransformGizmoMode::Rotate
        && bounds.is_some_and(|bounds| {
            SpriteHandle::ALL.into_iter().any(|handle| {
                cursor_world.distance(
                    global
                        .transform_point(bounds.handle(handle).extend(0.0))
                        .truncate(),
                ) <= hit_radius
            })
        })
    {
        return true;
    }

    let origin = global.translation().truncate();
    let (axis_x, axis_y) = gizmo_axes(global, settings.mode, settings.space);
    let axis_start = AXIS_START_PX * world_per_pixel;
    let axis_length = AXIS_LENGTH_PX * world_per_pixel;
    match settings.mode {
        TransformGizmoMode::Translate => {
            let on_axis = [axis_x, axis_y].into_iter().any(|axis| {
                point_segment_distance(
                    cursor_world,
                    origin + axis * axis_start,
                    origin + axis * axis_length,
                ) <= hit_radius
            });
            let inside_body = bounds.is_some_and(|bounds| {
                let local = global
                    .affine()
                    .inverse()
                    .transform_point3(cursor_world.extend(global.translation().z))
                    .truncate();
                bounds.contains(local)
            });
            on_axis || inside_body
        }
        TransformGizmoMode::Rotate => {
            (cursor_world.distance(origin) - ROTATE_RADIUS_PX * world_per_pixel).abs() <= hit_radius
        }
        TransformGizmoMode::Scale => [axis_x, axis_y]
            .into_iter()
            .any(|axis| cursor_world.distance(origin + axis * axis_length) <= hit_radius),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_ui_editor_target_does_not_block_sprite_editing() {
        let rect = Rect::from_corners(Vec2::ZERO, Vec2::splat(100.0));

        assert!(!ui_editor_target_blocks_sprite(
            &InheritedVisibility::HIDDEN,
            Some(rect),
            Vec2::splat(50.0),
        ));
    }

    #[test]
    fn visible_ui_editor_target_blocks_sprite_editing_inside_its_rect() {
        let rect = Rect::from_corners(Vec2::ZERO, Vec2::splat(100.0));

        assert!(ui_editor_target_blocks_sprite(
            &InheritedVisibility::VISIBLE,
            Some(rect),
            Vec2::splat(50.0),
        ));
        assert!(!ui_editor_target_blocks_sprite(
            &InheritedVisibility::VISIBLE,
            Some(rect),
            Vec2::splat(150.0),
        ));
    }

    #[test]
    fn selected_sprite_body_and_gizmo_keep_pointer_priority() {
        let global = GlobalTransform::IDENTITY;
        let bounds = local_sprite_bounds(Vec2::splat(64.0), (0.0, 0.0));
        let mut settings = TransformGizmoSettings::default();
        settings.mode = TransformGizmoMode::Translate;

        assert!(selected_sprite_edit_hit(
            Vec2::new(32.0, -32.0),
            1.0,
            &global,
            Some(bounds),
            &settings,
        ));
        assert!(selected_sprite_edit_hit(
            Vec2::new(36.0, 0.0),
            1.0,
            &global,
            None,
            &settings,
        ));
        assert!(!selected_sprite_edit_hit(
            Vec2::splat(256.0),
            1.0,
            &global,
            Some(bounds),
            &settings,
        ));
    }

    #[test]
    fn group_move_converts_one_world_delta_for_each_parent() {
        let parent = Affine3A::from_scale(Vec3::splat(2.0));
        let member = GroupSpriteMove {
            entity: Entity::from_bits(1),
            start_transform: Transform::default(),
            parent_inverse: Some(parent.inverse()),
        };

        assert_eq!(
            member.world_vector_to_parent(Vec2::new(12.0, -8.0)),
            Vec2::new(6.0, -4.0)
        );
    }

    fn active_edit(start_transform: Transform, operation: SpriteEditOperation) -> ActiveSpriteEdit {
        ActiveSpriteEdit {
            entity: Entity::PLACEHOLDER,
            start_transform,
            parent_inverse: None,
            world_z: 0.0,
            local_bounds: None,
            operation,
        }
    }

    #[test]
    fn top_left_anchor_places_the_transform_at_the_sprite_corner() {
        let bounds = local_sprite_bounds(Vec2::new(64.0, 32.0), (0.0, 0.0));
        assert_eq!(bounds.corners()[0], Vec2::ZERO);
        assert_eq!(bounds.corners()[2], Vec2::new(64.0, -32.0));
    }

    #[test]
    fn south_east_resize_scales_from_a_fixed_top_left_pivot() {
        let bounds = local_sprite_bounds(Vec2::splat(64.0), (0.0, 0.0));
        let mut transform = Transform::default();
        apply_resize(
            &mut transform,
            Transform::default(),
            SpriteHandle::SouthEast,
            bounds.opposite(SpriteHandle::SouthEast),
            bounds.handle(SpriteHandle::SouthEast),
            Vec2::ZERO,
            Vec2::ZERO,
            Vec2::new(128.0, -128.0),
            None,
        );
        assert_eq!(transform.translation, Vec3::ZERO);
        assert_eq!(transform.scale, Vec3::new(2.0, 2.0, 1.0));
    }

    #[test]
    fn move_snaps_to_the_absolute_grid() {
        let mut transform = Transform::default();
        let active = active_edit(
            Transform::default(),
            SpriteEditOperation::Move {
                start_cursor_parent: Vec2::ZERO,
                axis_parent: None,
            },
        );

        apply_active_edit(
            &mut transform,
            active,
            Vec2::new(47.0, -18.0),
            true,
            Snap2dSettings::default(),
        );

        assert_eq!(transform.translation.truncate(), Vec2::new(32.0, -32.0));
    }

    #[test]
    fn smart_move_aligns_the_closest_sprite_edge() {
        let bounds = local_sprite_bounds(Vec2::splat(100.0), (0.0, 0.0));
        let mut active = active_edit(
            Transform::default(),
            SpriteEditOperation::Move {
                start_cursor_parent: Vec2::ZERO,
                axis_parent: None,
            },
        );
        active.local_bounds = Some(bounds);
        let mut transform = Transform::from_xyz(92.0, 0.0, 0.0);
        let targets = SmartSnapTargets {
            x: vec![200.0],
            y: vec![500.0],
        };

        let guides = apply_smart_snap(&mut transform, active, &targets, 8.0);

        assert_eq!(transform.translation.x, 100.0);
        assert_eq!(guides.x, Some(200.0));
        assert_eq!(guides.y, None);
    }

    #[test]
    fn rotation_snaps_to_fifteen_degrees() {
        let mut transform = Transform::default();
        let active = active_edit(
            Transform::default(),
            SpriteEditOperation::Rotate { start_angle: 0.0 },
        );
        let angle = 22.0_f32.to_radians();

        apply_active_edit(
            &mut transform,
            active,
            Vec2::new(angle.cos(), angle.sin()),
            true,
            Snap2dSettings::default(),
        );

        let (_, _, rotation) = transform.rotation.to_euler(EulerRot::XYZ);
        assert!((rotation.to_degrees() - 15.0).abs() < 0.001);
    }

    #[test]
    fn snapped_resize_recomputes_from_the_fixed_corner() {
        let bounds = local_sprite_bounds(Vec2::splat(64.0), (0.0, 0.0));
        let mut transform = Transform::default();
        for cursor in [Vec2::new(70.0, -70.0), Vec2::new(94.0, -94.0)] {
            apply_resize(
                &mut transform,
                Transform::default(),
                SpriteHandle::SouthEast,
                bounds.opposite(SpriteHandle::SouthEast),
                bounds.handle(SpriteHandle::SouthEast),
                Vec2::ZERO,
                Vec2::ZERO,
                cursor,
                Some(32.0),
            );
        }

        assert_eq!(transform.translation, Vec3::ZERO);
        assert_eq!(transform.scale, Vec3::new(1.5, 1.5, 1.0));
    }

    #[test]
    fn smart_resize_aligns_the_dragged_corner_and_keeps_the_opposite_fixed() {
        let bounds = local_sprite_bounds(Vec2::splat(64.0), (0.0, 0.0));
        let operation = SpriteEditOperation::Resize {
            handle: SpriteHandle::SouthEast,
            fixed_local: bounds.opposite(SpriteHandle::SouthEast),
            grabbed_local: bounds.handle(SpriteHandle::SouthEast),
            fixed_parent: Vec2::ZERO,
            cursor_offset_parent: Vec2::ZERO,
        };
        let active = active_edit(Transform::default(), operation);
        let mut transform = Transform::default();
        apply_resize(
            &mut transform,
            Transform::default(),
            SpriteHandle::SouthEast,
            bounds.opposite(SpriteHandle::SouthEast),
            bounds.handle(SpriteHandle::SouthEast),
            Vec2::ZERO,
            Vec2::ZERO,
            Vec2::new(96.0, -96.0),
            None,
        );
        let targets = SmartSnapTargets {
            x: vec![100.0],
            y: vec![-100.0],
        };

        let guides = apply_smart_snap(&mut transform, active, &targets, 8.0);

        assert_eq!(transform.translation, Vec3::ZERO);
        assert_eq!(transform.scale, Vec3::new(1.5625, 1.5625, 1.0));
        assert_eq!(guides.x, Some(100.0));
        assert_eq!(guides.y, Some(-100.0));
    }

    #[test]
    fn active_edit_keeps_the_parent_inverse_captured_at_drag_start() {
        let parent = GlobalTransform::from(Transform::from_xyz(100.0, 50.0, 0.0));
        let mut active = active_edit(
            Transform::default(),
            SpriteEditOperation::Move {
                start_cursor_parent: Vec2::ZERO,
                axis_parent: None,
            },
        );
        active.parent_inverse = Some(parent.affine().inverse());

        assert_eq!(
            active.cursor_parent(Vec2::new(116.0, 82.0)),
            Vec2::new(16.0, 32.0)
        );
    }

    #[test]
    fn segment_hit_distance_is_clamped_to_the_handle_length() {
        assert_eq!(
            point_segment_distance(Vec2::new(5.0, 3.0), Vec2::ZERO, Vec2::X * 10.0),
            3.0
        );
        assert_eq!(
            point_segment_distance(Vec2::new(14.0, 0.0), Vec2::ZERO, Vec2::X * 10.0),
            4.0
        );
    }

    #[test]
    fn edit_system_initializes_without_conflicting_queries() {
        let mut app = App::new();
        app.init_resource::<WorkspaceMode>()
            .init_resource::<EditorViewMode>()
            .init_resource::<Selection>()
            .init_resource::<SelectionSet>()
            .init_resource::<ViewportNavigationState>()
            .init_resource::<TransformGizmoSettings>()
            .init_resource::<Snap2dSettings>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<SpriteEditState>()
            .init_resource::<PrimaryPointerOwnership>()
            .add_systems(Update, edit_selected_2d);

        app.update();

        {
            let mut state = app.world_mut().resource_mut::<SpriteEditState>();
            state.active = Some(active_edit(
                Transform::default(),
                SpriteEditOperation::Move {
                    start_cursor_parent: Vec2::ZERO,
                    axis_parent: None,
                },
            ));
            state.guides = SmartSnapGuides {
                x: Some(10.0),
                y: Some(20.0),
            };
        }
        app.update();

        let state = app.world().resource::<SpriteEditState>();
        assert!(state.active.is_none());
        assert_eq!(state.guides, SmartSnapGuides::default());
    }
}
