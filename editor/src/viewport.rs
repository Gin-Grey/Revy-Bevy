use bevy::{
    camera::{Projection, Viewport, visibility::RenderLayers},
    gizmos::transform_gizmo::{
        TransformGizmoCamera, TransformGizmoMode, TransformGizmoPlugin, TransformGizmoSettings,
        TransformGizmoSpace, TransformGizmoSystems,
    },
    prelude::*,
    ui::UiGlobalTransform,
    window::PrimaryWindow,
};

mod marquee;
mod navigation;
mod ownership;
mod snap;
mod sprite_edit;
mod ui_edit;

use marquee::MarqueeSelectionPlugin;
use navigation::{
    EditorCamera2dNavigation, EditorCamera3dNavigation, ViewportNavigationPlugin,
    gizmo_input_allowed,
};
use ownership::release_primary_pointer_ownership;
use snap::{snap_vec2, visible_grid_step};
use sprite_edit::SpriteEditorPlugin;
use ui_edit::{UiEditorPlugin, spawn_ui_preview_canvas};

pub(crate) use navigation::ViewportNavigationState;
pub(crate) use ownership::{PrimaryPointerOwner, PrimaryPointerOwnership};
pub(crate) use snap::Snap2dSettings;

use crate::{
    project_settings::ProjectDisplaySettings,
    workspace::{EditorViewMode, WorkspaceMode},
};

/// Identifies one rendered viewport panel in the editor shell.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EditorViewportSlot {
    #[default]
    Main,
    TopRight,
    BottomRight,
}

/// Marks a UI node whose rectangle is rendered by scene cameras.
#[derive(Component, Clone, Copy, Default)]
pub struct EditorViewportPane(pub EditorViewportSlot);

impl EditorViewportPane {
    pub const fn main() -> Self {
        Self(EditorViewportSlot::Main)
    }

    pub const fn top_right() -> Self {
        Self(EditorViewportSlot::TopRight)
    }

    pub const fn bottom_right() -> Self {
        Self(EditorViewportSlot::BottomRight)
    }
}

/// 3D world camera rendered into [`EditorViewportPane`].
#[derive(Component, Clone, Copy, Default)]
pub struct EditorSceneCamera3d(pub EditorViewportSlot);

/// 2D world camera rendered into [`EditorViewportPane`].
#[derive(Component, Clone, Copy, Default)]
pub struct EditorSceneCamera2d(pub EditorViewportSlot);

/// Marks the interactive viewport camera; preview cameras only render.
#[derive(Component, Clone, Copy, Default)]
pub struct MainViewportCamera;

// Bevy 0.19's private transform-gizmo overlay camera renders exclusively on layer 15.
const TRANSFORM_GIZMO_RENDER_LAYER: usize = 15;

pub struct ViewportPlugin;

impl Plugin for ViewportPlugin {
    fn build(&self, app: &mut App) {
        let snap_settings = Snap2dSettings::load(app.world().get_resource());
        app.insert_resource(snap_settings)
            .init_resource::<PrimaryPointerOwnership>();
        app.add_plugins((
            MeshPickingPlugin,
            TransformGizmoPlugin,
            ViewportNavigationPlugin,
            SpriteEditorPlugin,
            UiEditorPlugin,
            MarqueeSelectionPlugin,
        ))
        .configure_sets(
            PostUpdate,
            TransformGizmoSystems.run_if(gizmo_input_allowed),
        )
        .add_systems(Startup, setup_viewport)
        .add_systems(
            PostUpdate,
            sync_scene_camera_viewports.after(TransformSystems::Propagate),
        )
        .add_systems(
            Update,
            (
                gizmo_mode_keys,
                draw_workspace_guides,
                release_primary_pointer_ownership.after(marquee::MarqueeSelection2d),
            ),
        );
    }
}

