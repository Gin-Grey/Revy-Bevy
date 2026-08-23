//! Editor output log fed by both local systems and child game processes.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use bevy::{
    feathers::cursor::EntityCursor,
    prelude::*,
    ui::{ScrollPosition, UiSystems},
    ui_widgets::{ControlOrientation, ScrollArea, Scrollbar, ScrollbarThumb},
    window::SystemCursorIcon,
};

use crate::{
    panels::{BottomDockState, BottomDockTab},
    ui::theme,
};

const MAX_OUTPUT_ENTRIES: usize = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputLevel {
    Info,
    Warning,
    Error,
}

impl OutputLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Info => theme::accent(),
            Self::Warning => theme::warning(),
            Self::Error => Color::srgb(0.93, 0.34, 0.34),
        }
    }
}

#[derive(Clone, Debug)]
struct OutputEntry {
    level: OutputLevel,
    message: String,
}

#[derive(Clone, Default)]
pub struct OutputSender(Arc<Mutex<VecDeque<OutputEntry>>>);

impl OutputSender {
    pub fn push(&self, level: OutputLevel, message: impl Into<String>) {
        let Ok(mut pending) = self.0.lock() else {
            return;
        };
        pending.push_back(OutputEntry {
            level,
            message: message.into(),
        });
    }
}

#[derive(Resource)]
pub struct OutputLog {
    entries: VecDeque<OutputEntry>,
    sender: OutputSender,
    open: bool,
}

impl Default for OutputLog {
    fn default() -> Self {
        let mut entries = VecDeque::new();
        entries.push_back(OutputEntry {
            level: OutputLevel::Info,
            message: "Editor initialized".into(),
        });
        entries.push_back(OutputEntry {
            level: OutputLevel::Info,
            message: "Watching project assets for changes".into(),
        });
        Self {
            entries,
            sender: OutputSender::default(),
            open: false,
        }
    }
}

impl OutputLog {
    pub fn sender(&self) -> OutputSender {
        self.sender.clone()
    }

    pub fn push(&mut self, level: OutputLevel, message: impl Into<String>) {
        if level == OutputLevel::Error {
            self.open = true;
        }
        self.entries.push_back(OutputEntry {
            level,
            message: message.into(),
        });
        while self.entries.len() > MAX_OUTPUT_ENTRIES {
            self.entries.pop_front();
        }
    }
}

#[derive(Component, Clone, Copy, Default)]
pub struct OutputList;

#[derive(Component, Clone, Copy, Default)]
struct OutputScrollbar;

#[derive(Component, Clone, Copy, Default)]
pub struct OutputTotalLabel;

#[derive(Component, Clone, Copy, Default)]
pub struct OutputInfoLabel;

#[derive(Component, Clone, Copy, Default)]
pub struct OutputWarningLabel;

#[derive(Component, Clone, Copy, Default)]
pub struct OutputErrorLabel;

pub struct OutputPlugin;

impl Plugin for OutputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OutputLog>()
            .add_systems(
                Update,
                (
                    drain_background_output,
                    open_output_for_errors,
                    mount_output_scrollbar,
                    rebuild_output_chrome,
                )
                    .chain(),
            )
            .add_systems(PostUpdate, auto_scroll_output.after(UiSystems::Layout));
    }
}

fn mount_output_scrollbar(
    mut commands: Commands,
    lists: Query<(Entity, &ChildOf), Added<OutputList>>,
) {
    for (list, parent) in &lists {
        commands.entity(list).insert(ScrollArea);
        commands.entity(parent.parent()).with_children(|host| {
            host.spawn((
                OutputScrollbar,
                Scrollbar::new(list, ControlOrientation::Vertical, 28.0),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(2.0),
                    top: Val::Px(4.0),
                    bottom: Val::Px(4.0),
                    width: Val::Px(8.0),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.11, 0.115, 0.13)),
                ZIndex(1),
            ))
            .with_children(|track| {
                track.spawn((
                    ScrollbarThumb {
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        border: UiRect::ZERO,
                    },
                    BackgroundColor(Color::srgb(0.42, 0.44, 0.49)),
                    EntityCursor::System(SystemCursorIcon::Pointer),
                ));
            });
        });
    }
}

fn auto_scroll_output(
    output: Res<OutputLog>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<OutputList>>,
) {
    if !output.is_changed() {
        return;
    }
    for (computed, mut scroll) in &mut lists {
        let visible = (computed.size() - computed.scrollbar_size) * computed.inverse_scale_factor;
        let content = computed.content_size() * computed.inverse_scale_factor;
        scroll.y = (content.y - visible.y).max(0.0);
    }
}

