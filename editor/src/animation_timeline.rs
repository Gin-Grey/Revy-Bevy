//! AnimationPlayer timeline authoring panel.
//!
//! The editor only stores stable scene node IDs in animation tracks. Runtime
//! Bevy entities are resolved later, so scene reloads and nested `.bsn` files
//! do not invalidate authored animation data.

use std::collections::HashMap;

use arisna_engine::{
    SceneAnimationClip, SceneAnimationKey, SceneAnimationPlayer, SceneAnimationTrack,
    SceneAnimationTrackKind, SceneSprite2D, SceneUiLayout, animation_transform_from_ui_layout,
    apply_sample_to_transform_property, apply_sample_to_ui_layout_property,
    format_animation_transform, format_sprite_frame, sample_animation_transform,
    sample_sprite_frame,
};
use bevy::{
    input_focus::AutoFocus,
    prelude::*,
    text::{EditableText, EditableTextFilter, TextCursorStyle},
    ui::RelativeCursorPosition,
    ui_widgets::{Activate, Button as WidgetButton, SelectAllOnFocus},
};

use crate::{
    hierarchy::SceneNodeId,
    scene::SceneDocument,
    selection::{EditableObject, Selection},
    ui::{
        components::{
            AnimationTimelineBody, AnimationTimelineClipButton, AnimationTimelineClipLabel,
            AnimationTimelineClipList, AnimationTimelinePanel, AnimationTimelinePlayButton,
            AnimationTimelineTimeLabel, EditorVerticalScrollArea,
        },
        theme,
    },
    undo::{SceneHistory, SceneSnapshot, SceneSnapshotQuery, capture_scene_snapshot},
    workspace::WorkspaceMode,
};

#[derive(Resource, Debug, Default)]
pub struct AnimationTimelineState {
    active_player: Option<Entity>,
    selected_clip: usize,
    current_time: f32,
    playing: bool,
    previewing: bool,
    selected_track: Option<usize>,
    selected_key: Option<(usize, usize)>,
    key_drag: Option<AnimationKeyDragState>,
    track_menu_open: bool,
    animation_menu_open: bool,
    clip_menu_open: bool,
    new_dialog_open: bool,
    new_name_error: String,
    status: String,
    revision: u64,
}

#[derive(Debug)]
struct AnimationKeyDragState {
    track_index: usize,
    key_index: usize,
    start_time: f32,
    current_time: f32,
    clip_length: f32,
    lane_width: f32,
    before: SceneSnapshot,
}

/// Inspector 中 Transform 三组钥匙按钮对应的稳定属性路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnimationTransformProperty {
    Position,
    Rotation,
    Scale,
}

impl AnimationTransformProperty {
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::Position => "transform.position",
            Self::Rotation => "transform.rotation",
            Self::Scale => "transform.scale",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Position => "Position",
            Self::Rotation => "Rotation",
            Self::Scale => "Scale",
        }
    }
}

/// Inspector 发出的全局请求，由当前 AnimationPlayer 和时间游标完成实际插帧。
#[derive(Event, Clone, Copy, Debug)]
pub(crate) struct InsertAnimationPropertyKey {
    pub(crate) property: AnimationTransformProperty,
}

/// Sprite2D Inspector 发出的当前帧关键帧请求。
#[derive(Event, Clone, Copy, Debug, Default)]
pub(crate) struct InsertSpriteFrameKey;

/// 时间轴预览前的场景 Transform。保存和撤销必须读取它，而不是临时预览姿态。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct AnimationPreviewOriginalTransform(pub Transform);

/// 时间轴预览前的 UI 布局，作用与 AnimationPreviewOriginalTransform 相同。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct AnimationPreviewOriginalUiLayout(pub SceneUiLayout);

/// 时间轴预览前的 Sprite2D 静态帧，停止预览和保存场景时必须恢复它。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct AnimationPreviewOriginalSpriteFrame(pub u32);

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineStopButton;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineMenuButton;

#[derive(Component, Clone, Copy, Debug, Default)]
struct AnimationTimelineMenuActionButton(AnimationTimelineMenuAction);

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AnimationTimelineMenuAction {
    #[default]
    New,
    Manage,
    Duplicate,
    Rename,
    EditTransitions,
    OpenInInspector,
    Remove,
}

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineAnimationMenu;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineClipSelectorButton;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineClipSelectorMenu;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineNewDialog;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineNewInput;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineNewConfirmButton;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineNewCancelButton;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineNewErrorLabel;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelinePlayIcon;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelinePlayhead;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineAddTrackButton;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineAddKeyButton;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineRemoveTrackButton;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineRemoveKeyButton;

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineTrackMenu;

#[derive(Component, Clone, Copy)]
struct AnimationTimelineTrackKindButton(SceneAnimationTrackKind);

#[derive(Component, Clone, Copy)]
struct AnimationTimelineTrackButton(usize);

#[derive(Component, Clone, Copy)]
struct AnimationTimelineKeyButton {
    track_index: usize,
    key_index: usize,
}

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineStatusLabel;

#[derive(Component, Clone, Copy)]
struct AnimationTimelineRulerTick(usize);

#[derive(Component, Clone, Copy, Default)]
struct AnimationTimelineScrubArea;

pub struct AnimationTimelinePlugin;

impl Plugin for AnimationTimelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnimationTimelineState>()
            .add_observer(select_animation_clip)
            .add_observer(toggle_animation_menu)
            .add_observer(handle_animation_menu_action)
            .add_observer(toggle_animation_clip_selector)
            .add_observer(confirm_new_animation)
            .add_observer(cancel_new_animation)
            .add_observer(toggle_timeline_playback)
            .add_observer(stop_timeline_playback)
            .add_observer(toggle_track_menu)
            .add_observer(add_animation_track)
            .add_observer(select_animation_track)
            .add_observer(add_animation_key)
            .add_observer(insert_inspector_animation_key)
            .add_observer(insert_inspector_sprite_frame_key)
            .add_observer(select_animation_key)
            .add_observer(begin_animation_key_drag)
            .add_observer(drag_animation_key)
            .add_observer(finish_animation_key_drag)
            .add_observer(remove_animation_key)
            .add_observer(remove_animation_track)
            .add_observer(scrub_animation_timeline_press)
            .add_observer(scrub_animation_timeline_drag)
            .add_systems(
                Update,
                (
                    track_selected_animation_player,
                    rebuild_animation_timeline,
                    advance_animation_playhead,
                    preview_animation_pose,
                    sync_animation_timeline_chrome,
                )
                    .chain(),
            );
    }
}