pub fn setup_viewport(mut commands: Commands, display: Res<ProjectDisplaySettings>) {
    // UI chrome uses the full-window default camera, independent of scene cameras.
    commands.spawn((
        Camera2d,
        Camera {
            // The gizmo renderer reserves order 1 for its overlay camera.
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.125, 0.137, 0.149)),
            ..default()
        },
        IsDefaultUiCamera,
    ));

    // 3D scene cameras (default workspace).
    let camera_3d_transform =
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y);
    for (slot, order, transform) in [
        (EditorViewportSlot::Main, 0, camera_3d_transform),
        (
            EditorViewportSlot::TopRight,
            2,
            Transform::from_xyz(0.0, 12.0, 0.001).looking_at(Vec3::ZERO, Vec3::Z),
        ),
        (
            EditorViewportSlot::BottomRight,
            3,
            Transform::from_xyz(10.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        ),
    ] {
        let mut entity = commands.spawn((
            Camera3d::default(),
            Camera {
                order,
                is_active: true,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.16, 0.17, 0.19)),
                ..default()
            },
            transform,
            EditorSceneCamera3d(slot),
        ));
        if slot == EditorViewportSlot::Main {
            entity.insert((
                EditorCamera3dNavigation::from_view(&transform, Vec3::new(0.0, 0.5, 0.0)),
                TransformGizmoCamera,
                MainViewportCamera,
            ));
        }
    }

    // 2D scene cameras (inactive until workspace switches).
    let mut main_camera_2d = None;
    let screen_center = project_screen_rect(&display).center();
    let camera_2d_transform = Transform::from_xyz(screen_center.x, screen_center.y, 1000.0);
    for (slot, order, transform) in [
        (EditorViewportSlot::Main, 0, camera_2d_transform),
        (EditorViewportSlot::TopRight, 2, camera_2d_transform),
        (EditorViewportSlot::BottomRight, 3, camera_2d_transform),
    ] {
        let mut entity = commands.spawn((
            Camera2d,
            Camera {
                order,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.14, 0.15, 0.18)),
                ..default()
            },
            transform,
            EditorSceneCamera2d(slot),
        ));
        if slot == EditorViewportSlot::Main {
            entity.insert((EditorCamera2dNavigation::default(), MainViewportCamera));
            main_camera_2d = Some(entity.id());
        }
    }
    let camera_2d = main_camera_2d.expect("main 2D editor camera should be spawned");
    spawn_ui_preview_canvas(&mut commands, camera_2d);
}

// Bevy system parameters encode disjoint ECS access in their types.
#[allow(clippy::type_complexity)]
fn sync_scene_camera_viewports(
    panes: Query<(&EditorViewportPane, &ComputedNode, &UiGlobalTransform)>,
    view: Res<EditorViewMode>,
    mut cameras: Query<(
        &mut Camera,
        Option<&EditorSceneCamera3d>,
        Option<&EditorSceneCamera2d>,
        Option<&RenderLayers>,
    )>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let win = UVec2::new(
        window.physical_width().max(1),
        window.physical_height().max(1),
    );

    let mut pane_rects = Vec::new();
    for (pane, node, transform) in &panes {
        let size = node.size();
        if size.x < 1.0 || size.y < 1.0 {
            continue;
        }
        let rect = Rect::from_center_size(transform.translation.trunc(), size);
        let mut physical_position = UVec2::new(
            rect.min.x.round().max(0.0) as u32,
            rect.min.y.round().max(0.0) as u32,
        );
        let mut physical_size = UVec2::new(
            size.x.round().max(1.0) as u32,
            size.y.round().max(1.0) as u32,
        );
        if physical_position.x >= win.x || physical_position.y >= win.y {
            continue;
        }
        physical_size = physical_size.min(win - physical_position);
        physical_position = physical_position.min(win - UVec2::ONE);
        pane_rects.push((pane.0, physical_position, physical_size));
    }

    let gizmo_layer = RenderLayers::layer(TRANSFORM_GIZMO_RENDER_LAYER);
    for (mut camera, scene_3d, scene_2d, render_layers) in &mut cameras {
        let is_scene_camera = scene_3d.is_some() || scene_2d.is_some();
        let is_gizmo_overlay =
            camera.order == 1 && render_layers.is_some_and(|layers| layers == &gizmo_layer);
        let slot = scene_3d
            .map(|camera| camera.0)
            .or_else(|| scene_2d.map(|camera| camera.0))
            .unwrap_or(EditorViewportSlot::Main);
        if !is_scene_camera && !is_gizmo_overlay {
            continue;
        }

        camera.is_active = match (scene_3d, scene_2d) {
            (Some(_), _) => *view == EditorViewMode::ThreeD,
            (_, Some(_)) => *view == EditorViewMode::TwoD,
            _ => camera.is_active,
        };

        let viewport = pane_rects
            .iter()
            .find(|(pane_slot, _, _)| *pane_slot == slot)
            .map(|(_, physical_position, physical_size)| Viewport {
                physical_position: *physical_position,
                physical_size: *physical_size,
                depth: 0.0..1.0,
            });
        if is_scene_camera && viewport.is_none() {
            camera.is_active = false;
        }
        camera.viewport = viewport;
    }
}

