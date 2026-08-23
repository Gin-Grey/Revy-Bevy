use bevy::{ecs::system::SystemParam, prelude::*, ui::UiGlobalTransform, window::PrimaryWindow};

use crate::{
    entities::{SceneSprite2D, SceneUiLayout},
    hierarchy::{SceneNodeId, SceneParentId},
    selection::{Selection, SelectionSet},
    workspace::{EditorViewMode, SceneSpace, WorkspaceMode},
};

use super::{
    EditorSceneCamera2d, MainViewportCamera, PrimaryPointerOwner, PrimaryPointerOwnership,
    navigation::{CameraNavigation2d, ViewportNavigationState},
    sprite_edit::{SpriteEditState, SpriteEditing2d, local_sprite_bounds, sprite_size},
    ui_edit::{
        UiEditState, UiPreviewCanvas, UiPreviewNode, UiSelectionMarquee, ui_preview_logical_rect,
    },
};

const DRAG_THRESHOLD_PX: f32 = 3.0;

#[derive(SystemParam)]
struct TransformEditOwnership<'w> {
    sprite: Res<'w, SpriteEditState>,
    ui: Res<'w, UiEditState>,
    primary: ResMut<'w, PrimaryPointerOwnership>,
}

impl TransformEditOwnership<'_> {
    fn is_active(&self) -> bool {
        self.sprite.is_active() || self.ui.is_active()
    }
}

#[derive(Resource, Default, Debug)]
struct MarqueeState {
    active: bool,
    start: Vec2,
    current: Vec2,
    shift: bool,
    control: bool,
}

impl MarqueeState {
    fn rect(&self) -> Rect {
        Rect::from_corners(self.start.min(self.current), self.start.max(self.current))
    }

    fn clear(&mut self) {
        self.active = false;
    }
}

pub(super) struct MarqueeSelectionPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct MarqueeSelection2d;

impl Plugin for MarqueeSelectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MarqueeState>()
            .init_resource::<SpriteEditState>()
            .init_resource::<UiEditState>()
            .init_resource::<SelectionSet>()
            .init_resource::<PrimaryPointerOwnership>()
            .add_systems(
                Update,
                update_marquee_selection
                    .after(CameraNavigation2d)
                    .after(SpriteEditing2d)
                    .in_set(MarqueeSelection2d),
            )
            .add_systems(PostUpdate, sync_marquee_overlay);
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_marquee_selection(
    mode: Res<WorkspaceMode>,
    view: Res<EditorViewMode>,
    navigation: Res<ViewportNavigationState>,
    mut edit_ownership: TransformEditOwnership,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<
        (&Camera, &GlobalTransform),
        (With<EditorSceneCamera2d>, With<MainViewportCamera>),
    >,
    images: Option<Res<Assets<Image>>>,
    sprites: Query<(
        Entity,
        &SceneSpace,
        &GlobalTransform,
        &SceneSprite2D,
        &Sprite,
    )>,
    ui_nodes: Query<(&UiPreviewNode, &ComputedNode, &UiGlobalTransform)>,
    ui_sources: Query<&SceneUiLayout>,
    hierarchy: Query<(&SceneNodeId, &SceneParentId)>,
    mut selection: ResMut<Selection>,
    mut selection_set: ResMut<SelectionSet>,
    mut state: ResMut<MarqueeState>,
) {
    if *mode != WorkspaceMode::TwoD || *view != EditorViewMode::TwoD {
        state.clear();
        return;
    }
    let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), camera.single()) else {
        state.clear();
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        if !mouse.pressed(MouseButton::Left) {
            state.clear();
        }
        return;
    };

    // Transform handles can sit outside the object's visual bounds. Once an
    // editor drag owns the primary pointer, it must never become a marquee.
    if edit_ownership.is_active() {
        state.clear();
        return;
    }

    if state.active {
        if !edit_ownership
            .primary
            .is_owned_by(PrimaryPointerOwner::Marquee)
        {
            state.clear();
            return;
        }
        state.current = cursor;
        if keyboard.just_pressed(KeyCode::Escape) {
            state.clear();
            return;
        }
        if mouse.just_released(MouseButton::Left) {
            let rect = state.rect();
            let moved = state.start.distance(state.current) >= DRAG_THRESHOLD_PX;
            let hits = moved.then(|| {
                collect_marquee_hits(
                    rect,
                    camera,
                    camera_transform,
                    &sprites,
                    images.as_deref(),
                    &ui_nodes,
                    &ui_sources,
                    &hierarchy,
                )
            });
            selection_set.select_from_box(
                &mut selection,
                hits.unwrap_or_default(),
                state.shift,
                state.control,
            );
            state.clear();
        } else if !mouse.pressed(MouseButton::Left) {
            state.clear();
        }
        return;
    }

    if !mouse.just_pressed(MouseButton::Left)
        || !navigation.pointer_over()
        || navigation.is_navigating()
        || navigation.blocks_primary_selection()
        || camera_modifier_pressed(&keyboard)
    {
        return;
    }

    if edit_ownership.primary.is_claimed() {
        return;
    }

    if selectable_at_cursor(
        cursor,
        camera,
        camera_transform,
        &sprites,
        images.as_deref(),
        &ui_nodes,
    ) {
        return;
    }

    if !edit_ownership.primary.claim(PrimaryPointerOwner::Marquee) {
        return;
    }

    state.active = true;
    state.start = cursor;
    state.current = cursor;
    state.shift = shift_pressed(&keyboard);
    state.control = control_pressed(&keyboard);
}