fn drain_background_output(mut output: ResMut<OutputLog>) {
    let pending = {
        let Ok(mut queue) = output.sender.0.lock() else {
            return;
        };
        queue.drain(..).collect::<Vec<_>>()
    };
    if pending.is_empty() {
        return;
    }
    for entry in pending {
        output.push(entry.level, entry.message);
    }
}

fn open_output_for_errors(mut output: ResMut<OutputLog>, mut bottom_dock: ResMut<BottomDockState>) {
    if !output.open {
        return;
    }

    bottom_dock.show(BottomDockTab::Output);
    output.open = false;
}

fn rebuild_output_chrome(
    output: Res<OutputLog>,
    lists: Query<(Entity, Option<&Children>), With<OutputList>>,
    mut total_labels: Query<
        &mut Text,
        (
            With<OutputTotalLabel>,
            Without<OutputInfoLabel>,
            Without<OutputWarningLabel>,
            Without<OutputErrorLabel>,
        ),
    >,
    mut info_labels: Query<
        &mut Text,
        (
            With<OutputInfoLabel>,
            Without<OutputTotalLabel>,
            Without<OutputWarningLabel>,
            Without<OutputErrorLabel>,
        ),
    >,
    mut warning_labels: Query<
        &mut Text,
        (
            With<OutputWarningLabel>,
            Without<OutputTotalLabel>,
            Without<OutputInfoLabel>,
            Without<OutputErrorLabel>,
        ),
    >,
    mut error_labels: Query<
        &mut Text,
        (
            With<OutputErrorLabel>,
            Without<OutputTotalLabel>,
            Without<OutputInfoLabel>,
            Without<OutputWarningLabel>,
        ),
    >,
    mut commands: Commands,
) {
    if !output.is_changed() {
        return;
    }

    let info_count = output
        .entries
        .iter()
        .filter(|entry| entry.level == OutputLevel::Info)
        .count();
    let warning_count = output
        .entries
        .iter()
        .filter(|entry| entry.level == OutputLevel::Warning)
        .count();
    let error_count = output
        .entries
        .iter()
        .filter(|entry| entry.level == OutputLevel::Error)
        .count();
    for mut text in &mut total_labels {
        text.0 = format!("{} messages", output.entries.len());
    }
    for mut text in &mut info_labels {
        text.0 = format!("Info {info_count}");
    }
    for mut text in &mut warning_labels {
        text.0 = format!("Warnings {warning_count}");
    }
    for mut text in &mut error_labels {
        text.0 = format!("Errors {error_count}");
    }

    for (list, children) in &lists {
        if let Some(children) = children {
            for child in children.iter() {
                commands.entity(child).despawn();
            }
        }
        commands.entity(list).with_children(|parent| {
            for entry in &output.entries {
                parent
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(22.0),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn(Node {
                            width: Val::Px(44.0),
                            min_width: Val::Px(44.0),
                            ..default()
                        })
                        .with_child((
                            Text::new(entry.level.label()),
                            TextFont::from_font_size(10.0),
                            TextColor(entry.level.color()),
                        ));
                        row.spawn((
                            Text::new(entry.message.clone()),
                            TextFont::from_font_size(11.0),
                            TextColor(theme::text_primary()),
                        ));
                    });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_open_the_output_panel() {
        let mut output = OutputLog::default();
        assert!(!output.open);

        output.push(OutputLevel::Error, "build failed");

        assert!(output.open);
    }

    #[test]
    fn non_errors_do_not_force_the_output_panel_open() {
        let mut output = OutputLog::default();
        output.push(OutputLevel::Info, "building");
        output.push(OutputLevel::Warning, "warning");

        assert!(!output.open);
    }

    #[test]
    fn error_request_opens_and_selects_the_output_dock() {
        let mut app = App::new();
        app.init_resource::<OutputLog>()
            .init_resource::<BottomDockState>()
            .add_systems(Update, open_output_for_errors);

        {
            let mut dock = app.world_mut().resource_mut::<BottomDockState>();
            dock.active = BottomDockTab::Animation;
            dock.open = false;
        }
        app.world_mut()
            .resource_mut::<OutputLog>()
            .push(OutputLevel::Error, "build failed");

        app.update();

        let dock = app.world().resource::<BottomDockState>();
        assert_eq!(dock.active, BottomDockTab::Output);
        assert!(dock.open);
        assert!(!app.world().resource::<OutputLog>().open);
    }
}