/// Builds the stable timeline chrome inside the shell-owned host.
pub fn spawn_animation_timeline(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            AnimationTimelinePanel,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                position_type: PositionType::Relative,
                flex_direction: FlexDirection::Column,
                border: UiRect::top(Val::Px(1.0)),
                overflow: Overflow::visible(),
                ..default()
            },
            BackgroundColor(theme::bg_panel_alt()),
            BorderColor::all(theme::border()),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(34.0),
                        min_height: Val::Px(34.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(8.0)),
                        column_gap: Val::Px(6.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(theme::bg_toolbar()),
                    BorderColor::all(theme::border()),
                ))
                .with_children(|toolbar| {
                    icon_button(
                        toolbar,
                        AnimationTimelineStopButton,
                        "editor/icons/square.png",
                        asset_server,
                    );
                    toolbar
                        .spawn((
                            Button,
                            WidgetButton,
                            AnimationTimelinePlayButton,
                            Node {
                                width: Val::Px(28.0),
                                height: Val::Px(26.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(theme::bg_field()),
                            BorderColor::all(theme::border_soft()),
                        ))
                        .with_child((
                            AnimationTimelinePlayIcon,
                            ImageNode::new(asset_server.load("editor/icons/play.png"))
                                .with_color(theme::text_primary()),
                            Node {
                                width: Val::Px(14.0),
                                height: Val::Px(14.0),
                                ..default()
                            },
                        ));
                    toolbar.spawn((
                        AnimationTimelineTimeLabel,
                        Text::new("0.00 / 0.00 s"),
                        TextFont {
                            font_size: FontSize::Px(10.5),
                            ..default()
                        },
                        TextLayout::no_wrap(),
                        TextColor(theme::text_primary()),
                        Node {
                            width: Val::Px(106.0),
                            min_width: Val::Px(106.0),
                            height: Val::Px(18.0),
                            min_height: Val::Px(18.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    animation_menu_button(toolbar, asset_server);
                    toolbar
                        .spawn((
                            Button,
                            WidgetButton,
                            AnimationTimelineClipSelectorButton,
                            Node {
                                width: Val::Px(0.0),
                                min_width: Val::Px(150.0),
                                max_width: Val::Px(360.0),
                                flex_grow: 1.0,
                                flex_shrink: 1.0,
                                height: Val::Px(26.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(Val::Px(8.0)),
                                column_gap: Val::Px(6.0),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(theme::bg_field()),
                            BorderColor::all(theme::border_soft()),
                        ))
                        .with_children(|selector| {
                            selector.spawn((
                                AnimationTimelineClipLabel,
                                Text::new("No animation"),
                                TextFont {
                                    font_size: FontSize::Px(10.5),
                                    ..default()
                                },
                                TextLayout::no_wrap(),
                                TextColor(theme::text_primary()),
                                Node {
                                    flex_grow: 1.0,
                                    min_width: Val::Px(0.0),
                                    ..default()
                                },
                            ));
                            selector.spawn((
                                ImageNode::new(asset_server.load("editor/icons/chevron-down.png"))
                                    .with_color(theme::text_muted()),
                                Node {
                                    width: Val::Px(12.0),
                                    height: Val::Px(12.0),
                                    ..default()
                                },
                            ));
                        });
                    text_button(toolbar, AnimationTimelineAddTrackButton, "+ Add Track");
                    text_button(toolbar, AnimationTimelineAddKeyButton, "+ Key");
                    text_button(toolbar, AnimationTimelineRemoveKeyButton, "Remove Key");
                    text_button(toolbar, AnimationTimelineRemoveTrackButton, "Remove Track");
                    toolbar.spawn((
                        AnimationTimelineStatusLabel,
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(theme::warning()),
                    ));
                });

            panel
                .spawn((
                    AnimationTimelineAnimationMenu,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(174.0),
                        top: Val::Px(34.0),
                        width: Val::Px(250.0),
                        min_width: Val::Px(250.0),
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(5.0)),
                        row_gap: Val::Px(2.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    GlobalZIndex(1200),
                    BackgroundColor(theme::bg_menu()),
                    BorderColor::all(theme::border_soft()),
                ))
                .with_children(|menu| {
                    animation_menu_item(
                        menu,
                        asset_server,
                        AnimationTimelineMenuAction::New,
                        "New...",
                        "editor/icons/plus.png",
                        true,
                    );
                    animation_menu_separator(menu);
                    animation_menu_item(
                        menu,
                        asset_server,
                        AnimationTimelineMenuAction::Manage,
                        "Manage Animations...",
                        "editor/icons/settings.png",
                        false,
                    );
                    animation_menu_separator(menu);
                    animation_menu_item(
                        menu,
                        asset_server,
                        AnimationTimelineMenuAction::Duplicate,
                        "Duplicate...",
                        "editor/icons/redo-2.png",
                        false,
                    );
                    animation_menu_item(
                        menu,
                        asset_server,
                        AnimationTimelineMenuAction::Rename,
                        "Rename...",
                        "editor/icons/type.png",
                        false,
                    );
                    animation_menu_item(
                        menu,
                        asset_server,
                        AnimationTimelineMenuAction::EditTransitions,
                        "Edit Transitions...",
                        "editor/icons/link.png",
                        false,
                    );
                    animation_menu_item(
                        menu,
                        asset_server,
                        AnimationTimelineMenuAction::OpenInInspector,
                        "Open in Inspector",
                        "editor/icons/settings.png",
                        false,
                    );
                    animation_menu_separator(menu);
                    animation_menu_item(
                        menu,
                        asset_server,
                        AnimationTimelineMenuAction::Remove,
                        "Remove",
                        "editor/icons/x.png",
                        false,
                    );
                });

            panel.spawn((
                AnimationTimelineClipSelectorMenu,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(300.0),
                    top: Val::Px(34.0),
                    width: Val::Px(260.0),
                    min_width: Val::Px(220.0),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(5.0)),
                    row_gap: Val::Px(2.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                GlobalZIndex(1200),
                BackgroundColor(theme::bg_menu()),
                BorderColor::all(theme::border_soft()),
            ));

            panel
                .spawn((
                    AnimationTimelineNewDialog,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(18.0),
                        right: Val::Px(18.0),
                        top: Val::Px(42.0),
                        bottom: Val::Px(18.0),
                        display: Display::None,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        padding: UiRect::all(Val::Px(12.0)),
                        ..default()
                    },
                    GlobalZIndex(1300),
                    BackgroundColor(Color::srgba(0.02, 0.025, 0.035, 0.78)),
                ))
                .with_children(|root| {
                    root.spawn((
                        Node {
                            width: Val::Px(360.0),
                            max_width: Val::Percent(100.0),
                            padding: UiRect::all(Val::Px(14.0)),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(8.0),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(theme::bg_menu()),
                        BorderColor::all(theme::border_soft()),
                    ))
                    .with_children(|dialog| {
                        dialog.spawn((
                            Text::new("New Animation"),
                            TextFont {
                                font_size: FontSize::Px(13.0),
                                ..default()
                            },
                            TextColor(theme::text_primary()),
                        ));
                        dialog.spawn((
                            Text::new("Animation name"),
                            TextFont {
                                font_size: FontSize::Px(10.5),
                                ..default()
                            },
                            TextColor(theme::text_muted()),
                        ));
                        dialog.spawn((
                            AnimationTimelineNewInput,
                            SelectAllOnFocus,
                            EditableText::new("Animation"),
                            EditableTextFilter::new(|character| {
                                character != '\n' && character != '\r'
                            }),
                            TextCursorStyle::default(),
                            TextFont {
                                font_size: FontSize::Px(11.5),
                                ..default()
                            },
                            TextColor(theme::text_primary()),
                            Node {
                                width: Val::Percent(100.0),
                                min_width: Val::Px(0.0),
                                height: Val::Px(32.0),
                                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(theme::bg_field()),
                            BorderColor::all(theme::border_soft()),
                        ));
                        dialog.spawn((
                            AnimationTimelineNewErrorLabel,
                            Text::new(""),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::warning()),
                        ));
                        dialog
                            .spawn(Node {
                                width: Val::Percent(100.0),
                                justify_content: JustifyContent::FlexEnd,
                                column_gap: Val::Px(6.0),
                                ..default()
                            })
                            .with_children(|buttons| {
                                text_button(buttons, AnimationTimelineNewCancelButton, "Cancel");
                                text_button(buttons, AnimationTimelineNewConfirmButton, "Create");
                            });
                    });
                });

            panel
                .spawn((
                    AnimationTimelineTrackMenu,
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(34.0),
                        display: Display::None,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(8.0)),
                        column_gap: Val::Px(5.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_panel()),
                    BorderColor::all(theme::border()),
                ))
                .with_children(|menu| {
                    menu.spawn((
                        Text::new("Track type"),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                    ));
                    for kind in [
                        SceneAnimationTrackKind::Transform,
                        SceneAnimationTrackKind::SpriteFrame,
                        SceneAnimationTrackKind::Property,
                        SceneAnimationTrackKind::Event,
                        SceneAnimationTrackKind::Audio,
                        SceneAnimationTrackKind::Bone,
                        SceneAnimationTrackKind::Animation,
                    ] {
                        text_button(menu, AnimationTimelineTrackKindButton(kind), kind.label());
                    }
                });

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(0.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Row,
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|content| {
                    content
                        .spawn((
                            Node {
                                width: Val::Px(190.0),
                                min_width: Val::Px(150.0),
                                height: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                border: UiRect::right(Val::Px(1.0)),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            BackgroundColor(theme::bg_panel()),
                            BorderColor::all(theme::border()),
                        ))
                        .with_children(|clips| {
                            clips.spawn((
                                Text::new("Animations"),
                                TextFont {
                                    font_size: FontSize::Px(10.5),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(28.0),
                                    min_height: Val::Px(28.0),
                                    padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                                    ..default()
                                },
                            ));
                            clips.spawn((
                                AnimationTimelineClipList,
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(0.0),
                                    flex_grow: 1.0,
                                    min_height: Val::Px(0.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(2.0),
                                    padding: UiRect::horizontal(Val::Px(4.0)),
                                    overflow: Overflow::scroll_y(),
                                    ..default()
                                },
                            ));
                        });

                    content
                        .spawn(Node {
                            width: Val::Px(0.0),
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            height: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|timeline| {
                            spawn_time_ruler(timeline);
                            timeline.spawn((
                                AnimationTimelineBody,
                                EditorVerticalScrollArea,
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(0.0),
                                    flex_grow: 1.0,
                                    min_height: Val::Px(0.0),
                                    position_type: PositionType::Relative,
                                    flex_direction: FlexDirection::Column,
                                    overflow: Overflow::scroll_y(),
                                    ..default()
                                },
                                BackgroundColor(theme::viewport_frame()),
                            ));
                        });
                });
        });
}

fn animation_menu_button(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            Button,
            WidgetButton,
            AnimationTimelineMenuButton,
            Node {
                height: Val::Px(26.0),
                min_width: Val::Px(104.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::bg_field()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new("Animation"),
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(theme::text_primary()),
            ));
            button.spawn((
                ImageNode::new(asset_server.load("editor/icons/chevron-down.png"))
                    .with_color(theme::text_muted()),
                Node {
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    ..default()
                },
            ));
        });
}

fn animation_menu_item(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    action: AnimationTimelineMenuAction,
    label: &'static str,
    icon_path: &'static str,
    enabled: bool,
) {
    let color = if enabled {
        theme::text_primary()
    } else {
        theme::text_disabled()
    };
    let mut item = parent.spawn((
        Button,
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(28.0),
            min_height: Val::Px(28.0),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(8.0)),
            column_gap: Val::Px(8.0),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ));
    if enabled {
        item.insert((WidgetButton, AnimationTimelineMenuActionButton(action)));
    }
    item.with_children(|row| {
        row.spawn((
            ImageNode::new(asset_server.load(icon_path)).with_color(color),
            Node {
                width: Val::Px(15.0),
                height: Val::Px(15.0),
                ..default()
            },
        ));
        row.spawn((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(10.5),
                ..default()
            },
            TextColor(color),
        ));
    });
}

fn animation_menu_separator(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(1.0),
            min_height: Val::Px(1.0),
            margin: UiRect::vertical(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(theme::border_soft()),
    ));
}

