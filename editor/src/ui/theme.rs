//! Design tokens shared by the editor shell and dynamic panels.
//!
//! Keeping colors and dimensions here prevents visual tuning from leaking into
//! the hierarchy, inspector, and asset-browser implementations.

use bevy::prelude::*;

pub const MENU_HEIGHT: f32 = 38.0;
pub const MAIN_TOOLBAR_HEIGHT: f32 = 0.0;
pub const TAB_HEIGHT: f32 = 30.0;
pub const VIEWPORT_TOOLBAR_HEIGHT: f32 = 32.0;
pub const SCENE_PANEL_WIDTH: f32 = 300.0;
pub const DETAILS_PANEL_WIDTH: f32 = 430.0;
pub const FILESYSTEM_PANEL_HEIGHT: f32 = 560.0;
pub const OUTPUT_PANEL_HEIGHT: f32 = 280.0;

pub fn bg_app() -> Color {
    Color::srgb(0.020, 0.023, 0.026)
}

pub fn bg_menu() -> Color {
    Color::srgb(0.055, 0.056, 0.058)
}

pub fn bg_toolbar() -> Color {
    Color::srgb(0.135, 0.135, 0.138)
}

pub fn bg_panel() -> Color {
    Color::srgb(0.165, 0.166, 0.168)
}

pub fn bg_panel_alt() -> Color {
    Color::srgb(0.125, 0.126, 0.128)
}

pub fn bg_field() -> Color {
    Color::srgb(0.210, 0.211, 0.213)
}

pub fn bg_hover() -> Color {
    Color::srgb(0.250, 0.251, 0.253)
}

pub fn bg_selected() -> Color {
    Color::srgb(0.190, 0.280, 0.410)
}

pub fn bg_selected_pressed() -> Color {
    Color::srgb(0.145, 0.220, 0.325)
}

pub fn border() -> Color {
    Color::srgb(0.030, 0.032, 0.035)
}

pub fn border_soft() -> Color {
    Color::srgb(0.245, 0.247, 0.250)
}

pub fn accent() -> Color {
    Color::srgb(0.135, 0.520, 0.900)
}

pub fn accent_hover() -> Color {
    Color::srgb(0.12, 0.69, 0.90)
}

pub fn play() -> Color {
    Color::srgb(0.25, 0.76, 0.47)
}

pub fn warning() -> Color {
    Color::srgb(0.94, 0.66, 0.22)
}

pub fn text_primary() -> Color {
    Color::srgb(0.88, 0.89, 0.91)
}

pub fn text_muted() -> Color {
    Color::srgb(0.640, 0.650, 0.665)
}

pub fn text_disabled() -> Color {
    Color::srgb(0.34, 0.36, 0.39)
}

pub fn folder_icon() -> Color {
    Color::srgb(0.58, 0.60, 0.63)
}

pub fn viewport_frame() -> Color {
    Color::srgb(0.070, 0.073, 0.078)
}
