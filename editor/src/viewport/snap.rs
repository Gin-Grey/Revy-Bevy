use std::fs;

use arisna_engine::ProjectRoot;
use bevy::prelude::*;
use toml_edit::DocumentMut;

use crate::project_settings::update_toml_section;

const GRID_STEPS: [f32; 8] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0];

#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub(crate) struct Snap2dSettings {
    pub enabled: bool,
    pub grid_visible: bool,
    pub grid_size: f32,
    pub grid_offset: Vec2,
    pub smart_distance_px: f32,
    pub rotation_step_degrees: f32,
    pub scale_step: f32,
}

impl Default for Snap2dSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            grid_visible: true,
            grid_size: 32.0,
            grid_offset: Vec2::ZERO,
            smart_distance_px: 8.0,
            rotation_step_degrees: 15.0,
            scale_step: 0.1,
        }
    }
}

impl Snap2dSettings {
    pub(crate) fn load(project: Option<&ProjectRoot>) -> Self {
        let Some(path) = project.map(|project| project.root.join("project.toml")) else {
            return Self::default();
        };
        let Ok(source) = fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(document) = source.parse::<DocumentMut>() else {
            return Self::default();
        };
        Self::from_document(&document)
    }

    fn from_document(document: &DocumentMut) -> Self {
        let defaults = Self::default();
        let snap = document
            .get("editor")
            .and_then(|editor| editor.get("snap_2d"));
        let number = |key: &str, fallback: f32| {
            snap.and_then(|snap| snap.get(key))
                .and_then(|value| {
                    value
                        .as_float()
                        .or_else(|| value.as_integer().map(|v| v as f64))
                })
                .map(|value| value as f32)
                .filter(|value| value.is_finite())
                .unwrap_or(fallback)
        };
        Self {
            enabled: snap
                .and_then(|snap| snap.get("enabled"))
                .and_then(|value| value.as_bool())
                .unwrap_or(defaults.enabled),
            grid_visible: snap
                .and_then(|snap| snap.get("grid_visible"))
                .and_then(|value| value.as_bool())
                .unwrap_or(defaults.grid_visible),
            grid_size: number("grid_size", defaults.grid_size).clamp(1.0, 1024.0),
            grid_offset: Vec2::new(
                number("grid_offset_x", defaults.grid_offset.x),
                number("grid_offset_y", defaults.grid_offset.y),
            ),
            smart_distance_px: number("smart_distance_px", defaults.smart_distance_px)
                .clamp(1.0, 32.0),
            rotation_step_degrees: number("rotation_step_degrees", defaults.rotation_step_degrees)
                .clamp(0.1, 180.0),
            scale_step: number("scale_step", defaults.scale_step).clamp(0.01, 10.0),
        }
    }

    pub(crate) fn persist(&self, project: &ProjectRoot) -> Result<(), String> {
        let path = project.root.join("project.toml");
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("Could not read project.toml: {error}"))?;
        source
            .parse::<DocumentMut>()
            .map_err(|error| format!("Invalid project.toml: {error}"))?;
        let rendered = update_toml_section(
            &source,
            "editor.snap_2d",
            &[
                ("enabled", self.enabled.to_string()),
                ("grid_visible", self.grid_visible.to_string()),
                ("grid_size", self.grid_size.to_string()),
                ("grid_offset_x", self.grid_offset.x.to_string()),
                ("grid_offset_y", self.grid_offset.y.to_string()),
                ("smart_distance_px", self.smart_distance_px.to_string()),
                (
                    "rotation_step_degrees",
                    self.rotation_step_degrees.to_string(),
                ),
                ("scale_step", self.scale_step.to_string()),
            ],
            true,
        )?;
        fs::write(path, rendered).map_err(|error| format!("Could not save project.toml: {error}"))
    }

    pub(crate) fn effective(self, shift_pressed: bool) -> bool {
        self.enabled ^ shift_pressed
    }

    pub(crate) fn cycle_grid_size(&mut self) {
        let current = GRID_STEPS
            .iter()
            .position(|step| (*step - self.grid_size).abs() < f32::EPSILON)
            .unwrap_or(5);
        self.grid_size = GRID_STEPS[(current + 1) % GRID_STEPS.len()];
    }
}

pub(crate) fn snap_scalar(value: f32, step: f32, offset: f32) -> f32 {
    if !value.is_finite() || !step.is_finite() || step <= 0.0 {
        return value;
    }
    ((value - offset) / step).round() * step + offset
}

pub(crate) fn snap_vec2(value: Vec2, step: f32, offset: Vec2) -> Vec2 {
    Vec2::new(
        snap_scalar(value.x, step, offset.x),
        snap_scalar(value.y, step, offset.y),
    )
}