fn selectable_at_cursor(
    cursor: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    sprites: &Query<(
        Entity,
        &SceneSpace,
        &GlobalTransform,
        &SceneSprite2D,
        &Sprite,
    )>,
    images: Option<&Assets<Image>>,
    ui_nodes: &Query<(&UiPreviewNode, &ComputedNode, &UiGlobalTransform)>,
) -> bool {
    ui_nodes.iter().any(|(_, node, transform)| {
        ui_preview_window_rect(node, transform, camera).is_some_and(|rect| rect.contains(cursor))
    }) || sprites.iter().any(|(_, space, transform, data, sprite)| {
        *space == SceneSpace::TwoD
            && data.visible
            && sprite_logical_rect(camera, camera_transform, transform, data, sprite, images)
                .is_some_and(|rect| rect.contains(cursor))
    })
}

fn collect_marquee_hits(
    marquee: Rect,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    sprites: &Query<(
        Entity,
        &SceneSpace,
        &GlobalTransform,
        &SceneSprite2D,
        &Sprite,
    )>,
    images: Option<&Assets<Image>>,
    ui_nodes: &Query<(&UiPreviewNode, &ComputedNode, &UiGlobalTransform)>,
    ui_sources: &Query<&SceneUiLayout>,
    hierarchy: &Query<(&SceneNodeId, &SceneParentId)>,
) -> Vec<Entity> {
    let mut hits = Vec::new();
    for (entity, space, transform, data, sprite) in sprites {
        if *space == SceneSpace::TwoD
            && data.visible
            && sprite_logical_rect(camera, camera_transform, transform, data, sprite, images)
                .is_some_and(|rect| rects_intersect(marquee, rect))
        {
            hits.push(entity);
        }
    }
    for (preview, node, transform) in ui_nodes {
        if ui_sources.get(preview.source).is_ok()
            && ui_preview_window_rect(node, transform, camera)
                .is_some_and(|rect| rects_intersect(marquee, rect))
            && !hits.contains(&preview.source)
        {
            hits.push(preview.source);
        }
    }
    top_level_hits(hits, hierarchy)
}

fn top_level_hits(
    hits: Vec<Entity>,
    hierarchy: &Query<(&SceneNodeId, &SceneParentId)>,
) -> Vec<Entity> {
    let hit_ids: std::collections::HashSet<_> = hits
        .iter()
        .filter_map(|entity| hierarchy.get(*entity).ok().map(|(id, _)| *id))
        .collect();
    let parents: std::collections::HashMap<_, _> = hierarchy
        .iter()
        .map(|(id, parent)| (*id, parent.0))
        .collect();
    hits.into_iter()
        .filter(|entity| {
            let mut parent = hierarchy.get(*entity).ok().and_then(|(_, parent)| parent.0);
            while let Some(parent_id) = parent {
                if hit_ids.contains(&parent_id) {
                    return false;
                }
                parent = parents.get(&parent_id).copied().flatten();
            }
            true
        })
        .collect()
}

