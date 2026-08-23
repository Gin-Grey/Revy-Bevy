//! Marker components and small pieces of editor UI state.

use bevy::{gizmos::transform_gizmo::TransformGizmoMode, prelude::*};

use crate::workspace::WorkspaceMode;

#[derive(Component, Clone, Copy, Default)]
pub struct DetailsHost;

#[derive(Component, Clone, Copy, Default)]
pub struct FileSystemHost;

#[derive(Component, Clone, Copy, Default)]
pub struct FileSystemSplitterHost;

#[derive(Component, Clone, Copy, Default)]
pub struct SceneSplitterHost;

#[derive(Component, Clone, Copy, Default)]
pub struct DetailsSplitterHost;

#[derive(Component, Clone, Copy, Default)]
pub struct ViewportSideSplitterHost;

#[derive(Component, Clone, Copy, Default)]
pub struct ViewportPreviewSplitterHost;

/// Associates a viewport toolbar button with a Bevy transform-gizmo mode.
#[derive(Component, Clone, Copy, Default)]
pub struct GizmoToolbarButton(pub TransformGizmoMode);

#[derive(Component, Clone, Copy, Default)]
pub struct Snap2dToolbarButton;

#[derive(Component, Clone, Copy, Default)]
pub struct Snap2dGridButton;

#[derive(Component, Clone, Copy, Default)]
pub struct Snap2dStepButton;

#[derive(Component, Clone, Copy, Default)]
pub struct Snap2dStepLabel;

/// Shows a toolbar group only while its workspace is active.
#[derive(Component, Clone, Copy, Default)]
pub struct WorkspaceToolbarGroup(pub WorkspaceMode);

#[derive(Component, Clone, Copy, Default)]
pub struct SceneTabBar;

#[derive(Component, Clone, Copy, Default)]
pub struct SceneTabLabel;

#[derive(Component, Clone, Copy, Default)]
pub struct SceneLevelLabel;

#[derive(Component, Clone, Copy, Default)]
pub struct SceneSaveDialogHost;

#[derive(Component, Clone, Copy, Default)]
pub struct SystemScriptPickerHost;

/// 标记编辑器中的竖向滚动目标。
///
/// 该实体的父节点必须是固定尺寸且启用裁剪的 Host，公共系统会把滚动条
/// 作为 Host 的另一个子节点挂载，避免滚动条跟随内容移动。
#[derive(Component, Clone, Copy, Default)]
pub struct EditorVerticalScrollArea;

#[derive(Component, Clone, Copy, Default)]
pub struct EditorVerticalScrollbar;

/// 动画时间轴挂载点和动态内容标记。
#[derive(Component, Clone, Copy, Default)]
pub struct AnimationTimelineHost;

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationTimelinePanel;

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationTimelineBody;

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationTimelineClipList;

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationTimelinePlayButton;

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationTimelineTimeLabel;

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationTimelineClipLabel;

#[derive(Component, Clone, Copy)]
pub struct AnimationTimelineClipButton(pub usize);
