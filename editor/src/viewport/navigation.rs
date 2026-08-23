use std::f32::consts::FRAC_PI_2;

use bevy::{
    camera::{Projection, primitives::Aabb},
    ecs::system::SystemParam,
    gizmos::transform_gizmo::TransformGizmoState,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    ui::UiGlobalTransform,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};

use crate::{
    entities::NeedsModel3dFocus,
    hierarchy::SceneRootMenuState,
    selection::Selection,
    workspace::{EditorViewMode, SceneSpace, WorkspaceMode},
};

use super::{
    EditorSceneCamera2d, EditorSceneCamera3d, EditorViewportPane, EditorViewportSlot,
    MainViewportCamera, PrimaryPointerOwner, PrimaryPointerOwnership,
};

const LOOK_SENSITIVITY: f32 = 0.0035;
const ORBIT_SENSITIVITY: f32 = 0.004;
const MIN_PITCH: f32 = -FRAC_PI_2 + 0.01;
const MAX_PITCH: f32 = FRAC_PI_2 - 0.01;

pub(super) struct ViewportNavigationPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct CameraNavigation2d;

impl Plugin for ViewportNavigationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewportNavigationState>()
            .init_resource::<PrimaryPointerOwnership>()
            .add_systems(
                Update,
                (
                    update_navigation_capture,
                    navigate_camera_3d,
                    navigate_camera_2d.in_set(CameraNavigation2d),
                    focus_selected_object,
                    focus_new_model,
                )
                    .chain(),
            );
    }
}

/// Persistent orbit and fly-camera values for the 3D workspace.
#[derive(Component, Debug)]
pub(super) struct EditorCamera3dNavigation {
    pivot: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    fly_speed: f32,
}

impl EditorCamera3dNavigation {
    pub(super) fn from_view(transform: &Transform, pivot: Vec3) -> Self {
        let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        Self {
            pivot,
            yaw,
            pitch,
            distance: transform.translation.distance(pivot),
            fly_speed: 5.0,
        }
    }
}

/// 2D zoom limits live beside the camera so future document views can differ.
#[derive(Component, Debug)]
pub(super) struct EditorCamera2dNavigation {
    min_scale: f32,
    max_scale: f32,
}

