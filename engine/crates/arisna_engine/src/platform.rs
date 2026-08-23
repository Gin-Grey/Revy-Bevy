//! Process-wide platform configuration applied before Bevy starts its worker threads.

use bevy::{
    prelude::*,
    render::{
        settings::{Backends, WgpuSettings},
        RenderPlugin,
    },
};

/// Configures backend options that wgpu currently exposes through environment variables.
pub fn configure_process() {
    #[cfg(target_os = "windows")]
    configure_windows_dx12();
}

/// Configures wgpu to use the primary native graphics API on each desktop platform.
pub fn native_render_plugin() -> RenderPlugin {
    RenderPlugin {
        render_creation: WgpuSettings {
            backends: Some(if cfg!(target_os = "windows") {
                Backends::DX12
            } else if cfg!(target_os = "macos") {
                Backends::METAL
            } else {
                Backends::VULKAN
            }),
            ..default()
        }
        .into(),
        ..default()
    }
}

#[cfg(target_os = "windows")]
fn configure_windows_dx12() {
    const PRESENTATION_SYSTEM: &str = "WGPU_DX12_PRESENTATION_SYSTEM";

    // DirectComposition avoids a wgpu 29 HWND swap-chain resize failure for
    // normal top-level windows. Embedded child windows require an HWND
    // swapchain because DirectComposition cannot claim a child surface that
    // is already attached to the editor window.
    let presentation_system = if std::env::var_os("REVY_EMBED_PARENT_HWND").is_some()
        || std::env::var_os("ARISNA_EMBED_PARENT_HWND").is_some()
    {
        "DxgiFromHwnd"
    } else {
        "DxgiFromVisual"
    };
    // SAFETY: This runs at process startup, before Bevy creates worker threads.
    unsafe {
        std::env::set_var(PRESENTATION_SYSTEM, presentation_system);
    }
}

pub struct PlatformPlugin;

impl Plugin for PlatformPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_os = "windows")]
        app.init_resource::<LastDrawableWindowSize>()
            .add_systems(First, preserve_surface_size_while_minimized);
    }
}

#[cfg(target_os = "windows")]
#[derive(Resource)]
struct LastDrawableWindowSize(UVec2);

#[cfg(target_os = "windows")]
impl Default for LastDrawableWindowSize {
    fn default() -> Self {
        Self(UVec2::new(1280, 720))
    }
}

#[cfg(target_os = "windows")]
fn preserve_surface_size_while_minimized(
    mut last_size: ResMut<LastDrawableWindowSize>,
    mut primary_window: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let Ok(mut window) = primary_window.single_mut() else {
        return;
    };

    let current_size = window.resolution.physical_size();
    let surface_size = stable_surface_size(current_size, &mut last_size.0);
    if surface_size != current_size {
        window
            .bypass_change_detection()
            .resolution
            .set_physical_resolution(surface_size.x, surface_size.y);
    }
}

#[cfg(target_os = "windows")]
fn stable_surface_size(current: UVec2, last: &mut UVec2) -> UVec2 {
    if current.x > 1 && current.y > 1 {
        *last = current;
    }
    *last
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn minimized_window_keeps_the_last_drawable_surface_size() {
        let mut last = UVec2::new(1920, 1080);

        assert_eq!(stable_surface_size(UVec2::ZERO, &mut last), last);
        assert_eq!(stable_surface_size(UVec2::ONE, &mut last), last);
        assert_eq!(
            stable_surface_size(UVec2::new(2560, 1440), &mut last),
            UVec2::new(2560, 1440)
        );
    }
}