fn gizmo_mode_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<TransformGizmoSettings>,
) {
    if keyboard.just_pressed(KeyCode::Digit1) {
        settings.mode = TransformGizmoMode::Translate;
    }
    if keyboard.just_pressed(KeyCode::Digit2) {
        settings.mode = TransformGizmoMode::Rotate;
    }
    if keyboard.just_pressed(KeyCode::Digit3) {
        settings.mode = TransformGizmoMode::Scale;
    }
    if keyboard.just_pressed(KeyCode::KeyX) {
        settings.space = match settings.space {
            TransformGizmoSpace::World => TransformGizmoSpace::Local,
            TransformGizmoSpace::Local => TransformGizmoSpace::World,
        };
    }
}

/// Draws mode-specific editor guides without adding them to the scene itself.
fn draw_workspace_guides(
    mode: Res<WorkspaceMode>,
    snap: Res<Snap2dSettings>,
    display: Res<ProjectDisplaySettings>,
    camera_2d: Query<
        (&Transform, &Projection),
        (
            With<EditorSceneCamera2d>,
            With<EditorCamera2dNavigation>,
            With<MainViewportCamera>,
        ),
    >,
    mut gizmos: Gizmos,
) {
    match *mode {
        WorkspaceMode::ThreeD => {
            gizmos.grid_3d(
                Isometry3d::from_translation(Vec3::new(0.0, 0.002, 0.0)),
                UVec3::new(40, 0, 40),
                Vec3::ONE,
                Color::srgba(0.34, 0.36, 0.40, 0.46),
            );
        }
        WorkspaceMode::TwoD => {
            let (grid_center, view_size, grid_step) = camera_2d
                .single()
                .ok()
                .and_then(|(transform, projection)| {
                    let Projection::Orthographic(projection) = projection else {
                        return None;
                    };
                    let step = visible_grid_step(snap.grid_size, projection.scale);
                    let center = transform.translation.truncate();
                    let snapped_center = snap_vec2(center, step, snap.grid_offset);
                    Some((snapped_center, projection.area.size(), step))
                })
                .unwrap_or((snap.grid_offset, Vec2::new(2048.0, 1280.0), snap.grid_size));
            let cell_count = UVec2::new(
                ((view_size.x / grid_step).ceil() as u32 + 4).clamp(2, 256),
                ((view_size.y / grid_step).ceil() as u32 + 4).clamp(2, 256),
            );
            if snap.grid_visible {
                gizmos.grid_2d(
                    Isometry2d::from_translation(grid_center),
                    cell_count,
                    Vec2::splat(grid_step),
                    Color::srgba(0.32, 0.34, 0.38, 0.42),
                );
            }
            // Revy 的 2D 编辑坐标以左上角为 (0, 0)，因此游戏画面向右、
            // 向下（世界坐标负 Y）延伸。该框仅是编辑器辅助线，不会进入场景文件。
            let screen_rect = project_screen_rect(&display);
            gizmos.rect_2d(
                Isometry2d::from_translation(screen_rect.center()),
                screen_rect.size(),
                Color::srgba(0.56, 0.50, 0.88, 0.95),
            );
            let axis_extent = view_size.max_element().max(2048.0);
            gizmos.line_2d(
                Vec2::new(grid_center.x - axis_extent, 0.0),
                Vec2::new(grid_center.x + axis_extent, 0.0),
                Color::srgb(0.58, 0.22, 0.25),
            );
            gizmos.line_2d(
                Vec2::new(0.0, grid_center.y - axis_extent),
                Vec2::new(0.0, grid_center.y + axis_extent),
                Color::srgb(0.24, 0.55, 0.32),
            );
        }
    }
}

fn project_screen_rect(display: &ProjectDisplaySettings) -> Rect {
    Rect::from_corners(
        Vec2::new(0.0, -(display.height as f32)),
        Vec2::new(display.width as f32, 0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_screen_starts_at_top_left_and_extends_downward() {
        let rect = project_screen_rect(&ProjectDisplaySettings {
            width: 960,
            height: 540,
        });

        assert_eq!(rect.min, Vec2::new(0.0, -540.0));
        assert_eq!(rect.max, Vec2::new(960.0, 0.0));
        assert_eq!(rect.center(), Vec2::new(480.0, -270.0));
        assert_eq!(rect.size(), Vec2::new(960.0, 540.0));
    }
}