impl Default for EditorCamera2dNavigation {
    fn default() -> Self {
        Self {
            min_scale: 0.05,
            max_scale: 32.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum NavigationGesture {
    #[default]
    None,
    Fly3d,
    Orbit3d,
    Pan3d,
    Pan2dMiddle,
    Pan2dSpace,
}

impl NavigationGesture {
    fn belongs_to(self, mode: WorkspaceMode) -> bool {
        matches!(
            (self, mode),
            (
                Self::Fly3d | Self::Orbit3d | Self::Pan3d,
                WorkspaceMode::ThreeD
            ) | (Self::Pan2dMiddle | Self::Pan2dSpace, WorkspaceMode::TwoD)
                | (Self::None, _)
        )
    }

    fn is_held(self, mouse: &ButtonInput<MouseButton>, keyboard: &ButtonInput<KeyCode>) -> bool {
        match self {
            Self::None => true,
            Self::Fly3d => mouse.pressed(MouseButton::Right),
            Self::Orbit3d => mouse.pressed(MouseButton::Left) && alt_pressed(keyboard),
            Self::Pan3d | Self::Pan2dMiddle => mouse.pressed(MouseButton::Middle),
            Self::Pan2dSpace => {
                mouse.pressed(MouseButton::Left) && keyboard.pressed(KeyCode::Space)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SavedCursorOptions {
    visible: bool,
    grab_mode: CursorGrabMode,
}

/// Shared gesture ownership keeps scene navigation from leaking into editor UI.
#[derive(Resource, Debug, Default)]
pub(crate) struct ViewportNavigationState {
    pointer_over: bool,
    gesture: NavigationGesture,
    saved_cursor: Option<SavedCursorOptions>,
    suppress_primary_click: bool,
}

impl ViewportNavigationState {
    pub(crate) fn pointer_over(&self) -> bool {
        self.pointer_over
    }

    pub(crate) fn blocks_primary_selection(&self) -> bool {
        self.suppress_primary_click
    }

    pub(crate) fn is_navigating(&self) -> bool {
        self.gesture != NavigationGesture::None
    }
}

#[derive(SystemParam)]
struct MouseNavigationInput<'w> {
    buttons: Res<'w, ButtonInput<MouseButton>>,
    motion: Res<'w, AccumulatedMouseMotion>,
    scroll: Res<'w, AccumulatedMouseScroll>,
}

fn update_navigation_capture(
    mode: Res<WorkspaceMode>,
    view: Res<EditorViewMode>,
    scene_menu: Res<SceneRootMenuState>,
    pane: Query<(&EditorViewportPane, &ComputedNode, &UiGlobalTransform)>,
    mut primary_window: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gizmo: Res<TransformGizmoState>,
    mut navigation: ResMut<ViewportNavigationState>,
    mut ownership: ResMut<PrimaryPointerOwnership>,
) {
    let Ok((window, mut cursor)) = primary_window.single_mut() else {
        return;
    };

    if !matches!(*view, EditorViewMode::TwoD | EditorViewMode::ThreeD) {
        navigation.pointer_over = false;
        if navigation.gesture != NavigationGesture::None {
            release_navigation(&mut navigation, &mut cursor);
        }
        return;
    }

    navigation.pointer_over = pane
        .iter()
        .find(|(pane, _, _)| pane.0 == EditorViewportSlot::Main)
        .and_then(|(_, node, transform)| {
            window
                .physical_cursor_position()
                .map(|position| node.contains_point(*transform, position))
        })
        .unwrap_or(false);

    if scene_menu.any_open() {
        if navigation.gesture != NavigationGesture::None {
            release_navigation(&mut navigation, &mut cursor);
        }
        return;
    }

    // A camera drag's release can reach Picking after this system runs. Keep
    // suppression latched until the next unmodified primary press begins.
    if navigation.suppress_primary_click
        && navigation.gesture == NavigationGesture::None
        && mouse.just_pressed(MouseButton::Left)
        && !alt_pressed(&keyboard)
        && !keyboard.pressed(KeyCode::Space)
    {
        navigation.suppress_primary_click = false;
    }

    let should_release = navigation.gesture != NavigationGesture::None
        && (!window.focused
            || !navigation.gesture.belongs_to(*mode)
            || !navigation.gesture.is_held(&mouse, &keyboard));
    if should_release {
        release_navigation(&mut navigation, &mut cursor);
    }

    if navigation.gesture != NavigationGesture::None || !navigation.pointer_over || gizmo.active {
        return;
    }

    navigation.gesture = match *mode {
        WorkspaceMode::ThreeD if mouse.just_pressed(MouseButton::Right) => {
            navigation.saved_cursor = Some(SavedCursorOptions {
                visible: cursor.visible,
                grab_mode: cursor.grab_mode,
            });
            cursor.visible = false;
            cursor.grab_mode = CursorGrabMode::Locked;
            NavigationGesture::Fly3d
        }
        WorkspaceMode::ThreeD
            if mouse.just_pressed(MouseButton::Left) && alt_pressed(&keyboard) =>
        {
            if !ownership.claim(PrimaryPointerOwner::Navigation) {
                return;
            }
            navigation.suppress_primary_click = true;
            NavigationGesture::Orbit3d
        }
        WorkspaceMode::ThreeD if mouse.just_pressed(MouseButton::Middle) => {
            NavigationGesture::Pan3d
        }
        WorkspaceMode::TwoD if mouse.just_pressed(MouseButton::Middle) => {
            NavigationGesture::Pan2dMiddle
        }
        WorkspaceMode::TwoD
            if mouse.just_pressed(MouseButton::Left) && keyboard.pressed(KeyCode::Space) =>
        {
            if !ownership.claim(PrimaryPointerOwner::Navigation) {
                return;
            }
            navigation.suppress_primary_click = true;
            NavigationGesture::Pan2dSpace
        }
        _ => NavigationGesture::None,
    };
}

/// Keeps Bevy's transform gizmo from competing with camera mouse gestures.
pub(super) fn gizmo_input_allowed(
    mode: Res<WorkspaceMode>,
    view: Res<EditorViewMode>,
    navigation: Res<ViewportNavigationState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
) -> bool {
    *view == EditorViewMode::ThreeD
        && *mode == WorkspaceMode::ThreeD
        && navigation.gesture == NavigationGesture::None
        && !alt_pressed(&keyboard)
        && !keyboard.pressed(KeyCode::Space)
        && !mouse.pressed(MouseButton::Right)
        && !mouse.pressed(MouseButton::Middle)
}

fn release_navigation(navigation: &mut ViewportNavigationState, cursor: &mut CursorOptions) {
    if let Some(saved) = navigation.saved_cursor.take() {
        cursor.visible = saved.visible;
        cursor.grab_mode = saved.grab_mode;
    }
    navigation.gesture = NavigationGesture::None;
}

fn navigate_camera_3d(
    mode: Res<WorkspaceMode>,
    navigation: Res<ViewportNavigationState>,
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    input: MouseNavigationInput,
    mut camera: Query<
        (&mut Transform, &mut EditorCamera3dNavigation),
        (
            With<EditorSceneCamera3d>,
            With<EditorCamera3dNavigation>,
            With<MainViewportCamera>,
        ),
    >,
) {
    if *mode != WorkspaceMode::ThreeD {
        return;
    }
    let Ok((mut transform, mut controller)) = camera.single_mut() else {
        return;
    };

    let scroll = normalized_scroll(&input.scroll);
    if scroll != 0.0 {
        if navigation.gesture == NavigationGesture::Fly3d {
            controller.fly_speed =
                (controller.fly_speed * (scroll * 0.12).exp()).clamp(0.1, 5_000.0);
        } else if navigation.pointer_over {
            controller.distance =
                (controller.distance * (-scroll * 0.16).exp()).clamp(0.05, 100_000.0);
            apply_orbit_transform(&mut transform, &controller);
        }
    }

    match navigation.gesture {
        NavigationGesture::Fly3d => {
            if !input.buttons.just_pressed(MouseButton::Right) {
                controller.yaw -= input.motion.delta.x * LOOK_SENSITIVITY;
                controller.pitch = (controller.pitch - input.motion.delta.y * LOOK_SENSITIVITY)
                    .clamp(MIN_PITCH, MAX_PITCH);
                transform.rotation =
                    Quat::from_euler(EulerRot::YXZ, controller.yaw, controller.pitch, 0.0);
            }

            let mut input = Vec3::ZERO;
            input.z += axis(&keyboard, KeyCode::KeyW, KeyCode::KeyS);
            input.x += axis(&keyboard, KeyCode::KeyD, KeyCode::KeyA);
            input.y += axis(&keyboard, KeyCode::KeyE, KeyCode::KeyQ);
            if input != Vec3::ZERO {
                let speed = if shift_pressed(&keyboard) {
                    controller.fly_speed * 4.0
                } else {
                    controller.fly_speed
                };
                let movement = (transform.right().as_vec3() * input.x
                    + Vec3::Y * input.y
                    + transform.forward().as_vec3() * input.z)
                    .normalize_or_zero()
                    * speed
                    * time.delta_secs();
                transform.translation += movement;
            }

            controller.pivot =
                transform.translation + transform.forward().as_vec3() * controller.distance;
        }
        NavigationGesture::Orbit3d => {
            if !input.buttons.just_pressed(MouseButton::Left) {
                controller.yaw -= input.motion.delta.x * ORBIT_SENSITIVITY;
                controller.pitch = (controller.pitch - input.motion.delta.y * ORBIT_SENSITIVITY)
                    .clamp(MIN_PITCH, MAX_PITCH);
                apply_orbit_transform(&mut transform, &controller);
            }
        }
        NavigationGesture::Pan3d if !input.buttons.just_pressed(MouseButton::Middle) => {
            let world_per_dot = controller.distance.max(0.1) * 0.0015;
            let movement = -transform.right().as_vec3() * input.motion.delta.x * world_per_dot
                + transform.up().as_vec3() * input.motion.delta.y * world_per_dot;
            transform.translation += movement;
            controller.pivot += movement;
        }
        _ => {}
    }
}

fn navigate_camera_2d(
    mode: Res<WorkspaceMode>,
    navigation: Res<ViewportNavigationState>,
    input: MouseNavigationInput,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<
        (
            &Camera,
            &mut Transform,
            &mut Projection,
            &EditorCamera2dNavigation,
        ),
        (
            With<EditorSceneCamera2d>,
            With<EditorCamera2dNavigation>,
            With<MainViewportCamera>,
        ),
    >,
) {
    if *mode != WorkspaceMode::TwoD {
        return;
    }
    let Ok((camera, mut transform, mut projection, controller)) = camera.single_mut() else {
        return;
    };
    let Projection::Orthographic(orthographic) = &mut *projection else {
        return;
    };

    if matches!(
        navigation.gesture,
        NavigationGesture::Pan2dMiddle | NavigationGesture::Pan2dSpace
    ) {
        let just_started = input.buttons.just_pressed(MouseButton::Middle)
            || input.buttons.just_pressed(MouseButton::Left);
        if !just_started {
            transform.translation.x -= input.motion.delta.x * orthographic.scale;
            transform.translation.y += input.motion.delta.y * orthographic.scale;
        }
    }

    let scroll = normalized_scroll(&input.scroll);
    if !navigation.pointer_over || scroll == 0.0 {
        return;
    }

    let old_scale = orthographic.scale;
    let new_scale =
        (old_scale * (-scroll * 0.14).exp()).clamp(controller.min_scale, controller.max_scale);
    if (new_scale - old_scale).abs() <= f32::EPSILON {
        return;
    }

    if let Ok(window) = windows.single()
        && let (Some(viewport), Some(cursor)) = (
            camera.physical_viewport_rect(),
            window.physical_cursor_position(),
        )
    {
        let viewport_center = (viewport.min.as_vec2() + viewport.max.as_vec2()) * 0.5;
        let screen_offset = Vec2::new(cursor.x - viewport_center.x, viewport_center.y - cursor.y);
        let center = zoom_center_around_screen_point(
            transform.translation.truncate(),
            screen_offset,
            old_scale,
            new_scale,
        );
        transform.translation.x = center.x;
        transform.translation.y = center.y;
    }
    orthographic.scale = new_scale;
}

type FocusObjectQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static GlobalTransform,
        &'static Transform,
        &'static SceneSpace,
        Option<&'static Sprite>,
    ),
    (Without<EditorSceneCamera2d>, Without<EditorSceneCamera3d>),
>;
type FocusCamera3dQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut EditorCamera3dNavigation,
    ),
    (
        With<EditorSceneCamera3d>,
        With<EditorCamera3dNavigation>,
        With<MainViewportCamera>,
        Without<EditorSceneCamera2d>,
    ),
>;
type FocusCamera2dQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut Projection,
        &'static EditorCamera2dNavigation,
    ),
    (
        With<EditorSceneCamera2d>,
        With<EditorCamera2dNavigation>,
        With<MainViewportCamera>,
        Without<EditorSceneCamera3d>,
    ),
>;

fn focus_selected_object(
    mode: Res<WorkspaceMode>,
    navigation: Res<ViewportNavigationState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    selection: Res<Selection>,
    objects: FocusObjectQuery,
    mut camera_3d: FocusCamera3dQuery,
    mut camera_2d: FocusCamera2dQuery,
) {
    if !navigation.pointer_over
        || navigation.gesture != NavigationGesture::None
        || !keyboard.just_pressed(KeyCode::KeyF)
    {
        return;
    }
    let Some(entity) = selection.0 else {
        return;
    };
    let Ok((global, local, space, sprite)) = objects.get(entity) else {
        return;
    };

    match (*mode, *space) {
        (WorkspaceMode::ThreeD, SceneSpace::ThreeD) => {
            let Ok((mut transform, mut controller)) = camera_3d.single_mut() else {
                return;
            };
            controller.pivot = global.translation();
            controller.distance = (local.scale.abs().max_element() * 4.0).clamp(2.5, 100_000.0);
            apply_orbit_transform(&mut transform, &controller);
        }
        (WorkspaceMode::TwoD, SceneSpace::TwoD) => {
            let Ok((mut transform, mut projection, controller)) = camera_2d.single_mut() else {
                return;
            };
            transform.translation.x = global.translation().x;
            transform.translation.y = global.translation().y;
            if let (Some(sprite), Projection::Orthographic(orthographic)) =
                (sprite, &mut *projection)
            {
                let object_size =
                    sprite.custom_size.unwrap_or(Vec2::splat(64.0)) * local.scale.truncate().abs();
                orthographic.scale = (object_size.max_element() / 240.0)
                    .clamp(controller.min_scale, controller.max_scale);
            }
        }
        _ => {}
    }
}

fn focus_new_model(
    mut commands: Commands,
    mode: Res<WorkspaceMode>,
    navigation: Res<ViewportNavigationState>,
    selection: Res<Selection>,
    pending: Query<Entity, With<NeedsModel3dFocus>>,
    children: Query<&Children>,
    mesh_bounds: Query<(&Aabb, &GlobalTransform), With<Mesh3d>>,
    mut camera: FocusCamera3dQuery,
) {
    let selected = selection.0;
    for entity in &pending {
        if selected != Some(entity) {
            commands.entity(entity).remove::<NeedsModel3dFocus>();
        }
    }
    let Some(root) = selected else {
        return;
    };
    if *mode != WorkspaceMode::ThreeD
        || navigation.gesture != NavigationGesture::None
        || pending.get(root).is_err()
    {
        return;
    }

    let Some((minimum, maximum)) = descendant_world_bounds(root, &children, &mesh_bounds) else {
        return;
    };
    let Ok((mut transform, mut controller)) = camera.single_mut() else {
        return;
    };
    controller.pivot = (minimum + maximum) * 0.5;
    controller.distance = ((maximum - minimum).length() * 1.35).clamp(1.5, 100_000.0);
    apply_orbit_transform(&mut transform, &controller);
    commands.entity(root).remove::<NeedsModel3dFocus>();
}

fn descendant_world_bounds(
    root: Entity,
    children: &Query<&Children>,
    mesh_bounds: &Query<(&Aabb, &GlobalTransform), With<Mesh3d>>,
) -> Option<(Vec3, Vec3)> {
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let mut found = false;
    let mut stack = vec![root];

    while let Some(entity) = stack.pop() {
        if let Ok((aabb, global)) = mesh_bounds.get(entity) {
            let center = Vec3::from(aabb.center);
            let half = Vec3::from(aabb.half_extents);
            for x in [-half.x, half.x] {
                for y in [-half.y, half.y] {
                    for z in [-half.z, half.z] {
                        let point = global.transform_point(center + Vec3::new(x, y, z));
                        minimum = minimum.min(point);
                        maximum = maximum.max(point);
                        found = true;
                    }
                }
            }
        }
        if let Ok(entity_children) = children.get(entity) {
            stack.extend(entity_children.iter());
        }
    }

    found.then_some((minimum, maximum))
}

fn apply_orbit_transform(transform: &mut Transform, controller: &EditorCamera3dNavigation) {
    transform.rotation = Quat::from_euler(EulerRot::YXZ, controller.yaw, controller.pitch, 0.0);
    transform.translation = controller.pivot - transform.forward().as_vec3() * controller.distance;
}

fn axis(keyboard: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f32 {
    f32::from(keyboard.pressed(positive)) - f32::from(keyboard.pressed(negative))
}

fn alt_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight)
}

fn shift_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
}

fn normalized_scroll(scroll: &AccumulatedMouseScroll) -> f32 {
    match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    }
}

fn zoom_center_around_screen_point(
    center: Vec2,
    screen_offset: Vec2,
    old_scale: f32,
    new_scale: f32,
) -> Vec2 {
    center + screen_offset * (old_scale - new_scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_anchored_zoom_preserves_world_point() {
        let center = Vec2::new(10.0, -20.0);
        let cursor_offset = Vec2::new(120.0, 40.0);
        let old_scale = 2.0;
        let new_scale = 0.5;
        let old_world_point = center + cursor_offset * old_scale;

        let new_center =
            zoom_center_around_screen_point(center, cursor_offset, old_scale, new_scale);

        assert_eq!(new_center + cursor_offset * new_scale, old_world_point);
    }

    #[test]
    fn orbit_transform_keeps_requested_distance() {
        let mut transform =
            Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y);
        let controller = EditorCamera3dNavigation::from_view(&transform, Vec3::ZERO);

        apply_orbit_transform(&mut transform, &controller);

        assert!((transform.translation.length() - controller.distance).abs() < 0.0001);
    }
}