pub(crate) fn snap_angle_radians(value: f32, step_degrees: f32) -> f32 {
    snap_scalar(value, step_degrees.to_radians(), 0.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SmartSnapAxisMatch {
    pub delta: f32,
    pub guide: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct SmartSnap2dMatch {
    pub x: Option<SmartSnapAxisMatch>,
    pub y: Option<SmartSnapAxisMatch>,
}

pub(crate) fn smart_snap_2d(
    moving_x: &[f32],
    moving_y: &[f32],
    target_x: &[f32],
    target_y: &[f32],
    tolerance: f32,
) -> SmartSnap2dMatch {
    SmartSnap2dMatch {
        x: nearest_axis_match(moving_x, target_x, tolerance),
        y: nearest_axis_match(moving_y, target_y, tolerance),
    }
}

fn nearest_axis_match(
    moving: &[f32],
    targets: &[f32],
    tolerance: f32,
) -> Option<SmartSnapAxisMatch> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return None;
    }
    let mut best: Option<(f32, SmartSnapAxisMatch)> = None;
    for &moving_value in moving.iter().filter(|value| value.is_finite()) {
        for &target in targets.iter().filter(|value| value.is_finite()) {
            let delta = target - moving_value;
            let distance = delta.abs();
            if distance <= tolerance
                && best.is_none_or(|(best_distance, _)| distance < best_distance)
            {
                best = Some((
                    distance,
                    SmartSnapAxisMatch {
                        delta,
                        guide: target,
                    },
                ));
            }
        }
    }
    best.map(|(_, snap)| snap)
}

pub(crate) fn visible_grid_step(grid_size: f32, camera_scale: f32) -> f32 {
    let level = camera_scale.max(0.001).log2().round().clamp(-4.0, 8.0);
    grid_size.max(1.0) * 2.0_f32.powf(level)
}

pub(crate) fn shift_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_temporarily_inverts_toolbar_snapping() {
        let mut settings = Snap2dSettings::default();
        assert!(!settings.effective(false));
        assert!(settings.effective(true));
        settings.enabled = true;
        assert!(settings.effective(false));
        assert!(!settings.effective(true));
    }

    #[test]
    fn scalar_vector_and_rotation_snap_to_absolute_steps() {
        assert_eq!(snap_scalar(47.0, 32.0, 0.0), 32.0);
        assert_eq!(snap_scalar(47.0, 32.0, 8.0), 40.0);
        assert_eq!(
            snap_vec2(Vec2::new(47.0, -18.0), 32.0, Vec2::ZERO),
            Vec2::new(32.0, -32.0)
        );
        assert!(
            (snap_angle_radians(22.0_f32.to_radians(), 15.0).to_degrees() - 15.0).abs() < 0.001
        );
        assert_eq!(visible_grid_step(16.0, 2.0), 32.0);
    }

    #[test]
    fn project_document_restores_snap_settings() {
        let document = r#"
[editor.snap_2d]
enabled = true
grid_visible = false
grid_size = 64
grid_offset_x = 4.5
grid_offset_y = -8
smart_distance_px = 6
rotation_step_degrees = 30
scale_step = 0.25
"#
        .parse::<DocumentMut>()
        .unwrap();

        let settings = Snap2dSettings::from_document(&document);

        assert!(settings.enabled);
        assert!(!settings.grid_visible);
        assert_eq!(settings.grid_size, 64.0);
        assert_eq!(settings.grid_offset, Vec2::new(4.5, -8.0));
        assert_eq!(settings.smart_distance_px, 6.0);
        assert_eq!(settings.rotation_step_degrees, 30.0);
        assert_eq!(settings.scale_step, 0.25);
    }

    #[test]
    fn smart_snap_uses_edges_centers_and_the_closest_target_per_axis() {
        let result = smart_snap_2d(
            &[92.0, 142.0, 192.0],
            &[49.0],
            &[0.0, 96.0, 200.0],
            &[0.0, 50.0],
            8.0,
        );

        assert_eq!(
            result.x,
            Some(SmartSnapAxisMatch {
                delta: 4.0,
                guide: 96.0,
            })
        );
        assert_eq!(
            result.y,
            Some(SmartSnapAxisMatch {
                delta: 1.0,
                guide: 50.0,
            })
        );
    }

    #[test]
    fn smart_snap_respects_world_tolerance() {
        let result = smart_snap_2d(&[91.9], &[], &[100.0], &[], 8.0);
        assert_eq!(result, SmartSnap2dMatch::default());
    }
}