fn sprite_logical_rect(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    transform: &GlobalTransform,
    data: &SceneSprite2D,
    sprite: &Sprite,
    images: Option<&Assets<Image>>,
) -> Option<Rect> {
    let local = local_sprite_bounds(sprite_size(sprite, images), data.anchor);
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for corner in local.corners() {
        let world = transform.transform_point(corner.extend(0.0));
        let screen = camera.world_to_viewport(camera_transform, world).ok()?;
        min = min.min(screen);
        max = max.max(screen);
    }
    min.is_finite()
        .then_some(Rect::from_corners(min, max))
        .filter(|rect| rect.max.is_finite())
}

fn ui_preview_window_rect(
    node: &ComputedNode,
    transform: &UiGlobalTransform,
    camera: &Camera,
) -> Option<Rect> {
    let local = ui_preview_logical_rect(node, transform)?;
    let viewport_origin = camera.logical_viewport_rect()?.min;
    Some(translate_rect(local, viewport_origin))
}

fn sync_marquee_overlay(
    state: Res<MarqueeState>,
    camera: Query<&Camera, (With<EditorSceneCamera2d>, With<MainViewportCamera>)>,
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<UiPreviewCanvas>>,
    mut overlays: Query<&mut Node, With<UiSelectionMarquee>>,
) {
    let Ok(mut overlay) = overlays.single_mut() else {
        return;
    };
    let Some((camera, canvas_rect)) = camera.single().ok().zip(
        canvas
            .single()
            .ok()
            .and_then(|(node, transform)| ui_preview_logical_rect(node, transform)),
    ) else {
        overlay.display = Display::None;
        return;
    };
    let Some(viewport_origin) = camera.logical_viewport_rect().map(|rect| rect.min) else {
        overlay.display = Display::None;
        return;
    };
    if !state.active {
        overlay.display = Display::None;
        return;
    }

    let rect = translate_rect(state.rect(), -viewport_origin - canvas_rect.min);
    overlay.display = Display::Flex;
    overlay.left = Val::Px(rect.min.x);
    overlay.top = Val::Px(rect.min.y);
    overlay.width = Val::Px(rect.width());
    overlay.height = Val::Px(rect.height());
}

fn translate_rect(rect: Rect, offset: Vec2) -> Rect {
    Rect::from_corners(rect.min + offset, rect.max + offset)
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}

fn camera_modifier_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::Space)
        || keyboard.pressed(KeyCode::AltLeft)
        || keyboard.pressed(KeyCode::AltRight)
}

fn shift_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
}

fn control_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marquee_rect_normalizes_reverse_drag_and_intersects_edges() {
        let state = MarqueeState {
            active: true,
            start: Vec2::new(100.0, 80.0),
            current: Vec2::new(20.0, 30.0),
            ..default()
        };
        assert_eq!(
            state.rect(),
            Rect::from_corners(Vec2::new(20.0, 30.0), Vec2::new(100.0, 80.0))
        );
        assert!(rects_intersect(
            state.rect(),
            Rect::from_corners(Vec2::new(95.0, 70.0), Vec2::new(120.0, 100.0))
        ));
        assert!(!rects_intersect(
            state.rect(),
            Rect::from_corners(Vec2::new(101.0, 81.0), Vec2::new(120.0, 100.0))
        ));
    }

    #[test]
    fn viewport_origin_is_applied_once_for_hits_and_removed_for_overlay_layout() {
        let viewport_origin = Vec2::new(420.0, 238.0);
        let canvas_origin = Vec2::new(0.0, 0.0);
        let local_node = Rect::from_corners(Vec2::new(110.0, 80.0), Vec2::new(210.0, 180.0));
        let window_node = translate_rect(local_node, viewport_origin);
        assert_eq!(
            window_node,
            Rect::from_corners(Vec2::new(530.0, 318.0), Vec2::new(630.0, 418.0))
        );

        let overlay_layout = translate_rect(window_node, -viewport_origin - canvas_origin);
        assert_eq!(overlay_layout, local_node);
    }

    #[test]
    fn marquee_plugin_idles_without_a_window_or_camera() {
        let mut app = App::new();
        app.init_resource::<WorkspaceMode>()
            .init_resource::<EditorViewMode>()
            .init_resource::<ViewportNavigationState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Selection>()
            .add_plugins(MarqueeSelectionPlugin);

        app.update();

        assert!(!app.world().resource::<MarqueeState>().active);
    }
}