fn icon_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    path: &'static str,
    asset_server: &AssetServer,
) {
    parent
        .spawn((
            Button,
            WidgetButton,
            marker,
            Node {
                width: Val::Px(28.0),
                height: Val::Px(26.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::bg_field()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_child((
            ImageNode::new(asset_server.load(path)).with_color(theme::text_primary()),
            Node {
                width: Val::Px(13.0),
                height: Val::Px(13.0),
                ..default()
            },
        ));
}

fn text_button<M: Component>(parent: &mut ChildSpawnerCommands, marker: M, label: &str) {
    parent.spawn((
        Button,
        WidgetButton,
        marker,
        Node {
            height: Val::Px(24.0),
            min_width: Val::Px(52.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(Val::Px(7.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(theme::bg_field()),
        BorderColor::all(theme::border_soft()),
        children![
            ((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_primary()),
            ))
        ],
    ));
}

fn spawn_time_ruler(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(28.0),
                min_height: Val::Px(28.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::bg_toolbar()),
            BorderColor::all(theme::border()),
        ))
        .with_children(|ruler| {
            ruler.spawn((
                Text::new("Track / Target"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
                Node {
                    width: Val::Px(230.0),
                    min_width: Val::Px(180.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    border: UiRect::right(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(theme::border()),
            ));
            ruler
                .spawn((
                    AnimationTimelineScrubArea,
                    Button,
                    RelativeCursorPosition::default(),
                    Node {
                        width: Val::Px(0.0),
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },
                ))
                .with_children(|ticks| {
                    for tick in 0..=10 {
                        ticks.spawn((
                            AnimationTimelineRulerTick(tick),
                            Text::new(format!("{:.1}", tick as f32 / 10.0)),
                            TextFont {
                                font_size: FontSize::Px(8.5),
                                ..default()
                            },
                            TextColor(theme::text_muted()),
                            Node {
                                width: Val::Percent(100.0 / 11.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                        ));
                    }
                });
        });
}

fn track_selected_animation_player(
    selection: Res<Selection>,
    players: Query<&SceneAnimationPlayer>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if let Some(entity) = selection.0
        && players.get(entity).is_ok()
        && state.active_player != Some(entity)
    {
        state.active_player = Some(entity);
        state.selected_clip = 0;
        state.current_time = 0.0;
        state.playing = false;
        state.previewing = false;
        state.selected_track = None;
        state.selected_key = None;
        state.key_drag = None;
        state.track_menu_open = false;
        state.animation_menu_open = false;
        state.clip_menu_open = false;
        state.new_dialog_open = false;
        state.new_name_error.clear();
        state.revision = state.revision.wrapping_add(1);
    }

    if state
        .active_player
        .is_some_and(|entity| players.get(entity).is_err())
    {
        *state = AnimationTimelineState::default();
        return;
    }

    if let Some(entity) = state.active_player
        && let Ok(player) = players.get(entity)
        && state.selected_clip >= player.clips.len()
    {
        state.selected_clip = player.clips.len().saturating_sub(1);
        state.current_time = 0.0;
        state.playing = false;
        state.previewing = false;
        state.selected_track = None;
        state.selected_key = None;
        state.key_drag = None;
        state.revision = state.revision.wrapping_add(1);
    }
}

fn rebuild_animation_timeline(
    players: Query<&SceneAnimationPlayer>,
    nodes: Query<(&SceneNodeId, &EditableObject)>,
    state: Res<AnimationTimelineState>,
    clip_lists: Query<Entity, With<AnimationTimelineClipList>>,
    selector_menus: Query<Entity, With<AnimationTimelineClipSelectorMenu>>,
    bodies: Query<Entity, With<AnimationTimelineBody>>,
    mut commands: Commands,
    mut last: Local<Option<(Option<Entity>, usize, u64, Option<SceneAnimationPlayer>)>>,
) {
    let player = state
        .active_player
        .and_then(|entity| players.get(entity).ok().cloned());
    let snapshot = (
        state.active_player,
        state.selected_clip,
        state.revision,
        player.clone(),
    );
    if last.as_ref() == Some(&snapshot) {
        return;
    }
    *last = Some(snapshot);

    let names: HashMap<String, String> = nodes
        .iter()
        .map(|(id, object)| (id.to_string(), object.name.clone()))
        .collect();

    for menu in &selector_menus {
        commands.entity(menu).despawn_related::<Children>();
        let Some(player) = player.as_ref() else {
            commands
                .entity(menu)
                .with_child(empty_hint("Select an AnimationPlayer entity"));
            continue;
        };
        if player.clips.is_empty() {
            commands
                .entity(menu)
                .with_child(empty_hint("No animations"));
            continue;
        }
        for (index, clip) in player.clips.iter().enumerate() {
            commands.entity(menu).with_child((
                Button,
                WidgetButton,
                AnimationTimelineClipButton(index),
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(27.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(7.0)),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(if index == state.selected_clip {
                    theme::bg_selected()
                } else {
                    theme::bg_field()
                }),
                BorderColor::all(if index == state.selected_clip {
                    theme::accent()
                } else {
                    Color::NONE
                }),
                children![
                    ((
                        Text::new(clip.name.clone()),
                        TextFont {
                            font_size: FontSize::Px(10.5),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ))
                ],
            ));
        }
    }

    for list in &clip_lists {
        commands.entity(list).despawn_related::<Children>();
        let Some(player) = player.as_ref() else {
            commands
                .entity(list)
                .with_child(empty_hint("Select an AnimationPlayer entity"));
            continue;
        };
        if player.clips.is_empty() {
            commands
                .entity(list)
                .with_child(empty_hint("No animations\nUse Animation > New..."));
            continue;
        }
        for (index, clip) in player.clips.iter().enumerate() {
            commands.entity(list).with_child((
                Button,
                WidgetButton,
                AnimationTimelineClipButton(index),
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(28.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(7.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(if index == state.selected_clip {
                    theme::bg_selected()
                } else {
                    theme::bg_field()
                }),
                BorderColor::all(if index == state.selected_clip {
                    theme::accent()
                } else {
                    theme::border_soft()
                }),
                children![
                    ((
                        Text::new(format!(
                            "{}    {:.2}s{}",
                            clip.name,
                            clip.length.max(0.0),
                            if clip.looped { "  Loop" } else { "" }
                        )),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ))
                ],
            ));
        }
    }

    for body in &bodies {
        commands.entity(body).despawn_related::<Children>();
        let Some(player) = player.as_ref() else {
            commands
                .entity(body)
                .with_child(empty_hint("Select an AnimationPlayer entity"));
            continue;
        };
        let Some(clip) = player.clips.get(state.selected_clip) else {
            commands
                .entity(body)
                .with_child(empty_hint("Create or select an animation clip"));
            continue;
        };

        if clip.tracks.is_empty() {
            commands.entity(body).with_child(empty_hint(
                "No tracks yet. Track creation is the next authoring step.",
            ));
        } else {
            for (track_index, track) in clip.tracks.iter().enumerate() {
                let target_name = names
                    .get(&track.target_node)
                    .cloned()
                    .unwrap_or_else(|| short_id(&track.target_node));
                commands.entity(body).with_children(|rows| {
                    spawn_track_row(
                        rows,
                        track.kind.label(),
                        &track.property,
                        &target_name,
                        clip.length,
                        &track.keys,
                        track_index,
                        state.selected_track == Some(track_index),
                        state.selected_key.and_then(|(selected_track, key)| {
                            (selected_track == track_index).then_some(key)
                        }),
                    );
                });
            }
        }

        commands.entity(body).with_child((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(230.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },
            Pickable::IGNORE,
            children![(
                AnimationTimelinePlayhead,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(theme::accent()),
                ZIndex(2),
            )],
        ));
    }
}

fn empty_hint(message: &str) -> impl Bundle {
    (
        Text::new(message.to_owned()),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(theme::text_muted()),
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(10.0)),
            ..default()
        },
    )
}

fn spawn_track_row(
    parent: &mut ChildSpawnerCommands,
    kind: &str,
    property: &str,
    target: &str,
    length: f32,
    keys: &[arisna_engine::SceneAnimationKey],
    track_index: usize,
    selected: bool,
    selected_key: Option<usize>,
) {
    let length = length.max(0.001);
    parent
        .spawn((
            Button,
            WidgetButton,
            AnimationTimelineTrackButton(track_index),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(34.0),
                flex_direction: FlexDirection::Row,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::bg_selected_pressed()
            } else {
                theme::bg_panel_alt()
            }),
            BorderColor::all(if selected {
                theme::accent()
            } else {
                theme::border()
            }),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(230.0),
                    min_width: Val::Px(180.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    border: UiRect::right(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(theme::border()),
                children![
                    (
                        Text::new(format!("{kind}  {property}")),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ),
                    (
                        Text::new(target.to_owned()),
                        TextFont {
                            font_size: FontSize::Px(8.5),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                    )
                ],
            ));
            row.spawn((Node {
                width: Val::Px(0.0),
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Relative,
                ..default()
            },))
                .with_children(|lane| {
                    for (key_index, key) in keys.iter().enumerate() {
                        let percent = (key.time / length * 100.0).clamp(0.0, 100.0);
                        lane.spawn((
                            Button,
                            WidgetButton,
                            AnimationTimelineKeyButton {
                                track_index,
                                key_index,
                            },
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(percent),
                                top: Val::Px(12.0),
                                width: Val::Px(9.0),
                                height: Val::Px(9.0),
                                border: UiRect::all(Val::Px(1.0)),
                                ..default()
                            },
                            Transform::from_rotation(Quat::from_rotation_z(
                                std::f32::consts::FRAC_PI_4,
                            )),
                            BackgroundColor(if selected_key == Some(key_index) {
                                theme::accent()
                            } else {
                                theme::warning()
                            }),
                            BorderColor::all(theme::text_primary()),
                            ZIndex(3),
                        ));
                    }
                });
        });
}

fn short_id(value: &str) -> String {
    if value.is_empty() {
        return "Unassigned target".into();
    }
    value.chars().take(8).collect()
}

fn toggle_animation_menu(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelineMenuButton>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    state.animation_menu_open = !state.animation_menu_open;
    state.clip_menu_open = false;
    state.track_menu_open = false;
    state.status.clear();
}

fn handle_animation_menu_action(
    activate: On<Activate>,
    actions: Query<&AnimationTimelineMenuActionButton>,
    players: Query<&SceneAnimationPlayer>,
    mut inputs: Query<(Entity, &mut EditableText), With<AnimationTimelineNewInput>>,
    mut commands: Commands,
    mut state: ResMut<AnimationTimelineState>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    state.animation_menu_open = false;

    match action.0 {
        AnimationTimelineMenuAction::New => {
            let Some(player) = state
                .active_player
                .and_then(|entity| players.get(entity).ok())
            else {
                state.status = "Select an AnimationPlayer entity first".into();
                return;
            };
            let suggested = next_available_animation_name(player, "Animation");
            if let Ok((entity, mut input)) = inputs.single_mut() {
                input.editor_mut().set_text(&suggested);
                commands.entity(entity).insert(AutoFocus);
            }
            state.new_dialog_open = true;
            state.new_name_error.clear();
            state.status.clear();
        }
        AnimationTimelineMenuAction::Manage
        | AnimationTimelineMenuAction::Duplicate
        | AnimationTimelineMenuAction::Rename
        | AnimationTimelineMenuAction::EditTransitions
        | AnimationTimelineMenuAction::OpenInInspector
        | AnimationTimelineMenuAction::Remove => {
            state.status = "This animation action is not available yet".into();
        }
    }
}

fn toggle_animation_clip_selector(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelineClipSelectorButton>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    state.clip_menu_open = !state.clip_menu_open;
    state.animation_menu_open = false;
    state.track_menu_open = false;
}

fn confirm_new_animation(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelineNewConfirmButton>>,
    inputs: Query<(Entity, &EditableText), With<AnimationTimelineNewInput>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut focus: Option<ResMut<bevy::input_focus::InputFocus>>,
    mut commands: Commands,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(player_entity) = state.active_player else {
        state.new_name_error = "Select an AnimationPlayer entity first".into();
        return;
    };
    let Ok((input_entity, input)) = inputs.single() else {
        state.new_name_error = "Animation name field is unavailable".into();
        return;
    };
    let requested = input.value().to_string();
    let validation = {
        let mut players = nodes.p1();
        let Ok(player) = players.get_mut(player_entity) else {
            state.new_name_error = "AnimationPlayer is no longer available".into();
            return;
        };
        validate_animation_name(&player, &requested)
    };
    let name = match validation {
        Ok(name) => name,
        Err(error) => {
            state.new_name_error = error;
            return;
        }
    };

    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Create Animation",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let clip_index = {
        let mut players = nodes.p1();
        let Ok(mut player) = players.get_mut(player_entity) else {
            state.new_name_error = "AnimationPlayer is no longer available".into();
            return;
        };
        match append_animation_clip(&mut player, &name) {
            Ok(index) => index,
            Err(error) => {
                state.new_name_error = error;
                return;
            }
        }
    };

    state.selected_clip = clip_index;
    state.current_time = 0.0;
    state.playing = false;
    state.previewing = false;
    state.selected_track = None;
    state.selected_key = None;
    state.key_drag = None;
    state.new_dialog_open = false;
    state.new_name_error.clear();
    state.status = format!("Created animation '{name}'");
    state.revision = state.revision.wrapping_add(1);
    commands.entity(input_entity).remove::<AutoFocus>();
    if let Some(focus) = focus.as_deref_mut() {
        focus.clear();
    }
    mark_document_changed(document.as_deref_mut());
}

fn cancel_new_animation(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelineNewCancelButton>>,
    inputs: Query<Entity, With<AnimationTimelineNewInput>>,
    mut focus: Option<ResMut<bevy::input_focus::InputFocus>>,
    mut commands: Commands,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    state.new_dialog_open = false;
    state.new_name_error.clear();
    if let Ok(input) = inputs.single() {
        commands.entity(input).remove::<AutoFocus>();
    }
    if let Some(focus) = focus.as_deref_mut() {
        focus.clear();
    }
}

fn select_animation_clip(
    activate: On<Activate>,
    buttons: Query<&AnimationTimelineClipButton>,
    mut state: ResMut<AnimationTimelineState>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    state.selected_clip = button.0;
    state.current_time = 0.0;
    state.playing = false;
    state.previewing = true;
    state.selected_track = None;
    state.selected_key = None;
    state.key_drag = None;
    state.clip_menu_open = false;
    state.status.clear();
    state.revision = state.revision.wrapping_add(1);
}

fn toggle_timeline_playback(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelinePlayButton>>,
    players: Query<&SceneAnimationPlayer>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let has_clip = state
        .active_player
        .and_then(|entity| players.get(entity).ok())
        .is_some_and(|player| player.clips.get(state.selected_clip).is_some());
    if has_clip {
        state.playing = !state.playing;
        state.previewing = true;
    }
}

fn stop_timeline_playback(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelineStopButton>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_ok() {
        state.playing = false;
        state.current_time = 0.0;
        state.previewing = false;
    }
}

fn toggle_track_menu(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelineAddTrackButton>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_ok() {
        state.track_menu_open = !state.track_menu_open;
        state.status.clear();
    }
}

fn add_animation_track(
    activate: On<Activate>,
    buttons: Query<&AnimationTimelineTrackKindButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    target_ids: Query<&SceneNodeId>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(player_entity) = state.active_player else {
        return;
    };
    let Some(target_entity) = selection.0 else {
        state.status = "Select a scene node as the track target".into();
        return;
    };
    let Ok(target_id) = target_ids.get(target_entity) else {
        state.status = "Selected object has no stable scene ID".into();
        return;
    };

    let has_clip = {
        let mut players = nodes.p1();
        players
            .get_mut(player_entity)
            .is_ok_and(|player| player.clips.get(state.selected_clip).is_some())
    };
    if !has_clip {
        state.status = "Create an animation clip first".into();
        return;
    }

    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Add Animation Track",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(player_entity) else {
        return;
    };
    let Some(clip) = player.clips.get_mut(state.selected_clip) else {
        return;
    };
    clip.tracks.push(SceneAnimationTrack {
        target_node: target_id.to_string(),
        property: default_track_property(button.0).into(),
        kind: button.0,
        keys: Vec::new(),
    });
    state.selected_track = Some(clip.tracks.len() - 1);
    state.selected_key = None;
    state.track_menu_open = false;
    state.status = format!("Added {} track", button.0.label());
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn default_track_property(kind: SceneAnimationTrackKind) -> &'static str {
    match kind {
        SceneAnimationTrackKind::Transform => "transform",
        SceneAnimationTrackKind::SpriteFrame => "sprite.frame",
        SceneAnimationTrackKind::Property => "property",
        SceneAnimationTrackKind::Bone => "bone.transform",
        SceneAnimationTrackKind::Animation => "animation",
        SceneAnimationTrackKind::Event => "event",
        SceneAnimationTrackKind::Audio => "audio",
    }
}

pub(crate) fn next_available_animation_name(player: &SceneAnimationPlayer, base: &str) -> String {
    let base = if base.trim().is_empty() {
        "Animation"
    } else {
        base.trim()
    };
    if !player.clips.iter().any(|clip| clip.name == base) {
        return base.to_owned();
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{base}{suffix}");
        if !player.clips.iter().any(|clip| clip.name == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn validate_animation_name(
    player: &SceneAnimationPlayer,
    requested: &str,
) -> Result<String, String> {
    let name = requested.trim();
    if name.is_empty() {
        return Err("Enter an animation name".into());
    }
    if player.clips.iter().any(|clip| clip.name == name) {
        return Err(format!("Animation '{name}' already exists"));
    }
    Ok(name.to_owned())
}

pub(crate) fn append_animation_clip(
    player: &mut SceneAnimationPlayer,
    requested: &str,
) -> Result<usize, String> {
    let name = validate_animation_name(player, requested)?;
    let index = player.clips.len();
    player.clips.push(SceneAnimationClip {
        name,
        length: 1.0,
        ..default()
    });
    Ok(index)
}

fn select_animation_track(
    activate: On<Activate>,
    buttons: Query<&AnimationTimelineTrackButton>,
    mut state: ResMut<AnimationTimelineState>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    state.selected_track = Some(button.0);
    state.selected_key = None;
    state.key_drag = None;
    state.status.clear();
    state.revision = state.revision.wrapping_add(1);
}

fn select_animation_key(
    activate: On<Activate>,
    buttons: Query<&AnimationTimelineKeyButton>,
    players: Query<&SceneAnimationPlayer>,
    mut state: ResMut<AnimationTimelineState>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(player) = state
        .active_player
        .and_then(|entity| players.get(entity).ok())
    else {
        return;
    };
    let Some(key) = player
        .clips
        .get(state.selected_clip)
        .and_then(|clip| clip.tracks.get(button.track_index))
        .and_then(|track| track.keys.get(button.key_index))
    else {
        return;
    };
    state.selected_track = Some(button.track_index);
    state.selected_key = Some((button.track_index, button.key_index));
    state.current_time = key.time.max(0.0);
    state.playing = false;
    state.previewing = true;
    state.status = format!("Key selected at {:.2}s", state.current_time);
    state.revision = state.revision.wrapping_add(1);
}

fn begin_animation_key_drag(
    mut drag: On<Pointer<DragStart>>,
    buttons: Query<(&AnimationTimelineKeyButton, &ChildOf)>,
    lanes: Query<&ComputedNode>,
    players: Query<&SceneAnimationPlayer>,
    scene_nodes: Query<SceneSnapshotQuery>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok((button, parent)) = buttons.get(drag.entity) else {
        return;
    };
    let Some(player) = state
        .active_player
        .and_then(|entity| players.get(entity).ok())
    else {
        return;
    };
    let Some(clip) = player.clips.get(state.selected_clip) else {
        return;
    };
    let Some(key) = clip
        .tracks
        .get(button.track_index)
        .and_then(|track| track.keys.get(button.key_index))
    else {
        return;
    };
    let lane_width = lanes
        .get(parent.parent())
        .map_or(1.0, |computed| computed.size().x.max(1.0));
    state.selected_track = Some(button.track_index);
    state.selected_key = Some((button.track_index, button.key_index));
    state.key_drag = Some(AnimationKeyDragState {
        track_index: button.track_index,
        key_index: button.key_index,
        start_time: key.time,
        current_time: key.time,
        clip_length: clip.length.max(0.0),
        lane_width,
        before: capture_scene_snapshot(&scene_nodes, &selection, *mode),
    });
    state.playing = false;
    state.previewing = true;
    drag.propagate(false);
}

fn drag_animation_key(
    mut drag: On<Pointer<Drag>>,
    mut buttons: Query<(&AnimationTimelineKeyButton, &mut Node)>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok((button, mut node)) = buttons.get_mut(drag.entity) else {
        return;
    };
    let Some(operation) = state.key_drag.as_mut() else {
        return;
    };
    if operation.track_index != button.track_index || operation.key_index != button.key_index {
        return;
    }
    let time = (operation.start_time
        + drag.distance.x / operation.lane_width * operation.clip_length)
        .clamp(0.0, operation.clip_length);
    operation.current_time = time;
    let percent = if operation.clip_length <= f32::EPSILON {
        0.0
    } else {
        time / operation.clip_length * 100.0
    };
    node.left = Val::Percent(percent.clamp(0.0, 100.0));
    state.current_time = time;
    state.status = format!("Move key to {time:.2}s");
    drag.propagate(false);
}

fn finish_animation_key_drag(
    mut drag: On<Pointer<DragEnd>>,
    buttons: Query<&AnimationTimelineKeyButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(drag.entity) else {
        return;
    };
    let Some(operation) = state.key_drag.take() else {
        return;
    };
    if operation.track_index != button.track_index || operation.key_index != button.key_index {
        return;
    }
    let Some(player_entity) = state.active_player else {
        return;
    };
    let final_key_index = {
        let mut players = nodes.p1();
        let Ok(mut player) = players.get_mut(player_entity) else {
            return;
        };
        let Some(track) = player
            .clips
            .get_mut(state.selected_clip)
            .and_then(|clip| clip.tracks.get_mut(operation.track_index))
        else {
            return;
        };
        let Some(index) = move_animation_key(track, operation.key_index, operation.current_time)
        else {
            return;
        };
        index
    };

    state.selected_track = Some(operation.track_index);
    state.selected_key = Some((operation.track_index, final_key_index));
    state.current_time = operation.current_time;
    state.status = format!("Key moved to {:.2}s", operation.current_time);
    state.revision = state.revision.wrapping_add(1);

    if let (Some(history), Some(document)) = (history.as_deref_mut(), document.as_deref_mut()) {
        let after = {
            let scene_nodes = nodes.p0();
            capture_scene_snapshot(&scene_nodes, &selection, *mode)
        };
        history.commit("Move Animation Key", operation.before, after, document);
    } else {
        mark_document_changed(document.as_deref_mut());
    }
    drag.propagate(false);
}

fn move_animation_key(
    track: &mut SceneAnimationTrack,
    key_index: usize,
    time: f32,
) -> Option<usize> {
    if key_index >= track.keys.len() {
        return None;
    }
    let mut moved = track.keys.remove(key_index);
    moved.time = time.max(0.0);
    if let Some(existing) = track
        .keys
        .iter_mut()
        .find(|key| (key.time - moved.time).abs() <= 0.001)
    {
        existing.value = moved.value;
    } else {
        track.keys.push(moved);
    }
    track
        .keys
        .sort_by(|left, right| left.time.total_cmp(&right.time));
    track
        .keys
        .iter()
        .position(|key| (key.time - time.max(0.0)).abs() <= 0.001)
}

fn add_animation_key(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelineAddKeyButton>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    targets: Query<(
        &SceneNodeId,
        Option<&Transform>,
        Option<&SceneUiLayout>,
        Option<&SceneSprite2D>,
    )>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(player_entity) = state.active_player else {
        return;
    };
    let Some(track_index) = state.selected_track else {
        state.status = "Select a track before adding a key".into();
        return;
    };

    let (track_kind, target_node, previous_value) = {
        let mut players = nodes.p1();
        let Ok(player) = players.get_mut(player_entity) else {
            return;
        };
        let Some(track) = player
            .clips
            .get(state.selected_clip)
            .and_then(|clip| clip.tracks.get(track_index))
        else {
            state.status = "Selected track is no longer available".into();
            return;
        };
        (
            track.kind,
            track.target_node.clone(),
            track.keys.last().map(|key| key.value.clone()),
        )
    };
    let target = targets
        .iter()
        .find(|(id, ..)| id.to_string() == target_node);
    let target_transform = target.and_then(|(_, transform, _, _)| transform);
    let target_ui_layout = target.and_then(|(_, _, layout, _)| layout);
    let target_sprite = target.and_then(|(_, _, _, sprite)| sprite);
    let value = key_value(
        track_kind,
        target_transform,
        target_ui_layout,
        target_sprite,
        previous_value,
    );

    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Add Animation Key",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(player_entity) else {
        return;
    };
    let Some(track) = player
        .clips
        .get_mut(state.selected_clip)
        .and_then(|clip| clip.tracks.get_mut(track_index))
    else {
        return;
    };
    let time = state.current_time.max(0.0);
    if let Some(existing) = track
        .keys
        .iter_mut()
        .find(|key| (key.time - time).abs() <= 0.001)
    {
        existing.value = value;
    } else {
        track.keys.push(SceneAnimationKey { time, value });
        track
            .keys
            .sort_by(|left, right| left.time.total_cmp(&right.time));
    }
    let key_index = track
        .keys
        .iter()
        .position(|key| (key.time - time).abs() <= 0.001)
        .unwrap_or(0);
    state.selected_key = Some((track_index, key_index));
    state.status = format!("Key added at {time:.2}s");
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn remove_animation_key(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelineRemoveKeyButton>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let (Some(player_entity), Some((track_index, key_index))) =
        (state.active_player, state.selected_key)
    else {
        state.status = "Select a key before removing it".into();
        return;
    };
    let exists = {
        let mut players = nodes.p1();
        players.get_mut(player_entity).is_ok_and(|player| {
            player
                .clips
                .get(state.selected_clip)
                .and_then(|clip| clip.tracks.get(track_index))
                .is_some_and(|track| key_index < track.keys.len())
        })
    };
    if !exists {
        state.selected_key = None;
        state.status = "Selected key is no longer available".into();
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let scene_nodes = nodes.p0();
        history.begin(
            "Remove Animation Key",
            capture_scene_snapshot(&scene_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(player_entity) else {
        return;
    };
    let Some(track) = player
        .clips
        .get_mut(state.selected_clip)
        .and_then(|clip| clip.tracks.get_mut(track_index))
    else {
        return;
    };
    track.keys.remove(key_index);
    state.selected_key = None;
    state.key_drag = None;
    state.status = "Key removed".into();
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn insert_inspector_animation_key(
    request: On<InsertAnimationPropertyKey>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    targets: Query<(&SceneNodeId, Option<&Transform>, Option<&SceneUiLayout>)>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    let Some(player_entity) = state.active_player else {
        state.status = "Select an AnimationPlayer before inserting keys".into();
        return;
    };
    let Some(target_entity) = selection.0 else {
        state.status = "Select a 2D or UI entity to insert a key".into();
        return;
    };
    let Ok((target_id, transform, ui_layout)) = targets.get(target_entity) else {
        state.status = "Selected entity has no animatable Transform".into();
        return;
    };
    if transform.is_none() && ui_layout.is_none() {
        state.status = "Selected entity has no animatable Transform".into();
        return;
    }
    let value = key_value(
        SceneAnimationTrackKind::Transform,
        transform,
        ui_layout,
        None,
        None,
    );
    let target_node = target_id.to_string();
    let property = request.property;
    let time = state.current_time.max(0.0);

    let has_clip = {
        let mut players = nodes.p1();
        players
            .get_mut(player_entity)
            .is_ok_and(|player| player.clips.get(state.selected_clip).is_some())
    };
    if !has_clip {
        state.status = "Create or select an animation before inserting keys".into();
        return;
    }

    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            format!("Insert {} Key", property.label()),
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(player_entity) else {
        return;
    };
    let Ok((track_index, key_index)) = upsert_transform_property_key(
        &mut player,
        state.selected_clip,
        &target_node,
        property,
        time,
        value,
    ) else {
        state.status = "Selected animation is no longer available".into();
        return;
    };

    state.selected_track = Some(track_index);
    state.selected_key = Some((track_index, key_index));
    state.previewing = true;
    state.playing = false;
    state.track_menu_open = false;
    state.status = format!("{} key inserted at {time:.2}s", property.label());
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn insert_inspector_sprite_frame_key(
    _request: On<InsertSpriteFrameKey>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    targets: Query<(&SceneNodeId, &SceneSprite2D)>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    let Some(player_entity) = state.active_player else {
        state.status = "Select an AnimationPlayer before inserting keys".into();
        return;
    };
    let Some(target_entity) = selection.0 else {
        state.status = "Select a Sprite2D entity to insert a frame key".into();
        return;
    };
    let Ok((target_id, sprite)) = targets.get(target_entity) else {
        state.status = "Selected entity has no Sprite2D frame".into();
        return;
    };
    let target_node = target_id.to_string();
    let value = format_sprite_frame(sprite.clamped_frame());
    let time = state.current_time.max(0.0);

    let has_clip = {
        let mut players = nodes.p1();
        players
            .get_mut(player_entity)
            .is_ok_and(|player| player.clips.get(state.selected_clip).is_some())
    };
    if !has_clip {
        state.status = "Create or select an animation before inserting keys".into();
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Insert Sprite Frame Key",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(player_entity) else {
        return;
    };
    let Ok((track_index, key_index)) = upsert_track_key(
        &mut player,
        state.selected_clip,
        &target_node,
        SceneAnimationTrackKind::SpriteFrame,
        "sprite.frame",
        time,
        value,
    ) else {
        state.status = "Selected animation is no longer available".into();
        return;
    };
    state.selected_track = Some(track_index);
    state.selected_key = Some((track_index, key_index));
    state.previewing = true;
    state.playing = false;
    state.track_menu_open = false;
    state.status = format!("Sprite Frame key inserted at {time:.2}s");
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn upsert_transform_property_key(
    player: &mut SceneAnimationPlayer,
    clip_index: usize,
    target_node: &str,
    property: AnimationTransformProperty,
    time: f32,
    value: String,
) -> Result<(usize, usize), ()> {
    upsert_track_key(
        player,
        clip_index,
        target_node,
        SceneAnimationTrackKind::Transform,
        property.path(),
        time,
        value,
    )
}

fn upsert_track_key(
    player: &mut SceneAnimationPlayer,
    clip_index: usize,
    target_node: &str,
    kind: SceneAnimationTrackKind,
    property_path: &str,
    time: f32,
    value: String,
) -> Result<(usize, usize), ()> {
    let clip = player.clips.get_mut(clip_index).ok_or(())?;
    let track_index = clip
        .tracks
        .iter()
        .position(|track| {
            track.kind == kind
                && track.target_node == target_node
                && track.property == property_path
        })
        .unwrap_or_else(|| {
            clip.tracks.push(SceneAnimationTrack {
                target_node: target_node.to_owned(),
                property: property_path.to_owned(),
                kind,
                keys: Vec::new(),
            });
            clip.tracks.len() - 1
        });
    let track = &mut clip.tracks[track_index];
    let time = time.max(0.0);
    let key_index = if let Some((index, existing)) = track
        .keys
        .iter_mut()
        .enumerate()
        .find(|(_, key)| (key.time - time).abs() <= 0.001)
    {
        existing.value = value;
        index
    } else {
        track.keys.push(SceneAnimationKey { time, value });
        track
            .keys
            .sort_by(|left, right| left.time.total_cmp(&right.time));
        track
            .keys
            .iter()
            .position(|key| (key.time - time).abs() <= 0.001)
            .unwrap_or(0)
    };
    Ok((track_index, key_index))
}

fn remove_animation_track(
    activate: On<Activate>,
    buttons: Query<(), With<AnimationTimelineRemoveTrackButton>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let (Some(player_entity), Some(track_index)) = (state.active_player, state.selected_track)
    else {
        state.status = "Select a track before removing it".into();
        return;
    };
    let exists = {
        let mut players = nodes.p1();
        players.get_mut(player_entity).is_ok_and(|player| {
            player
                .clips
                .get(state.selected_clip)
                .is_some_and(|clip| track_index < clip.tracks.len())
        })
    };
    if !exists {
        state.status = "Selected track is no longer available".into();
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Remove Animation Track",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(player_entity) else {
        return;
    };
    let Some(clip) = player.clips.get_mut(state.selected_clip) else {
        return;
    };
    clip.tracks.remove(track_index);
    state.selected_track = track_index
        .checked_sub(1)
        .or_else(|| (!clip.tracks.is_empty()).then_some(0));
    state.selected_key = None;
    state.key_drag = None;
    state.status = "Track removed".into();
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn key_value(
    kind: SceneAnimationTrackKind,
    transform: Option<&Transform>,
    ui_layout: Option<&SceneUiLayout>,
    sprite: Option<&SceneSprite2D>,
    previous: Option<String>,
) -> String {
    if matches!(
        kind,
        SceneAnimationTrackKind::Transform | SceneAnimationTrackKind::Bone
    ) && let Some(transform) = transform
    {
        return format_animation_transform(transform);
    }
    if matches!(
        kind,
        SceneAnimationTrackKind::Transform | SceneAnimationTrackKind::Bone
    ) && let Some(layout) = ui_layout
    {
        return format_animation_transform(&animation_transform_from_ui_layout(layout));
    }
    if matches!(kind, SceneAnimationTrackKind::SpriteFrame)
        && let Some(sprite) = sprite
    {
        return format_sprite_frame(sprite.clamped_frame());
    }
    previous.unwrap_or_else(|| match kind {
        SceneAnimationTrackKind::Event => "event".into(),
        SceneAnimationTrackKind::Audio => "res://".into(),
        SceneAnimationTrackKind::Animation => "Animation".into(),
        _ => "0".into(),
    })
}

fn mark_document_changed(document: Option<&mut SceneDocument>) {
    if let Some(document) = document {
        document.open = true;
        document.dirty = true;
        document.bump_revision();
    }
}

fn advance_animation_playhead(
    time: Res<Time>,
    players: Query<&SceneAnimationPlayer>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if !state.playing {
        return;
    }
    let Some(player) = state
        .active_player
        .and_then(|entity| players.get(entity).ok())
    else {
        state.playing = false;
        return;
    };
    let Some(clip) = player.clips.get(state.selected_clip) else {
        state.playing = false;
        return;
    };
    let length = clip.length.max(0.0);
    if length <= f32::EPSILON {
        state.playing = false;
        state.current_time = 0.0;
        return;
    }
    state.current_time += time.delta_secs() * player.speed.max(0.0);
    if state.current_time >= length {
        if clip.looped {
            state.current_time %= length;
        } else {
            state.current_time = length;
            state.playing = false;
        }
    }
}

#[derive(Default)]
struct AnimationPreviewCache {
    signature: Option<(Entity, usize, u32, SceneAnimationPlayer)>,
    applied: bool,
}

/// 把时间轴当前时间真正应用到编辑 World，同时保留场景的原始静态值。
fn preview_animation_pose(
    players: Query<&SceneAnimationPlayer>,
    state: Res<AnimationTimelineState>,
    mut targets: Query<(
        Entity,
        &SceneNodeId,
        Option<&mut Transform>,
        Option<&mut SceneUiLayout>,
        Option<&mut SceneSprite2D>,
        Option<&AnimationPreviewOriginalTransform>,
        Option<&AnimationPreviewOriginalUiLayout>,
        Option<&AnimationPreviewOriginalSpriteFrame>,
    )>,
    mut commands: Commands,
    mut cache: Local<AnimationPreviewCache>,
) {
    let player = state
        .active_player
        .filter(|_| state.previewing)
        .and_then(|entity| players.get(entity).ok().map(|player| (entity, player)));
    let signature = player.map(|(entity, player)| {
        (
            entity,
            state.selected_clip,
            state.current_time.to_bits(),
            player.clone(),
        )
    });
    if cache.signature == signature {
        return;
    }

    // 每次重新取样先回到静态场景值，避免预览帧逐步累积误差。
    if cache.applied {
        for (
            entity,
            _,
            transform,
            layout,
            sprite,
            original_transform,
            original_layout,
            original_sprite,
        ) in &mut targets
        {
            if let (Some(mut transform), Some(original)) = (transform, original_transform) {
                *transform = original.0;
            }
            if let (Some(mut layout), Some(original)) = (layout, original_layout) {
                *layout = original.0;
            }
            if let (Some(mut sprite), Some(original)) = (sprite, original_sprite) {
                sprite.frame = original.0;
            }
            commands
                .entity(entity)
                .remove::<AnimationPreviewOriginalTransform>()
                .remove::<AnimationPreviewOriginalUiLayout>()
                .remove::<AnimationPreviewOriginalSpriteFrame>();
        }
    }

    cache.signature = signature;
    cache.applied = false;
    let Some((_, player)) = player else {
        return;
    };
    let Some(clip) = player.clips.get(state.selected_clip) else {
        return;
    };

    let mut original_transforms = HashMap::new();
    let mut original_layouts = HashMap::new();
    let mut original_frames = HashMap::new();
    for track in &clip.tracks {
        let Some((
            entity,
            _,
            transform,
            layout,
            sprite,
            original_transform,
            original_layout,
            original_sprite,
        )) = targets
            .iter_mut()
            .find(|(_, id, ..)| id.to_string() == track.target_node)
        else {
            continue;
        };
        if matches!(track.kind, SceneAnimationTrackKind::SpriteFrame) {
            let (Some(sample), Some(mut sprite)) =
                (sample_sprite_frame(&track.keys, state.current_time), sprite)
            else {
                continue;
            };
            let original = *original_frames
                .entry(entity)
                .or_insert_with(|| original_sprite.map_or(sprite.frame, |original| original.0));
            commands
                .entity(entity)
                .insert(AnimationPreviewOriginalSpriteFrame(original));
            sprite.frame = sample.min(sprite.frame_count().saturating_sub(1));
            cache.applied = true;
            continue;
        }
        if !matches!(track.kind, SceneAnimationTrackKind::Transform) {
            continue;
        }
        let Some(sample) = sample_animation_transform(&track.keys, state.current_time) else {
            continue;
        };
        if let Some(mut transform) = transform {
            let original = *original_transforms
                .entry(entity)
                .or_insert_with(|| original_transform.map_or(*transform, |original| original.0));
            commands
                .entity(entity)
                .insert(AnimationPreviewOriginalTransform(original));
            apply_sample_to_transform_property(&sample, &track.property, &mut transform);
            cache.applied = true;
            continue;
        }
        if let Some(mut layout) = layout {
            let original = *original_layouts
                .entry(entity)
                .or_insert_with(|| original_layout.map_or(*layout, |original| original.0));
            commands
                .entity(entity)
                .insert(AnimationPreviewOriginalUiLayout(original));
            apply_sample_to_ui_layout_property(&sample, &track.property, &mut layout);
            cache.applied = true;
        }
    }
}

fn scrub_animation_timeline_press(
    press: On<Pointer<Press>>,
    areas: Query<&RelativeCursorPosition, With<AnimationTimelineScrubArea>>,
    players: Query<&SceneAnimationPlayer>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let Ok(cursor) = areas.get(press.entity) {
        set_time_from_cursor(cursor, &players, &mut state);
    }
}

fn scrub_animation_timeline_drag(
    drag: On<Pointer<Drag>>,
    areas: Query<&RelativeCursorPosition, With<AnimationTimelineScrubArea>>,
    players: Query<&SceneAnimationPlayer>,
    mut state: ResMut<AnimationTimelineState>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    if let Ok(cursor) = areas.get(drag.entity) {
        set_time_from_cursor(cursor, &players, &mut state);
    }
}

fn set_time_from_cursor(
    cursor: &RelativeCursorPosition,
    players: &Query<&SceneAnimationPlayer>,
    state: &mut AnimationTimelineState,
) {
    let Some(normalized) = cursor.normalized else {
        return;
    };
    let length = state
        .active_player
        .and_then(|entity| players.get(entity).ok())
        .and_then(|player| player.clips.get(state.selected_clip))
        .map_or(0.0, |clip| clip.length.max(0.0));
    state.current_time = (normalized.x + 0.5).clamp(0.0, 1.0) * length;
    state.playing = false;
    state.previewing = true;
}

fn sync_animation_timeline_chrome(
    players: Query<&SceneAnimationPlayer>,
    state: Res<AnimationTimelineState>,
    asset_server: Res<AssetServer>,
    mut labels: ParamSet<(
        Query<&mut Text, With<AnimationTimelineTimeLabel>>,
        Query<&mut Text, With<AnimationTimelineClipLabel>>,
        Query<&mut Text, With<AnimationTimelineStatusLabel>>,
        Query<(&AnimationTimelineRulerTick, &mut Text)>,
        Query<&mut Text, With<AnimationTimelineNewErrorLabel>>,
    )>,
    mut icons: Query<&mut ImageNode, With<AnimationTimelinePlayIcon>>,
    mut chrome_nodes: ParamSet<(
        Query<&mut Node, With<AnimationTimelinePlayhead>>,
        Query<&mut Node, With<AnimationTimelineTrackMenu>>,
        Query<&mut Node, With<AnimationTimelineAnimationMenu>>,
        Query<&mut Node, With<AnimationTimelineClipSelectorMenu>>,
        Query<&mut Node, With<AnimationTimelineNewDialog>>,
    )>,
) {
    let clip = state
        .active_player
        .and_then(|entity| players.get(entity).ok())
        .and_then(|player| player.clips.get(state.selected_clip));
    let length = clip.map_or(0.0, |clip| clip.length.max(0.0));

    for mut label in &mut labels.p0() {
        label.0 = format!("{:.2} / {:.2} s", state.current_time, length);
    }
    for mut label in &mut labels.p1() {
        label.0 = clip.map_or_else(|| "No animation".into(), |clip| clip.name.clone());
    }
    for mut label in &mut labels.p2() {
        label.0 = state.status.clone();
    }
    for mut node in &mut chrome_nodes.p1() {
        node.display = if state.track_menu_open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut chrome_nodes.p2() {
        node.display = if state.animation_menu_open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut chrome_nodes.p3() {
        node.display = if state.clip_menu_open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut chrome_nodes.p4() {
        node.display = if state.new_dialog_open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (tick, mut text) in &mut labels.p3() {
        text.0 = format!("{:.1}", length * tick.0 as f32 / 10.0);
    }
    for mut label in &mut labels.p4() {
        label.0 = state.new_name_error.clone();
    }
    let icon_path = if state.playing {
        "editor/icons/pause.png"
    } else {
        "editor/icons/play.png"
    };
    for mut icon in &mut icons {
        icon.image = asset_server.load(icon_path);
    }
    let percent = if length > f32::EPSILON {
        (state.current_time / length * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    for mut node in &mut chrome_nodes.p0() {
        node.left = Val::Percent(percent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_types_have_stable_default_properties() {
        assert_eq!(
            default_track_property(SceneAnimationTrackKind::Transform),
            "transform"
        );
        assert_eq!(
            default_track_property(SceneAnimationTrackKind::SpriteFrame),
            "sprite.frame"
        );
        assert_eq!(
            default_track_property(SceneAnimationTrackKind::Event),
            "event"
        );
    }

    #[test]
    fn new_animation_names_are_unique_and_persist_default_length() {
        let mut player = SceneAnimationPlayer {
            clips: vec![SceneAnimationClip {
                name: "Animation".into(),
                ..default()
            }],
            ..default()
        };

        assert_eq!(
            next_available_animation_name(&player, "Animation"),
            "Animation2"
        );
        let index = append_animation_clip(&mut player, "  Walk  ").unwrap();

        assert_eq!(index, 1);
        assert_eq!(player.clips[index].name, "Walk");
        assert_eq!(player.clips[index].length, 1.0);
        assert!(append_animation_clip(&mut player, "Walk").is_err());
        assert!(append_animation_clip(&mut player, "  ").is_err());
    }

    #[test]
    fn transform_key_captures_all_transform_parts() {
        let transform = Transform {
            translation: Vec3::new(12.0, 24.0, 3.0),
            rotation: Quat::from_rotation_z(0.5),
            scale: Vec3::new(2.0, 3.0, 1.0),
        };
        let value = key_value(
            SceneAnimationTrackKind::Transform,
            Some(&transform),
            None,
            None,
            None,
        );
        assert!(value.contains("translation:12.0000,24.0000,3.0000"));
        assert!(value.contains("rotation:"));
        assert!(value.contains("scale:2.0000,3.0000,1.0000"));
    }

    #[test]
    fn ui_transform_key_uses_layout_position_rotation_and_scale() {
        let mut layout = SceneUiLayout::sized(120.0, 40.0);
        layout.offset = (24.0, 36.0);
        layout.rotation = 45.0;
        layout.scale = (2.0, 0.5);
        let value = key_value(
            SceneAnimationTrackKind::Transform,
            None,
            Some(&layout),
            None,
            None,
        );
        let parsed = arisna_engine::parse_animation_transform(&value).unwrap();
        assert!(
            parsed
                .translation
                .abs_diff_eq(Vec3::new(24.0, 36.0, 0.0), 0.001)
        );
        assert!(parsed.scale.abs_diff_eq(Vec3::new(2.0, 0.5, 1.0), 0.001));
    }

    #[test]
    fn inspector_key_creates_reuses_and_sorts_property_tracks() {
        let mut player = SceneAnimationPlayer {
            clips: vec![SceneAnimationClip {
                name: "Move".into(),
                length: 2.0,
                ..default()
            }],
            ..default()
        };

        let (position_track, _) = upsert_transform_property_key(
            &mut player,
            0,
            "player",
            AnimationTransformProperty::Position,
            1.0,
            "middle".into(),
        )
        .unwrap();
        let (same_track, _) = upsert_transform_property_key(
            &mut player,
            0,
            "player",
            AnimationTransformProperty::Position,
            1.0,
            "updated".into(),
        )
        .unwrap();
        upsert_transform_property_key(
            &mut player,
            0,
            "player",
            AnimationTransformProperty::Position,
            0.25,
            "first".into(),
        )
        .unwrap();
        let (rotation_track, _) = upsert_transform_property_key(
            &mut player,
            0,
            "player",
            AnimationTransformProperty::Rotation,
            0.5,
            "rotation".into(),
        )
        .unwrap();

        assert_eq!(position_track, same_track);
        assert_ne!(position_track, rotation_track);
        assert_eq!(
            player.clips[0].tracks[position_track].property,
            "transform.position"
        );
        assert_eq!(player.clips[0].tracks[position_track].keys.len(), 2);
        assert_eq!(player.clips[0].tracks[position_track].keys[0].time, 0.25);
        assert_eq!(
            player.clips[0].tracks[position_track].keys[1].value,
            "updated"
        );
        assert_eq!(
            player.clips[0].tracks[rotation_track].property,
            "transform.rotation"
        );
    }

    #[test]
    fn moved_keys_sort_and_replace_a_key_at_the_same_time() {
        let mut track = SceneAnimationTrack {
            keys: vec![
                SceneAnimationKey {
                    time: 0.0,
                    value: "start".into(),
                },
                SceneAnimationKey {
                    time: 1.0,
                    value: "middle".into(),
                },
                SceneAnimationKey {
                    time: 2.0,
                    value: "end".into(),
                },
            ],
            ..default()
        };

        let moved_index = move_animation_key(&mut track, 2, 0.5).unwrap();
        assert_eq!(moved_index, 1);
        assert_eq!(
            track.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
            vec![0.0, 0.5, 1.0]
        );

        let merged_index = move_animation_key(&mut track, moved_index, 1.0).unwrap();
        assert_eq!(merged_index, 1);
        assert_eq!(track.keys.len(), 2);
        assert_eq!(track.keys[merged_index].value, "end");
    }

    #[test]
    fn inspector_key_event_inserts_into_the_active_animation_player() {
        let mut app = App::new();
        app.init_resource::<AnimationTimelineState>()
            .init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .add_observer(insert_inspector_animation_key);
        let target = app
            .world_mut()
            .spawn((SceneNodeId::new(), Transform::from_xyz(18.0, 26.0, 0.0)))
            .id();
        let player = app
            .world_mut()
            .spawn(SceneAnimationPlayer {
                clips: vec![SceneAnimationClip {
                    name: "Move".into(),
                    length: 2.0,
                    ..default()
                }],
                ..default()
            })
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(target);
        *app.world_mut().resource_mut::<AnimationTimelineState>() = AnimationTimelineState {
            active_player: Some(player),
            current_time: 0.75,
            ..default()
        };

        app.world_mut().trigger(InsertAnimationPropertyKey {
            property: AnimationTransformProperty::Position,
        });

        let player = app.world().get::<SceneAnimationPlayer>(player).unwrap();
        assert_eq!(player.clips[0].tracks.len(), 1);
        assert_eq!(player.clips[0].tracks[0].property, "transform.position");
        assert_eq!(player.clips[0].tracks[0].keys[0].time, 0.75);
        let value =
            arisna_engine::parse_animation_transform(&player.clips[0].tracks[0].keys[0].value)
                .unwrap();
        assert_eq!(value.translation, Vec3::new(18.0, 26.0, 0.0));
    }

    #[test]
    fn inspector_key_event_captures_ui_layout_values() {
        let mut app = App::new();
        app.init_resource::<AnimationTimelineState>()
            .init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .add_observer(insert_inspector_animation_key);
        let mut layout = SceneUiLayout::sized(160.0, 48.0);
        layout.offset = (30.0, 44.0);
        layout.rotation = 20.0;
        layout.scale = (1.5, 0.75);
        let target = app.world_mut().spawn((SceneNodeId::new(), layout)).id();
        let player = app
            .world_mut()
            .spawn(SceneAnimationPlayer {
                clips: vec![SceneAnimationClip {
                    name: "Pulse".into(),
                    length: 1.0,
                    ..default()
                }],
                ..default()
            })
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(target);
        *app.world_mut().resource_mut::<AnimationTimelineState>() = AnimationTimelineState {
            active_player: Some(player),
            current_time: 0.5,
            ..default()
        };

        app.world_mut().trigger(InsertAnimationPropertyKey {
            property: AnimationTransformProperty::Scale,
        });

        let player = app.world().get::<SceneAnimationPlayer>(player).unwrap();
        assert_eq!(player.clips[0].tracks[0].property, "transform.scale");
        let value =
            arisna_engine::parse_animation_transform(&player.clips[0].tracks[0].keys[0].value)
                .unwrap();
        assert_eq!(value.scale, Vec3::new(1.5, 0.75, 1.0));
    }

    #[test]
    fn inspector_sprite_frame_key_creates_and_reuses_sprite_track() {
        let mut app = App::new();
        app.init_resource::<AnimationTimelineState>()
            .init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .add_observer(insert_inspector_sprite_frame_key);
        let target = app
            .world_mut()
            .spawn((
                SceneNodeId::new(),
                SceneSprite2D {
                    hframes: 4,
                    vframes: 2,
                    frame: 5,
                    ..default()
                },
            ))
            .id();
        let player = app
            .world_mut()
            .spawn(SceneAnimationPlayer {
                clips: vec![SceneAnimationClip {
                    name: "Run".into(),
                    length: 1.0,
                    ..default()
                }],
                ..default()
            })
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(target);
        *app.world_mut().resource_mut::<AnimationTimelineState>() = AnimationTimelineState {
            active_player: Some(player),
            current_time: 0.25,
            ..default()
        };

        app.world_mut().trigger(InsertSpriteFrameKey);
        app.world_mut().trigger(InsertSpriteFrameKey);

        let player = app.world().get::<SceneAnimationPlayer>(player).unwrap();
        assert_eq!(player.clips[0].tracks.len(), 1);
        assert_eq!(
            player.clips[0].tracks[0].kind,
            SceneAnimationTrackKind::SpriteFrame
        );
        assert_eq!(player.clips[0].tracks[0].property, "sprite.frame");
        assert_eq!(player.clips[0].tracks[0].keys.len(), 1);
        assert_eq!(player.clips[0].tracks[0].keys[0].value, "5");
    }

    #[test]
    fn editor_preview_applies_and_stop_restores_original_transform() {
        let target_id = SceneNodeId::new();
        let keys = vec![
            SceneAnimationKey {
                time: 0.0,
                value: format_animation_transform(&Transform::default()),
            },
            SceneAnimationKey {
                time: 2.0,
                value: format_animation_transform(&Transform::from_xyz(100.0, 40.0, 0.0)),
            },
        ];
        let mut app = App::new();
        app.init_resource::<AnimationTimelineState>()
            .add_systems(Update, preview_animation_pose);
        let player = app
            .world_mut()
            .spawn(SceneAnimationPlayer {
                clips: vec![arisna_engine::SceneAnimationClip {
                    name: "Move".into(),
                    length: 2.0,
                    tracks: vec![SceneAnimationTrack {
                        target_node: target_id.to_string(),
                        property: "transform".into(),
                        kind: SceneAnimationTrackKind::Transform,
                        keys,
                    }],
                    ..default()
                }],
                ..default()
            })
            .id();
        let target = app
            .world_mut()
            .spawn((target_id, Transform::from_xyz(10.0, 0.0, 0.0)))
            .id();
        *app.world_mut().resource_mut::<AnimationTimelineState>() = AnimationTimelineState {
            active_player: Some(player),
            current_time: 1.0,
            previewing: true,
            ..default()
        };

        app.update();
        assert!(
            app.world()
                .get::<Transform>(target)
                .unwrap()
                .translation
                .abs_diff_eq(Vec3::new(50.0, 20.0, 0.0), 0.001)
        );
        assert!(
            app.world()
                .get::<AnimationPreviewOriginalTransform>(target)
                .is_some()
        );

        app.world_mut()
            .resource_mut::<AnimationTimelineState>()
            .previewing = false;
        app.update();
        assert_eq!(
            app.world().get::<Transform>(target).unwrap().translation,
            Vec3::new(10.0, 0.0, 0.0)
        );
        assert!(
            app.world()
                .get::<AnimationPreviewOriginalTransform>(target)
                .is_none()
        );
    }

    #[test]
    fn editor_preview_applies_and_stop_restores_original_sprite_frame() {
        let target_id = SceneNodeId::new();
        let mut app = App::new();
        app.init_resource::<AnimationTimelineState>()
            .add_systems(Update, preview_animation_pose);
        let player = app
            .world_mut()
            .spawn(SceneAnimationPlayer {
                clips: vec![SceneAnimationClip {
                    name: "Run".into(),
                    length: 1.0,
                    tracks: vec![SceneAnimationTrack {
                        target_node: target_id.to_string(),
                        property: "sprite.frame".into(),
                        kind: SceneAnimationTrackKind::SpriteFrame,
                        keys: vec![
                            SceneAnimationKey {
                                time: 0.0,
                                value: "2".into(),
                            },
                            SceneAnimationKey {
                                time: 0.5,
                                value: "6".into(),
                            },
                        ],
                    }],
                    ..default()
                }],
                ..default()
            })
            .id();
        let target = app
            .world_mut()
            .spawn((
                target_id,
                SceneSprite2D {
                    hframes: 4,
                    vframes: 2,
                    frame: 1,
                    ..default()
                },
            ))
            .id();
        *app.world_mut().resource_mut::<AnimationTimelineState>() = AnimationTimelineState {
            active_player: Some(player),
            current_time: 0.75,
            previewing: true,
            ..default()
        };

        app.update();
        assert_eq!(app.world().get::<SceneSprite2D>(target).unwrap().frame, 6);
        assert!(
            app.world()
                .get::<AnimationPreviewOriginalSpriteFrame>(target)
                .is_some()
        );

        app.world_mut()
            .resource_mut::<AnimationTimelineState>()
            .previewing = false;
        app.update();
        assert_eq!(app.world().get::<SceneSprite2D>(target).unwrap().frame, 1);
        assert!(
            app.world()
                .get::<AnimationPreviewOriginalSpriteFrame>(target)
                .is_none()
        );
    }
}
