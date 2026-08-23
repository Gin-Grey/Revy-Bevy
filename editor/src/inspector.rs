use arisna_engine::{
    SceneCustomComponent, SceneSystemBinding, scene_image_asset_path, scene_model_resource_path,
    scene_sprite_frame_rect, scene_sprite_rect,
};
use bevy::{
    input_focus::InputFocus,
    picking::Pickable,
    prelude::*,
    sprite::Anchor,
    text::{EditableText, EditableTextFilter, TextCursorStyle},
    ui_widgets::{Activate, Button as WidgetButton},
};

use crate::entities::{
    AddedEntityComponents, BuiltinComponent, EntityCustomComponents, EntityKind,
    EntityScriptBinding, EntitySystemBindings, NeedsDefaultMesh3D, NeedsModel3dFocus,
    SceneAnimationPlayer, SceneCollisionRect2D, SceneModel3D, SceneSprite2D, SceneUiContent,
    SceneUiLayout, UiAlignment, insert_builtin_component, remove_builtin_component,
};
use crate::filesystem::{
    FileSystemState, FsTreeRow, image_resource_path_from_filesystem,
    model_resource_path_from_filesystem, rust_script_resource_path_from_filesystem,
};
use crate::hierarchy::SceneNodeId;
use crate::output::{OutputLevel, OutputLog};
use crate::rust_components::RustSystemDefinition;
use crate::rust_components::{RustComponentDefinition, RustComponentRegistry};
use crate::scene::SceneDocument;
use crate::selection::{EditableObject, Selection};
use crate::ui::{
    components::{EditorVerticalScrollArea, SystemScriptPickerHost},
    theme,
};
use crate::undo::{SceneHistory, SceneSnapshotQuery, capture_scene_snapshot};
use crate::workspace::{SceneSpace, WorkspaceMode};
#[cfg(test)]
use arisna_engine::SceneAnimationClip;
use arisna_engine::{ProjectRoot, SceneEntityScript, SceneEntityScriptCallback};
use bevy::world_serialization::WorldAssetRoot;

use crate::animation_timeline::{
    AnimationTransformProperty, InsertAnimationPropertyKey, InsertSpriteFrameKey,
    append_animation_clip, next_available_animation_name,
};

/// Marker on the inspector body root (for future rebuilds).
#[derive(Component, Clone, Copy, Default)]
pub struct InspectorPanel;

#[derive(Component, Clone, Copy, Default)]
pub struct InspectorNameLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorEntityIdLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorEntityHeader;

#[derive(Component, Clone, Copy, Default)]
struct InspectorEntityIcon;

#[derive(Component, Clone, Copy, Default)]
struct InspectorEntityScriptDropTarget;

#[derive(Component, Clone, Copy, Default)]
struct InspectorEntityScriptName;

#[derive(Component, Clone, Copy, Default)]
struct InspectorOpenEntityScriptButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorClearEntityScriptButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorEntityScriptLifecycleList;

#[derive(Component, Clone)]
struct InspectorEntityScriptCallbackToggle(String);

#[derive(Component, Clone, Copy, Default)]
struct InspectorComponentsSection;

#[derive(Component, Clone, Copy, Default)]
struct InspectorComponentsBody;

#[derive(Component, Clone, Copy, Default)]
struct InspectorComponentsToggle;

#[derive(Component, Clone, Copy, Default)]
struct InspectorComponentsChevron;

#[derive(Component, Clone, Copy, Default)]
struct InspectorComponentsCount;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSystemsSection;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSystemsBody;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSystemsToggle;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSystemsChevron;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSystemsCount;

/// AnimationPlayer is a first-class component panel, separate from the ECS
/// system list because its clips are authored data rather than auto-matched systems.
#[derive(Component, Clone, Copy, Default)]
struct InspectorAnimationSection;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAnimationAutoplayLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAnimationAutoplayButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAnimationAutoplayMenu;

#[derive(Component, Clone, Copy)]
struct InspectorAnimationAutoplayOption(Option<usize>);

#[derive(Resource, Debug, Default)]
struct InspectorAnimationUiState {
    autoplay_open: bool,
}

#[derive(Component, Clone, Copy, Default)]
struct InspectorAnimationSpeedInput;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAnimationClipList;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAddAnimationButton;

#[derive(Component, Clone, Copy)]
struct InspectorAnimationClipNameInput(usize);

#[derive(Component, Clone, Copy)]
struct InspectorAnimationClipLengthInput(usize);

#[derive(Component, Clone, Copy)]
struct InspectorAnimationClipLoopButton(usize);

#[derive(Component, Clone, Copy)]
struct InspectorRemoveAnimationButton(usize);

#[derive(Component, Clone, Copy, Default)]
struct InspectorNoMatchingSystems;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorSystemGroupKind {
    AutoMatch,
    ExplicitBindings,
}

#[derive(Component, Clone, Copy, Default)]
struct InspectorSystemGroup;

#[derive(Component, Clone, Copy)]
struct InspectorSystemGroupBody(InspectorSystemGroupKind);

#[derive(Component, Clone, Copy)]
struct InspectorSystemGroupToggle(InspectorSystemGroupKind);

#[derive(Component, Clone, Copy)]
struct InspectorSystemGroupChevron(InspectorSystemGroupKind);

#[derive(Component, Clone, Copy)]
struct InspectorSystemGroupCount(InspectorSystemGroupKind);

#[derive(Component, Clone, Copy, Default)]
struct InspectorExplicitSystemsList;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAddSystemButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorExplicitSystemCard;

#[derive(Component, Clone, Copy, Default)]
struct InspectorExplicitSystemBody;

#[derive(Component, Clone, Copy)]
struct InspectorExplicitSystemToggle(usize);

#[derive(Component, Clone, Copy)]
struct InspectorRemoveSystemButton(usize);

#[derive(Component, Clone, Copy)]
struct InspectorSystemScheduleButton(usize);

#[derive(Component, Clone, Copy)]
struct InspectorSystemEnabledButton(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorSystemOrderKind {
    Before,
    After,
}

#[derive(Component, Clone, Copy)]
struct InspectorSystemOrderButton {
    index: usize,
    kind: InspectorSystemOrderKind,
}

#[derive(Component, Clone, Copy)]
struct InspectorSystemDropTarget(usize);

#[derive(Component, Clone, Copy, Default)]
struct InspectorSystemDropName;

#[derive(Component, Clone, Copy)]
struct InspectorOpenSystemScriptButton(usize);

#[derive(Component, Clone, Copy)]
struct InspectorClearSystemScriptButton(usize);

#[derive(Component, Clone)]
struct InspectorSystemScriptOption {
    resource_path: String,
    system_path: String,
}

#[derive(Component, Clone, Copy, Default)]
struct InspectorSystemScriptPickerSearch;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorSystemScriptPickerAction {
    Cancel,
    Confirm,
}

#[derive(Component, Clone, Copy)]
struct InspectorSystemScriptPickerButton(InspectorSystemScriptPickerAction);

#[derive(Resource, Debug, Default)]
struct InspectorExplicitSystemsUiState {
    auto_collapsed: bool,
    explicit_collapsed: bool,
    expanded_binding: Option<usize>,
    revision: u64,
}

#[derive(Resource, Debug, Default)]
struct InspectorSystemScriptPickerState {
    open: bool,
    binding_index: Option<usize>,
    source_filter: Option<String>,
    selected: Option<String>,
    selected_system: Option<String>,
    filter: String,
    revision: u64,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorTabKind {
    #[default]
    Components,
    Systems,
}

impl InspectorTabKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Components => "Components",
            Self::Systems => "Systems",
        }
    }

    pub const fn icon_path(self) -> &'static str {
        match self {
            Self::Components => "editor/icons/box.png",
            Self::Systems => "editor/icons/link.png",
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct InspectorTab(pub InspectorTabKind);

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct InspectorTabChrome(pub InspectorTabKind);

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct InspectorTabCount(pub InspectorTabKind);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorComponentKind {
    Visibility,
    CollisionRect2D,
    Sprite,
    Camera2D,
    Mesh3D,
    Camera3D,
    DirectionalLight,
    PointLight,
    SpotLight,
}

impl InspectorComponentKind {
    const ALL: [Self; 9] = [
        Self::Visibility,
        Self::CollisionRect2D,
        Self::Sprite,
        Self::Camera2D,
        Self::Mesh3D,
        Self::Camera3D,
        Self::DirectionalLight,
        Self::PointLight,
        Self::SpotLight,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Visibility => "Visibility",
            Self::CollisionRect2D => "Collision Rect 2D",
            Self::Sprite => "Sprite Renderer",
            Self::Camera2D => "Camera 2D",
            Self::Mesh3D => "Mesh 3D",
            Self::Camera3D => "Camera 3D",
            Self::DirectionalLight => "Directional Light",
            Self::PointLight => "Point Light",
            Self::SpotLight => "Spot Light",
        }
    }

    const fn icon_path(self) -> &'static str {
        match self {
            Self::Visibility => "editor/icons/eye.png",
            Self::CollisionRect2D => "editor/icons/box.png",
            Self::Sprite => "editor/icons/image.png",
            Self::Camera2D | Self::Camera3D => "editor/icons/camera.png",
            Self::Mesh3D => "editor/icons/box.png",
            Self::DirectionalLight | Self::PointLight | Self::SpotLight => {
                "editor/icons/lightbulb.png"
            }
        }
    }

    const fn builtin(self) -> Option<BuiltinComponent> {
        match self {
            Self::Visibility | Self::CollisionRect2D => None,
            Self::Sprite => Some(BuiltinComponent::Sprite),
            Self::Camera2D => Some(BuiltinComponent::Camera2D),
            Self::Mesh3D => Some(BuiltinComponent::Mesh3D),
            Self::Camera3D => Some(BuiltinComponent::Camera3D),
            Self::DirectionalLight => Some(BuiltinComponent::DirectionalLight3D),
            Self::PointLight => Some(BuiltinComponent::PointLight3D),
            Self::SpotLight => Some(BuiltinComponent::SpotLight3D),
        }
    }
}

#[derive(Component, Clone, Copy)]
struct InspectorComponentSummary(InspectorComponentKind);

#[derive(Component, Clone, Copy)]
struct InspectorComponentStatus(InspectorComponentKind);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorComponentGroupKind {
    Required,
    BuiltIn,
    Custom,
}

#[derive(Component, Clone, Copy)]
struct InspectorComponentGroupToggle(InspectorComponentGroupKind);

#[derive(Component, Clone, Copy)]
struct InspectorComponentGroupChevron(InspectorComponentGroupKind);

#[derive(Component, Clone, Copy)]
struct InspectorComponentGroupCount(InspectorComponentGroupKind);

#[derive(Component, Clone, Copy)]
struct InspectorComponentGroupBody(InspectorComponentGroupKind);

#[derive(Resource, Debug, Default)]
struct InspectorComponentGroupState {
    required_collapsed: bool,
    builtin_collapsed: bool,
    custom_collapsed: bool,
}

#[derive(Component, Clone, Copy)]
struct InspectorRemoveComponentButton(BuiltinComponent);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorSystemKind {
    TransformPropagation,
    VisibilityPropagation,
    SpriteRender,
    Camera2D,
    MeshRender,
    Camera3D,
    LightManagement,
    UiLayout,
    UiRender,
}

impl InspectorSystemKind {
    const ALL: [Self; 9] = [
        Self::TransformPropagation,
        Self::VisibilityPropagation,
        Self::SpriteRender,
        Self::Camera2D,
        Self::MeshRender,
        Self::Camera3D,
        Self::LightManagement,
        Self::UiLayout,
        Self::UiRender,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::TransformPropagation => "Transform Propagation",
            Self::VisibilityPropagation => "Visibility Propagation",
            Self::SpriteRender => "Sprite Render System",
            Self::Camera2D => "Camera 2D System",
            Self::MeshRender => "Mesh Render System",
            Self::Camera3D => "Camera 3D System",
            Self::LightManagement => "Light Management System",
            Self::UiLayout => "UI Layout System",
            Self::UiRender => "UI Render System",
        }
    }

    const fn schedule(self) -> &'static str {
        match self {
            Self::TransformPropagation
            | Self::VisibilityPropagation
            | Self::UiLayout
            | Self::UiRender => "PostUpdate",
            _ => "Update",
        }
    }

    const fn query(self) -> &'static str {
        match self {
            Self::TransformPropagation => "With<Transform>",
            Self::VisibilityPropagation => "With<Visibility>",
            Self::SpriteRender => "With<Sprite>, With<Transform>",
            Self::Camera2D => "With<Camera2d>",
            Self::MeshRender => "With<Mesh3d>, With<Transform>",
            Self::Camera3D => "With<Camera3d>",
            Self::LightManagement => "With<Light>, With<Transform>",
            Self::UiLayout => "With<SceneUiLayout>",
            Self::UiRender => "With<SceneUiContent>",
        }
    }

    const fn reads(self) -> &'static str {
        match self {
            Self::TransformPropagation => "Transform, ChildOf",
            Self::VisibilityPropagation => "Visibility, ChildOf",
            Self::SpriteRender => "Sprite, GlobalTransform",
            Self::Camera2D => "Camera2d, GlobalTransform",
            Self::MeshRender => "Mesh3d, GlobalTransform",
            Self::Camera3D => "Camera3d, GlobalTransform",
            Self::LightManagement => "Light, GlobalTransform",
            Self::UiLayout => "SceneUiLayout, hierarchy",
            Self::UiRender => "SceneUiContent, SceneUiLayout",
        }
    }

    const fn writes(self) -> &'static str {
        match self {
            Self::TransformPropagation => "GlobalTransform",
            Self::VisibilityPropagation => "InheritedVisibility",
            Self::SpriteRender | Self::MeshRender | Self::UiRender => "Render world",
            Self::Camera2D | Self::Camera3D => "Camera view",
            Self::LightManagement => "Light uniforms",
            Self::UiLayout => "UI preview layout",
        }
    }

    const fn icon_path(self) -> &'static str {
        match self {
            Self::TransformPropagation => "editor/icons/move-3d.png",
            Self::VisibilityPropagation => "editor/icons/eye.png",
            Self::SpriteRender | Self::UiRender => "editor/icons/image.png",
            Self::Camera2D | Self::Camera3D => "editor/icons/camera.png",
            Self::MeshRender => "editor/icons/box.png",
            Self::LightManagement => "editor/icons/lightbulb.png",
            Self::UiLayout => "editor/icons/panel-bottom-open.png",
        }
    }
}

/// Explicit metadata for systems that opt into Inspector query matching.
///
/// Bevy systems do not expose their typed Query parameters at runtime, so the
/// editor keeps this small registry beside the systems it documents. Matching
/// is still evaluated against the selected entity's real ECS components.
#[derive(Resource, Debug)]
struct InspectorSystemRegistry {
    registered: Vec<InspectorSystemKind>,
}

impl Default for InspectorSystemRegistry {
    fn default() -> Self {
        Self {
            registered: InspectorSystemKind::ALL.to_vec(),
        }
    }
}

impl InspectorSystemRegistry {
    fn contains(&self, kind: InspectorSystemKind) -> bool {
        self.registered.contains(&kind)
    }

    fn matching_count(&self, features: InspectorEntityFeatures) -> usize {
        self.registered
            .iter()
            .filter(|kind| features.system_matches(**kind))
            .count()
    }
}

#[derive(Component, Clone, Copy)]
struct InspectorSystemCard(InspectorSystemKind);

#[derive(Component, Clone, Copy)]
struct InspectorSystemBody(InspectorSystemKind);

#[derive(Component, Clone, Copy)]
struct InspectorSystemToggle(InspectorSystemKind);

#[derive(Component, Clone, Copy)]
struct InspectorSystemChevron(InspectorSystemKind);

#[derive(Resource, Debug)]
struct InspectorEcsUiState {
    active_tab: InspectorTabKind,
    components_collapsed: bool,
    systems_collapsed: bool,
    expanded_system: Option<InspectorSystemKind>,
}

impl Default for InspectorEcsUiState {
    fn default() -> Self {
        Self {
            active_tab: InspectorTabKind::Components,
            components_collapsed: false,
            systems_collapsed: false,
            expanded_system: Some(InspectorSystemKind::TransformPropagation),
        }
    }
}

#[derive(Component, Clone, Copy, Default)]
struct InspectorAddComponentButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorComponentMenu;

#[derive(Component, Clone, Copy)]
struct InspectorComponentOption(BuiltinComponent);

#[derive(Resource, Debug, Default)]
struct InspectorComponentMenuState {
    open: bool,
}

#[derive(Component, Clone, Copy, Default)]
struct InspectorCustomComponentsList;

#[derive(Component, Clone, Copy, Default)]
struct InspectorCustomComponentDropTarget;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAddCustomComponentButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorCustomComponentMenu;

#[derive(Component, Clone)]
struct InspectorCustomComponentOption(String);

#[derive(Component, Clone, Copy)]
struct InspectorCustomComponentToggle(usize);

#[derive(Component, Clone, Copy)]
struct InspectorCustomComponentReset(usize);

#[derive(Component, Clone, Copy)]
struct InspectorCustomComponentRemove(usize);

#[derive(Component, Clone, Copy)]
struct InspectorCustomFieldInput {
    component: usize,
    field: usize,
}

#[derive(Resource, Debug, Default)]
struct InspectorCustomComponentsUiState {
    menu_open: bool,
    source_filter: Option<String>,
    expanded_component: Option<String>,
    revision: u64,
}

#[derive(Component, Clone, Copy, Default)]
struct InspectorTransformSection;

#[derive(Component, Clone, Copy, Default)]
struct InspectorTransformBody;

#[derive(Component, Clone, Copy)]
struct InspectorTransformSpace(SceneSpace);

#[derive(Component, Clone, Copy, Default)]
struct InspectorTransformToggle;

#[derive(Component, Clone, Copy, Default)]
struct InspectorTransformChevron;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorTransformGroup {
    Position,
    Rotation,
    Scale,
}

#[derive(Component, Clone, Copy)]
struct InspectorTransformReset(InspectorTransformGroup);

#[derive(Component, Clone, Copy)]
struct InspectorTransformKey(InspectorTransformGroup);

#[derive(Component, Clone, Copy, Default)]
struct InspectorScaleLinkButton;

#[derive(Component, Clone, Copy)]
struct InspectorTransformInput {
    field: InspectorField,
    space: SceneSpace,
}

#[derive(Resource, Debug)]
struct InspectorTransformUiState {
    collapsed: bool,
    scale_linked: bool,
}

impl Default for InspectorTransformUiState {
    fn default() -> Self {
        Self {
            collapsed: false,
            scale_linked: true,
        }
    }
}

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiSection;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorUiRowKind {
    Text,
    PanelColor,
    Image,
}

#[derive(Component, Clone, Copy)]
struct InspectorUiRow(InspectorUiRowKind);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorUiField {
    PositionX,
    PositionY,
    Width,
    Height,
    MinimumWidth,
    MinimumHeight,
    Rotation,
    ScaleX,
    ScaleY,
    PivotOffsetX,
    PivotOffsetY,
    PivotRatioX,
    PivotRatioY,
    MarginLeft,
    MarginTop,
    MarginRight,
    MarginBottom,
    ColorR,
    ColorG,
    ColorB,
    ColorA,
}

#[derive(Component, Clone, Copy)]
struct InspectorUiValueLabel(InspectorUiField);

#[derive(Component, Clone, Copy)]
struct InspectorUiNudge {
    field: InspectorUiField,
    delta: f32,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorUiTransformGroup {
    Size,
    Position,
    Rotation,
    Scale,
    PivotOffset,
    PivotRatio,
    MinimumSize,
    Offsets,
}

#[derive(Component, Clone, Copy)]
struct InspectorUiTransformReset(InspectorUiTransformGroup);

#[derive(Component, Clone, Copy)]
struct InspectorUiTransformKey(AnimationTransformProperty);

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiTransformToggle;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiTransformChevron;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiTransformBody;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiScaleLinkButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAnchorDropdownButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAnchorDropdownLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorAnchorDropdownMenu;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiClipButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiClipLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiClipIndicator;

#[derive(Resource, Debug)]
struct InspectorUiLayoutState {
    transform_collapsed: bool,
    anchor_menu_open: bool,
    scale_linked: bool,
}

impl Default for InspectorUiLayoutState {
    fn default() -> Self {
        Self {
            transform_collapsed: false,
            anchor_menu_open: false,
            scale_linked: true,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum AnchorPreset {
    FullRect,
    TopLeft,
    CenterTop,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    CenterBottom,
    BottomRight,
    WideLeft,
    WideTop,
    WideRight,
    WideBottom,
    WideVCenter,
    WideHCenter,
}

impl AnchorPreset {
    const ALL: [Self; 16] = [
        Self::TopLeft,
        Self::CenterTop,
        Self::TopRight,
        Self::CenterLeft,
        Self::Center,
        Self::CenterRight,
        Self::BottomLeft,
        Self::CenterBottom,
        Self::BottomRight,
        Self::WideLeft,
        Self::WideTop,
        Self::WideRight,
        Self::WideBottom,
        Self::WideVCenter,
        Self::WideHCenter,
        Self::FullRect,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::FullRect => "Full Rect",
            Self::TopLeft => "Top Left",
            Self::CenterTop => "Center Top",
            Self::TopRight => "Top Right",
            Self::CenterLeft => "Center Left",
            Self::Center => "Center",
            Self::CenterRight => "Center Right",
            Self::BottomLeft => "Bottom Left",
            Self::CenterBottom => "Center Bottom",
            Self::BottomRight => "Bottom Right",
            Self::WideLeft => "Wide Left",
            Self::WideTop => "Wide Top",
            Self::WideRight => "Wide Right",
            Self::WideBottom => "Wide Bottom",
            Self::WideVCenter => "Wide VCenter",
            Self::WideHCenter => "Wide HCenter",
        }
    }

    const fn anchors(self) -> ((f32, f32), (f32, f32)) {
        match self {
            Self::FullRect => ((0.0, 0.0), (1.0, 1.0)),
            Self::TopLeft => ((0.0, 0.0), (0.0, 0.0)),
            Self::CenterTop => ((0.5, 0.0), (0.5, 0.0)),
            Self::TopRight => ((1.0, 0.0), (1.0, 0.0)),
            Self::CenterLeft => ((0.0, 0.5), (0.0, 0.5)),
            Self::Center => ((0.5, 0.5), (0.5, 0.5)),
            Self::CenterRight => ((1.0, 0.5), (1.0, 0.5)),
            Self::BottomLeft => ((0.0, 1.0), (0.0, 1.0)),
            Self::CenterBottom => ((0.5, 1.0), (0.5, 1.0)),
            Self::BottomRight => ((1.0, 1.0), (1.0, 1.0)),
            Self::WideLeft => ((0.0, 0.0), (0.0, 1.0)),
            Self::WideTop => ((0.0, 0.0), (1.0, 0.0)),
            Self::WideRight => ((1.0, 0.0), (1.0, 1.0)),
            Self::WideBottom => ((0.0, 1.0), (1.0, 1.0)),
            Self::WideVCenter => ((0.0, 0.5), (1.0, 0.5)),
            Self::WideHCenter => ((0.5, 0.0), (0.5, 1.0)),
        }
    }
}

#[derive(Component, Clone, Copy)]
struct InspectorAnchorPreset(AnchorPreset);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum AlignmentAxis {
    Horizontal,
    Vertical,
}

#[derive(Component, Clone, Copy)]
struct InspectorAlignmentButton(AlignmentAxis);

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiTextInput;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiImageInput;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiImageDropTarget;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiImagePreview;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiImageNameLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiImagePathLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorUiImageClearButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteSection;

#[derive(Component, Clone, Copy, Default)]
struct InspectorMesh3dSection;

#[derive(Component, Clone, Copy, Default)]
struct InspectorMesh3dInput;

#[derive(Component, Clone, Copy, Default)]
struct InspectorMesh3dDropTarget;

#[derive(Component, Clone, Copy, Default)]
struct InspectorMesh3dNameLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorMesh3dPathLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorMesh3dClearButton;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteImageInput;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteImageDropTarget;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteImagePreview;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteImageNameLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteImagePathLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteImageClearButton;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorSpriteField {
    HFrames,
    VFrames,
    Frame,
    RegionX,
    RegionY,
    RegionWidth,
    RegionHeight,
    ColorR,
    ColorG,
    ColorB,
    ColorA,
    AnchorX,
    AnchorY,
    ZIndex,
}

#[derive(Component, Clone, Copy)]
struct InspectorSpriteValueLabel(InspectorSpriteField);

#[derive(Component, Clone, Copy)]
struct InspectorSpriteNudge {
    field: InspectorSpriteField,
    delta: f32,
}

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteFrameKey;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorSpriteToggleKind {
    Visible,
    FlipX,
    FlipY,
    Region,
}

#[derive(Component, Clone, Copy)]
struct InspectorSpriteToggle(InspectorSpriteToggleKind);

#[derive(Component, Clone, Copy)]
struct InspectorSpriteToggleLabel(InspectorSpriteToggleKind);

#[derive(Component, Clone, Copy)]
struct InspectorSpriteToggleIndicator(InspectorSpriteToggleKind);

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteRegionBody;

#[derive(Component, Clone, Copy, Default)]
struct InspectorSpriteColorSwatch;

#[derive(Component, Clone, Copy, Default)]
struct InspectorCollisionSection;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorCollisionField {
    Width,
    Height,
    OffsetX,
    OffsetY,
}

#[derive(Component, Clone, Copy)]
struct InspectorCollisionValueLabel(InspectorCollisionField);

#[derive(Component, Clone, Copy)]
struct InspectorCollisionNudge {
    field: InspectorCollisionField,
    delta: f32,
}

#[derive(Component, Clone, Copy, Default)]
struct InspectorCollisionEnabled;

#[derive(Component, Clone, Copy, Default)]
struct InspectorCollisionEnabledLabel;

#[derive(Component, Clone, Copy, Default)]
struct InspectorCollisionEnabledIndicator;

#[derive(Component, Clone, Copy, Default)]
struct InspectorCollisionReset;

#[derive(Resource, Debug, Default)]
struct InspectorUiInputBinding {
    bound: Option<SceneNodeId>,
    suppress_changes: bool,
}

#[derive(Resource, Debug, Default)]
struct InspectorSpriteInputBinding {
    bound: Option<SceneNodeId>,
    suppress_changes: bool,
}

#[derive(Resource, Debug, Default)]
struct InspectorMesh3dInputBinding {
    bound: Option<SceneNodeId>,
    suppress_changes: bool,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum InspectorField {
    PosX,
    PosY,
    PosZ,
    RotX,
    RotY,
    RotZ,
    ScaleX,
    ScaleY,
    ScaleZ,
}

#[derive(Component, Clone, Copy)]
pub struct InspectorNudge {
    pub field: InspectorField,
    pub delta: f32,
}

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectorComponentMenuState>()
            .init_resource::<RustComponentRegistry>()
            .init_resource::<InspectorCustomComponentsUiState>()
            .init_resource::<InspectorComponentGroupState>()
            .init_resource::<InspectorEcsUiState>()
            .init_resource::<InspectorSystemRegistry>()
            .init_resource::<InspectorExplicitSystemsUiState>()
            .init_resource::<InspectorSystemScriptPickerState>()
            .init_resource::<InspectorTransformUiState>()
            .init_resource::<InspectorUiLayoutState>()
            .init_resource::<InspectorUiInputBinding>()
            .init_resource::<InspectorSpriteInputBinding>()
            .init_resource::<InspectorMesh3dInputBinding>()
            .init_resource::<InspectorAnimationUiState>()
            .add_observer(activate_inspector_tab)
            .add_observer(apply_inspector_nudge)
            .add_observer(toggle_ecs_section)
            .add_observer(toggle_system_card)
            .add_observer(toggle_system_group)
            .add_observer(toggle_component_group)
            .add_observer(toggle_explicit_system_card)
            .add_observer(add_explicit_system)
            .add_observer(remove_explicit_system)
            .add_observer(cycle_explicit_system_schedule)
            .add_observer(toggle_explicit_system_enabled)
            .add_observer(cycle_explicit_system_order)
            .add_observer(open_system_script_picker)
            .add_observer(clear_explicit_system_script)
            .add_observer(select_system_script_option)
            .add_observer(handle_system_script_picker_action)
            .add_observer(toggle_transform_section)
            .add_observer(toggle_scale_link)
            .add_observer(reset_transform_group)
            .add_observer(insert_transform_animation_key)
            .add_observer(toggle_ui_transform_section)
            .add_observer(toggle_anchor_dropdown)
            .add_observer(toggle_ui_scale_link)
            .add_observer(reset_ui_transform_group)
            .add_observer(insert_ui_transform_animation_key)
            .add_observer(toggle_ui_clip)
            .add_observer(apply_ui_nudge)
            .add_observer(apply_anchor_preset)
            .add_observer(cycle_ui_alignment)
            .add_observer(toggle_sprite_option)
            .add_observer(apply_sprite_nudge)
            .add_observer(insert_sprite_frame_animation_key)
            .add_observer(toggle_collision_enabled)
            .add_observer(apply_collision_nudge)
            .add_observer(reset_collision_rect)
            .add_observer(handle_component_action)
            .add_observer(remove_component_action)
            .add_observer(toggle_custom_component)
            .add_observer(open_custom_component_menu)
            .add_observer(add_custom_component)
            .add_observer(reset_custom_component)
            .add_observer(remove_custom_component)
            .add_observer(clear_entity_script)
            .add_observer(open_entity_script)
            .add_observer(toggle_entity_script_callback)
            .add_observer(add_animation_clip)
            .add_observer(toggle_animation_autoplay_menu)
            .add_observer(select_animation_autoplay)
            .add_observer(toggle_animation_clip_loop)
            .add_observer(remove_animation_clip)
            .add_systems(
                Update,
                (
                    sync_inspector_section_visibility,
                    sync_ui_input_binding,
                    sync_sprite_input_binding,
                    apply_ui_text_edits,
                    apply_sprite_text_edits,
                    sync_scene_sprite_render,
                    sync_ui_image_preview,
                    sync_sprite_image_preview,
                    sync_inspector_tab_chrome,
                    sync_inspector_labels,
                    sync_transform_inputs,
                    apply_transform_text_edits,
                    sync_ecs_layout,
                    rebuild_explicit_systems,
                    sync_component_ownership,
                    sync_transform_chrome,
                    sync_ui_labels,
                    sync_ui_layout_chrome,
                    sync_sprite_labels,
                    sync_component_menu_visibility,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    sync_animation_panel,
                    sync_animation_autoplay_menu_visibility,
                    apply_animation_text_edits,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    rebuild_custom_components,
                    apply_custom_component_field_edits,
                    rebuild_entity_script_lifecycles,
                    report_entity_script_diagnostics,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    sync_collision_labels,
                    sync_collision_visual.after(sync_scene_sprite_render),
                ),
            )
            .add_systems(
                Update,
                (
                    rebuild_system_script_picker,
                    sync_system_script_picker_search,
                    sync_system_script_picker_options,
                    close_system_script_picker_on_escape,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    sync_mesh3d_input_binding,
                    apply_mesh3d_text_edits,
                    sync_mesh3d_labels,
                )
                    .chain(),
            );
    }
}

pub fn spawn_inspector_body(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            InspectorPanel,
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                min_height: Val::Px(0.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::bg_panel_alt()),
        ))
        .with_children(|body| {
            inspector_entity_header(body, asset_server);
            inspector_filter_bar(body, asset_server);
            body.spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|host| {
                host.spawn((
                    EditorVerticalScrollArea,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(12.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(6.0)),
                        row_gap: Val::Px(6.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|content| {
                    components_section(content, asset_server);
                    animation_section(content);
                    systems_section(content, asset_server);
                });
            });
            inspector_status_bar(body);
        });
}

fn inspector_filter_bar(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(42.0),
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(31.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(9.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ))
            .with_children(|filter| {
                filter.spawn((
                    Text::new("Filter..."),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                    Node {
                        width: Val::Px(0.0),
                        min_width: Val::Px(0.0),
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                filter.spawn((
                    ImageNode::new(asset_server.load("editor/icons/search.png")),
                    Node {
                        width: Val::Px(15.0),
                        height: Val::Px(15.0),
                        ..default()
                    },
                ));
            });
        });
}

fn inspector_status_bar(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(31.0),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(10.0)),
            column_gap: Val::Px(7.0),
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        })
        .with_children(|status| {
            status.spawn((
                Node {
                    width: Val::Px(7.0),
                    height: Val::Px(7.0),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(theme::play()),
            ));
            status.spawn((
                Text::new(".bsn synced · Rust registry valid"),
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
            ));
        });
}

fn inspector_entity_header(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            InspectorEntityHeader,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(104.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
        ))
        .with_children(|header| {
            header
                .spawn(Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(36.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(12.0)),
                    column_gap: Val::Px(8.0),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                })
                .with_children(|title| {
                    title.spawn((
                        Text::new("⌄"),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                        Node {
                            width: Val::Px(10.0),
                            ..default()
                        },
                    ));
                    title.spawn((
                        ImageNode::new(asset_server.load("editor/icons/node-circle.png"))
                            .with_color(theme::accent()),
                        Node {
                            width: Val::Px(16.0),
                            height: Val::Px(16.0),
                            ..default()
                        },
                    ));
                    title.spawn((
                        Text::new("Entity"),
                        TextFont {
                            font_size: FontSize::Px(12.5),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ));
                });
            header
                .spawn(Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(67.0),
                    flex_wrap: FlexWrap::Wrap,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|details| {
                    details
                        .spawn((
                            Node {
                                width: Val::Px(37.0),
                                min_width: Val::Px(37.0),
                                height: Val::Px(37.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.08, 0.27, 0.20)),
                        ))
                        .with_children(|icon| {
                            icon.spawn((
                                InspectorEntityIcon,
                                ImageNode::new(asset_server.load("editor/icons/node-circle.png"))
                                    .with_color(Color::srgb(0.48, 0.88, 0.67)),
                                Node {
                                    width: Val::Px(17.0),
                                    height: Val::Px(17.0),
                                    ..default()
                                },
                            ));
                        });
                    details
                        .spawn(Node {
                            width: Val::Px(0.0),
                            min_width: Val::Px(0.0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(3.0),
                            ..default()
                        })
                        .with_children(|copy| {
                            copy.spawn((
                                InspectorNameLabel,
                                Text::new("Name: (none)"),
                                TextFont {
                                    font_size: FontSize::Px(13.0),
                                    ..default()
                                },
                                TextColor(theme::text_primary()),
                            ));
                            copy.spawn((
                                InspectorEntityIdLabel,
                                Text::new("ID: -"),
                                TextFont {
                                    font_size: FontSize::Px(10.5),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                            ));
                        });
                    details
                        .spawn((
                            InspectorEntityScriptDropTarget,
                            Node {
                                width: Val::Px(128.0),
                                min_width: Val::Px(96.0),
                                min_height: Val::Px(27.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(Val::Px(7.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(theme::bg_field()),
                            BorderColor::all(theme::border_soft()),
                            Pickable::default(),
                        ))
                        .observe(on_entity_script_drag_enter)
                        .observe(on_entity_script_drag_leave)
                        .observe(on_entity_script_drop)
                        .with_children(|target| {
                            target.spawn((
                                Button,
                                WidgetButton,
                                InspectorOpenEntityScriptButton,
                                InspectorEntityScriptName,
                                Text::new("<empty>"),
                                TextFont {
                                    font_size: FontSize::Px(10.0),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                                Node {
                                    width: Val::Px(0.0),
                                    min_width: Val::Px(0.0),
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            target.spawn((
                                Button,
                                WidgetButton,
                                InspectorClearEntityScriptButton,
                                Text::new("x"),
                                TextFont {
                                    font_size: FontSize::Px(10.0),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                                Node {
                                    width: Val::Px(18.0),
                                    height: Val::Px(18.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                            ));
                        });
                    details.spawn((
                        InspectorEntityScriptLifecycleList,
                        Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(3.0),
                            ..default()
                        },
                    ));
                });
        });
}

fn components_section(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            InspectorComponentsSection,
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|section| {
            section
                .spawn((
                    InspectorComponentsBody,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        ..default()
                    },
                ))
                .with_children(|components| {
                    component_group(
                        components,
                        InspectorComponentGroupKind::Required,
                        "Required",
                        "editor/icons/star.png",
                        Color::srgb(0.35, 0.61, 1.0),
                        |body| {
                            transform_section(body, asset_server);
                            component_summary_card(
                                body,
                                InspectorComponentKind::Visibility,
                                asset_server,
                            );
                        },
                        asset_server,
                    );
                    component_group(
                        components,
                        InspectorComponentGroupKind::BuiltIn,
                        "Built-in",
                        "editor/icons/box.png",
                        Color::srgb(0.38, 0.84, 0.75),
                        |body| {
                            for kind in InspectorComponentKind::ALL {
                                if kind != InspectorComponentKind::Visibility {
                                    component_summary_card(body, kind, asset_server);
                                }
                            }
                            mesh3d_section(body);
                            collision_section(body, asset_server);
                            sprite_section(body, asset_server);
                            ui_section(body, asset_server);
                            add_component_button(body);
                            component_menu(body);
                        },
                        asset_server,
                    );
                    component_group(
                        components,
                        InspectorComponentGroupKind::Custom,
                        "Custom",
                        "editor/icons/link.png",
                        Color::srgb(0.82, 0.48, 1.0),
                        |body| {
                            body.spawn((
                                InspectorCustomComponentsList,
                                InspectorCustomComponentDropTarget,
                                Node {
                                    width: Val::Percent(100.0),
                                    min_height: Val::Px(32.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(5.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                                BorderColor::all(Color::NONE),
                                Pickable::default(),
                            ))
                            .observe(on_custom_component_drag_enter)
                            .observe(on_custom_component_drag_leave)
                            .observe(on_custom_component_drop);
                            add_custom_component_button(body);
                            body.spawn((
                                InspectorCustomComponentMenu,
                                Node {
                                    width: Val::Percent(100.0),
                                    display: Display::None,
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect::all(Val::Px(5.0)),
                                    row_gap: Val::Px(2.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::bg_panel()),
                                BorderColor::all(theme::border_soft()),
                            ));
                        },
                        asset_server,
                    );
                });
        });
}

/// Compact AnimationPlayer inspector. The actual clip data stays on the
/// selected entity and is serialized by the scene document; this view only
/// presents the stable target-node/track model for now.
fn animation_section(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            InspectorAnimationSection,
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                padding: UiRect::all(Val::Px(7.0)),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
        ))
        .with_children(|section| {
            section.spawn((
                Text::new("AnimationPlayer"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::text_primary()),
            ));
            section
                .spawn(animation_property_row_node())
                .with_children(|row| {
                    animation_property_label(row, "Autoplay");
                    row.spawn((
                        Button,
                        WidgetButton,
                        InspectorAnimationAutoplayButton,
                        Node {
                            width: Val::Px(0.0),
                            min_width: Val::Px(0.0),
                            flex_grow: 1.0,
                            min_height: Val::Px(27.0),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(Val::Px(8.0)),
                            column_gap: Val::Px(5.0),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(theme::bg_field()),
                        BorderColor::all(theme::border_soft()),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            InspectorAnimationAutoplayLabel,
                            Text::new("None"),
                            TextFont {
                                font_size: FontSize::Px(10.5),
                                ..default()
                            },
                            TextColor(theme::text_primary()),
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                ..default()
                            },
                        ));
                        button.spawn((
                            Text::new("v"),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(theme::text_muted()),
                        ));
                    });
                });
            section.spawn((
                InspectorAnimationAutoplayMenu,
                Node {
                    width: Val::Percent(100.0),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(4.0)),
                    row_gap: Val::Px(2.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_panel_alt()),
                BorderColor::all(theme::border_soft()),
            ));
            section
                .spawn(animation_property_row_node())
                .with_children(|row| {
                    animation_property_label(row, "Speed");
                    animation_editable_field(
                        row,
                        InspectorAnimationSpeedInput,
                        "1.00",
                        Val::Px(0.0),
                    );
                    row.spawn((
                        Text::new("x"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                    ));
                });
            section.spawn((
                Button,
                WidgetButton,
                InspectorAddAnimationButton,
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(27.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
                children![(
                    Text::new("+  Add Animation"),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                )],
            ));
            section.spawn((
                InspectorAnimationClipList,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                },
            ));
        });
}

fn animation_property_row_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        min_height: Val::Px(29.0),
        align_items: AlignItems::Center,
        column_gap: Val::Px(6.0),
        ..default()
    }
}

fn animation_property_label(parent: &mut ChildSpawnerCommands, label: &str) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(theme::text_muted()),
        Node {
            width: Val::Px(66.0),
            min_width: Val::Px(66.0),
            ..default()
        },
    ));
}

fn animation_editable_field<M: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    value: &str,
    width: Val,
) {
    parent.spawn((
        marker,
        EditableText::new(value),
        EditableTextFilter::new(|character| character != '\n' && character != '\r'),
        TextCursorStyle::default(),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(theme::text_primary()),
        Node {
            width,
            min_width: Val::Px(54.0),
            flex_grow: 1.0,
            min_height: Val::Px(27.0),
            padding: UiRect::axes(Val::Px(7.0), Val::Px(4.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(theme::bg_field()),
        BorderColor::all(theme::border_soft()),
    ));
}

fn sync_animation_panel(
    selection: Res<Selection>,
    players: Query<&SceneAnimationPlayer>,
    focus: Option<Res<InputFocus>>,
    animation_inputs: Query<
        (),
        Or<(
            With<InspectorAnimationSpeedInput>,
            With<InspectorAnimationClipNameInput>,
            With<InspectorAnimationClipLengthInput>,
        )>,
    >,
    mut autoplay_labels: Query<&mut Text, With<InspectorAnimationAutoplayLabel>>,
    mut speed_inputs: Query<(Entity, &mut EditableText), With<InspectorAnimationSpeedInput>>,
    autoplay_menus: Query<Entity, With<InspectorAnimationAutoplayMenu>>,
    lists: Query<Entity, With<InspectorAnimationClipList>>,
    mut commands: Commands,
    mut last: Local<Option<(Entity, SceneAnimationPlayer)>>,
) {
    let selected = selection
        .0
        .and_then(|entity| players.get(entity).ok().map(|player| (entity, player)));
    for mut text in &mut autoplay_labels {
        text.0 = selected.as_ref().map_or_else(
            || "None".into(),
            |(_, player)| {
                if player.autoplay.is_empty() {
                    "None".into()
                } else {
                    player.autoplay.clone()
                }
            },
        );
    }
    let focused = focus.as_deref().and_then(InputFocus::get);
    for (entity, mut input) in &mut speed_inputs {
        if focused == Some(entity) {
            continue;
        }
        let value = selected
            .as_ref()
            .map_or(1.0, |(_, player)| player.speed.max(0.0));
        let formatted = format!("{value:.2}");
        if input.value().to_string() != formatted {
            input.editor_mut().set_text(&formatted);
        }
    }
    let Some((entity, player)) = selected else {
        *last = None;
        return;
    };
    let snapshot = (entity, player.clone());
    if last.as_ref() == Some(&snapshot) {
        return;
    }
    if focused.is_some_and(|entity| animation_inputs.get(entity).is_ok()) {
        return;
    }
    *last = Some(snapshot);
    for menu in &autoplay_menus {
        commands.entity(menu).despawn_related::<Children>();
        commands.entity(menu).with_children(|menu| {
            animation_autoplay_option(menu, None, "None", player.autoplay.is_empty());
            for (index, clip) in player.clips.iter().enumerate() {
                animation_autoplay_option(
                    menu,
                    Some(index),
                    &clip.name,
                    player.autoplay == clip.name,
                );
            }
        });
    }
    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|list| {
            for (index, clip) in player.clips.iter().enumerate() {
                let keys: usize = clip.tracks.iter().map(|track| track.keys.len()).sum();
                list.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(6.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        row_gap: Val::Px(5.0),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::border_soft()),
                ))
                .with_children(|card| {
                    card.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(6.0),
                            ..default()
                        },
                        children![
                            (
                                Text::new(format!("Animation {}", index + 1)),
                                TextFont {
                                    font_size: FontSize::Px(10.5),
                                    ..default()
                                },
                                TextColor(theme::text_primary()),
                                Node {
                                    flex_grow: 1.0,
                                    min_width: Val::Px(0.0),
                                    ..default()
                                },
                            ),
                            (
                                Button,
                                WidgetButton,
                                InspectorRemoveAnimationButton(index),
                                Node {
                                    min_height: Val::Px(23.0),
                                    padding: UiRect::horizontal(Val::Px(7.0)),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::bg_panel()),
                                BorderColor::all(theme::border_soft()),
                                children![(
                                    Text::new("Remove"),
                                    TextFont {
                                        font_size: FontSize::Px(9.5),
                                        ..default()
                                    },
                                    TextColor(theme::warning()),
                                )],
                            )
                        ],
                    ));
                    card.spawn(animation_property_row_node())
                        .with_children(|row| {
                            animation_property_label(row, "Name");
                            animation_editable_field(
                                row,
                                InspectorAnimationClipNameInput(index),
                                &clip.name,
                                Val::Px(0.0),
                            );
                        });
                    card.spawn(animation_property_row_node())
                        .with_children(|row| {
                            animation_property_label(row, "Length");
                            animation_editable_field(
                                row,
                                InspectorAnimationClipLengthInput(index),
                                &format!("{:.2}", clip.length.max(0.01)),
                                Val::Px(0.0),
                            );
                            row.spawn((
                                Text::new("s"),
                                TextFont {
                                    font_size: FontSize::Px(10.0),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                            ));
                        });
                    card.spawn(animation_property_row_node())
                        .with_children(|row| {
                            animation_property_label(row, "Loop");
                            row.spawn((
                                Button,
                                WidgetButton,
                                InspectorAnimationClipLoopButton(index),
                                Node {
                                    width: Val::Px(0.0),
                                    min_width: Val::Px(0.0),
                                    flex_grow: 1.0,
                                    min_height: Val::Px(27.0),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(Val::Px(8.0)),
                                    column_gap: Val::Px(7.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::bg_panel()),
                                BorderColor::all(if clip.looped {
                                    theme::accent()
                                } else {
                                    theme::border_soft()
                                }),
                            ))
                            .with_children(|button| {
                                button.spawn((
                                    Node {
                                        width: Val::Px(12.0),
                                        height: Val::Px(12.0),
                                        border: UiRect::all(Val::Px(1.0)),
                                        border_radius: BorderRadius::all(Val::Px(2.0)),
                                        ..default()
                                    },
                                    BackgroundColor(if clip.looped {
                                        theme::accent()
                                    } else {
                                        Color::NONE
                                    }),
                                    BorderColor::all(if clip.looped {
                                        theme::accent()
                                    } else {
                                        theme::border()
                                    }),
                                ));
                                button.spawn((
                                    Text::new(if clip.looped { "On" } else { "Off" }),
                                    TextFont {
                                        font_size: FontSize::Px(10.5),
                                        ..default()
                                    },
                                    TextColor(if clip.looped {
                                        theme::text_primary()
                                    } else {
                                        theme::text_muted()
                                    }),
                                ));
                            });
                        });
                    card.spawn((
                        Text::new(format!("{} tracks / {} keys", clip.tracks.len(), keys)),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                    ));
                });
            }
        });
        if player.clips.is_empty() {
            commands.entity(list).with_child((
                Text::new("No animations. Click Add Animation."),
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
            ));
        }
    }
}

fn animation_autoplay_option(
    parent: &mut ChildSpawnerCommands,
    index: Option<usize>,
    label: &str,
    selected: bool,
) {
    parent.spawn((
        Button,
        WidgetButton,
        InspectorAnimationAutoplayOption(index),
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(25.0),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(7.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(if selected {
            theme::bg_selected()
        } else {
            theme::bg_field()
        }),
        children![(
            Text::new(label.to_owned()),
            TextFont {
                font_size: FontSize::Px(10.5),
                ..default()
            },
            TextColor(if selected {
                theme::text_primary()
            } else {
                theme::text_muted()
            }),
        )],
    ));
}

fn sync_animation_autoplay_menu_visibility(
    selection: Res<Selection>,
    mut state: ResMut<InspectorAnimationUiState>,
    mut menus: Query<&mut Node, With<InspectorAnimationAutoplayMenu>>,
) {
    if selection.is_changed() {
        state.autoplay_open = false;
    }
    for mut node in &mut menus {
        node.display = if state.autoplay_open {
            Display::Flex
        } else {
            Display::None
        };
    }
}

enum AnimationTextEdit {
    Speed(f32),
    Name { index: usize, value: String },
    Length { index: usize, value: f32 },
}

fn apply_animation_text_edits(
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    speed_inputs: Query<&EditableText, (With<InspectorAnimationSpeedInput>, Changed<EditableText>)>,
    name_inputs: Query<(&InspectorAnimationClipNameInput, &EditableText), Changed<EditableText>>,
    length_inputs: Query<
        (&InspectorAnimationClipLengthInput, &EditableText),
        Changed<EditableText>,
    >,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<&SceneAnimationPlayer>,
        Query<&mut SceneAnimationPlayer>,
    )>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Some(entity) = selection.0 else { return };
    let players = nodes.p1();
    let Ok(player) = players.get(entity) else {
        return;
    };
    let mut edits = Vec::new();

    if let Some(speed) = speed_inputs
        .iter()
        .next()
        .and_then(|input| input.value().to_string().trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 100.0))
        && (player.speed - speed).abs() > 0.0001
    {
        edits.push(AnimationTextEdit::Speed(speed));
    }
    for (input, text) in &name_inputs {
        let value = text.value().to_string();
        let value = value.trim();
        let Some(clip) = player.clips.get(input.0) else {
            continue;
        };
        if value.is_empty()
            || value == clip.name
            || player
                .clips
                .iter()
                .enumerate()
                .any(|(index, clip)| index != input.0 && clip.name == value)
        {
            continue;
        }
        edits.push(AnimationTextEdit::Name {
            index: input.0,
            value: value.to_owned(),
        });
    }
    for (input, text) in &length_inputs {
        let Some(clip) = player.clips.get(input.0) else {
            continue;
        };
        let Some(value) = text
            .value()
            .to_string()
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
        else {
            continue;
        };
        let last_key = clip
            .tracks
            .iter()
            .flat_map(|track| &track.keys)
            .map(|key| key.time.max(0.0))
            .fold(0.0_f32, f32::max);
        let value = value.clamp(0.01, 3600.0).max(last_key);
        if (clip.length - value).abs() > 0.0001 {
            edits.push(AnimationTextEdit::Length {
                index: input.0,
                value,
            });
        }
    }
    if edits.is_empty() {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Edit AnimationPlayer",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p2();
    let Ok(mut player) = players.get_mut(entity) else {
        return;
    };
    for edit in edits {
        match edit {
            AnimationTextEdit::Speed(value) => player.speed = value,
            AnimationTextEdit::Name { index, value } => {
                let Some(clip) = player.clips.get_mut(index) else {
                    continue;
                };
                let old_name = std::mem::replace(&mut clip.name, value.clone());
                if player.autoplay == old_name {
                    player.autoplay = value;
                }
            }
            AnimationTextEdit::Length { index, value } => {
                if let Some(clip) = player.clips.get_mut(index) {
                    clip.length = value;
                }
            }
        }
    }
    mark_document_changed(document.as_deref_mut());
}

fn component_group(
    parent: &mut ChildSpawnerCommands,
    kind: InspectorComponentGroupKind,
    label: &str,
    icon_path: &'static str,
    icon_color: Color,
    build_body: impl FnOnce(&mut ChildSpawnerCommands),
    asset_server: &AssetServer,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
        ))
        .with_children(|group| {
            group
                .spawn((
                    Button,
                    WidgetButton,
                    InspectorComponentGroupToggle(kind),
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(34.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(9.0)),
                        column_gap: Val::Px(7.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_toolbar()),
                    BorderColor::all(theme::border_soft()),
                ))
                .with_children(|header| {
                    header.spawn((
                        InspectorComponentGroupChevron(kind),
                        Text::new("v"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                        Node {
                            width: Val::Px(10.0),
                            ..default()
                        },
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        ImageNode::new(asset_server.load(icon_path)).with_color(icon_color),
                        Node {
                            width: Val::Px(15.0),
                            height: Val::Px(15.0),
                            ..default()
                        },
                    ));
                    header.spawn((
                        Text::new(label),
                        TextFont {
                            font_size: FontSize::Px(11.5),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        InspectorComponentGroupCount(kind),
                        Text::new("0"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                    ));
                });
            group
                .spawn((
                    InspectorComponentGroupBody(kind),
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(5.0)),
                        row_gap: Val::Px(5.0),
                        ..default()
                    },
                ))
                .with_children(build_body);
        });
}

fn add_custom_component_button(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Button,
        WidgetButton,
        InspectorAddCustomComponentButton,
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(30.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(theme::bg_field()),
        BorderColor::all(theme::border_soft()),
        children![(
            Text::new("+  Add Component"),
            TextFont {
                font_size: FontSize::Px(11.5),
                ..default()
            },
            TextColor(theme::text_primary()),
        )],
    ));
}

fn rebuild_custom_components(
    mut commands: Commands,
    asset_server: Option<Res<AssetServer>>,
    selection: Res<Selection>,
    registry: Res<RustComponentRegistry>,
    attached: Query<&EntityCustomComponents>,
    lists: Query<Entity, With<InspectorCustomComponentsList>>,
    menus: Query<Entity, With<InspectorCustomComponentMenu>>,
    mut menu_nodes: Query<&mut Node, With<InspectorCustomComponentMenu>>,
    state: Res<InspectorCustomComponentsUiState>,
    mut last_selection: Local<Option<Entity>>,
    mut last_registry_revision: Local<u64>,
    mut last_ui_revision: Local<u64>,
) {
    let selection_changed = *last_selection != selection.0;
    if !selection_changed
        && *last_registry_revision == registry.revision
        && *last_ui_revision == state.revision
    {
        return;
    }
    let Ok(list) = lists.single() else { return };
    let Ok(menu) = menus.single() else { return };
    let components = selection
        .0
        .and_then(|entity| attached.get(entity).ok())
        .map(|components| components.0.as_slice())
        .unwrap_or_default();

    commands.entity(list).despawn_related::<Children>();
    commands.entity(list).with_children(|list| {
        if components.is_empty() {
            list.spawn((
                Text::new("No custom Rust components attached."),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::text_muted()),
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(7.0)),
                    ..default()
                },
            ));
        } else {
            for (index, component) in components.iter().enumerate() {
                let definition = registry.get(&component.type_path);
                custom_component_card(
                    list,
                    index,
                    component,
                    definition,
                    state.expanded_component.as_deref() == Some(component.type_path.as_str()),
                    asset_server.as_deref(),
                );
            }
        }
    });

    commands.entity(menu).despawn_related::<Children>();
    commands.entity(menu).with_children(|menu| {
        let mut available = 0;
        for definition in &registry.components {
            if state
                .source_filter
                .as_deref()
                .is_some_and(|source| definition.source_path != source)
            {
                continue;
            }
            if components
                .iter()
                .any(|component| component.type_path == definition.type_path)
            {
                continue;
            }
            available += 1;
            menu.spawn((
                Button,
                WidgetButton,
                InspectorCustomComponentOption(definition.type_path.clone()),
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(29.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    column_gap: Val::Px(7.0),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
            ))
            .with_children(|option| {
                option.spawn((
                    Text::new("{}"),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.84, 0.55, 1.0)),
                ));
                option.spawn((
                    Text::new(definition.name.clone()),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                ));
                option.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                option.spawn((
                    Text::new("Rust"),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.59, 0.34)),
                ));
            });
        }
        if available == 0 {
            menu.spawn((
                Text::new(if registry.components.is_empty() {
                    "No #[derive(Component)] types found in src/."
                } else {
                    "All discovered components are attached."
                }),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
                Node {
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                },
            ));
        }
    });
    if let Ok(mut node) = menu_nodes.get_mut(menu) {
        node.display = if state.menu_open && selection.0.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }

    *last_selection = selection.0;
    *last_registry_revision = registry.revision;
    *last_ui_revision = state.revision;
}

fn custom_component_card(
    parent: &mut ChildSpawnerCommands,
    index: usize,
    component: &SceneCustomComponent,
    definition: Option<&RustComponentDefinition>,
    expanded: bool,
    asset_server: Option<&AssetServer>,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|card| {
            card.spawn((
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(33.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(7.0)),
                    column_gap: Val::Px(5.0),
                    ..default()
                },
                BackgroundColor(theme::bg_toolbar()),
            ))
            .with_children(|header| {
                header
                    .spawn((
                        Button,
                        WidgetButton,
                        InspectorCustomComponentToggle(index),
                        Node {
                            width: Val::Px(0.0),
                            min_width: Val::Px(0.0),
                            flex_grow: 1.0,
                            height: Val::Percent(100.0),
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(6.0),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new(if expanded { "v" } else { ">" }),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(theme::text_muted()),
                            Node {
                                width: Val::Px(9.0),
                                ..default()
                            },
                        ));
                        if let Some(asset_server) = asset_server {
                            button.spawn((
                                ImageNode::new(asset_server.load("editor/icons/link.png"))
                                    .with_color(Color::srgb(0.82, 0.48, 1.0)),
                                Node {
                                    width: Val::Px(14.0),
                                    height: Val::Px(14.0),
                                    ..default()
                                },
                            ));
                        }
                        button.spawn((
                            Text::new(component.display_name().to_owned()),
                            TextFont {
                                font_size: FontSize::Px(10.5),
                                ..default()
                            },
                            TextColor(theme::text_primary()),
                        ));
                    });
                header
                    .spawn((
                        Button,
                        WidgetButton,
                        InspectorCustomComponentReset(index),
                        Node {
                            width: Val::Px(24.0),
                            min_width: Val::Px(24.0),
                            height: Val::Px(24.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|button| {
                        if let Some(asset_server) = asset_server {
                            button.spawn((
                                ImageNode::new(asset_server.load("editor/icons/undo-2.png"))
                                    .with_color(theme::text_muted()),
                                Node {
                                    width: Val::Px(13.0),
                                    height: Val::Px(13.0),
                                    ..default()
                                },
                            ));
                        } else {
                            button.spawn((
                                Text::new("R"),
                                TextFont {
                                    font_size: FontSize::Px(9.0),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                            ));
                        }
                    });
                header.spawn((
                    Text::new("Rust"),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.59, 0.34)),
                    Node {
                        min_width: Val::Px(28.0),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    },
                ));
            });
            card.spawn(Node {
                width: Val::Percent(100.0),
                display: if expanded {
                    Display::Flex
                } else {
                    Display::None
                },
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(7.0)),
                row_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|body| {
                system_detail_row(body, "Source", &component.source_path);
                system_detail_row(
                    body,
                    "Runtime",
                    if definition.is_some_and(|definition| definition.reflection_ready) {
                        "Reflection ready"
                    } else {
                        "Saved metadata"
                    },
                );
                if component.fields.is_empty() {
                    body.spawn((
                        Text::new("Marker component (no fields)"),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                    ));
                }
                for (field_index, field) in component.fields.iter().enumerate() {
                    let field_definition = definition.and_then(|definition| {
                        definition
                            .fields
                            .iter()
                            .find(|candidate| candidate.name == field.name)
                    });
                    custom_component_field(
                        body,
                        index,
                        field_index,
                        &field.name,
                        &field.type_name,
                        &field.value,
                        field_definition.is_some_and(|field| field.editable),
                        asset_server,
                    );
                }
                body.spawn((
                    Button,
                    WidgetButton,
                    InspectorCustomComponentRemove(index),
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(29.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::border_soft()),
                    children![(
                        Text::new("Remove"),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(Color::srgb(0.94, 0.46, 0.50)),
                    )],
                ));
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn custom_component_field(
    parent: &mut ChildSpawnerCommands,
    component: usize,
    field: usize,
    name: &str,
    type_name: &str,
    value: &str,
    editable: bool,
    asset_server: Option<&AssetServer>,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(29.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(7.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(name.to_owned()),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
                Node {
                    width: Val::Px(74.0),
                    min_width: Val::Px(56.0),
                    ..default()
                },
            ));
            if editable {
                row.spawn((
                    InspectorCustomFieldInput { component, field },
                    EditableText::new(value),
                    EditableTextFilter::new(|character| character != '\n' && character != '\r'),
                    TextCursorStyle::default(),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                    Node {
                        width: Val::Px(0.0),
                        min_width: Val::Px(0.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(27.0),
                        padding: UiRect::axes(Val::Px(7.0), Val::Px(4.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::border_soft()),
                ));
            } else {
                row.spawn((
                    Text::new(type_name.to_owned()),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                    Node {
                        width: Val::Px(0.0),
                        min_width: Val::Px(0.0),
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                if let Some(asset_server) = asset_server {
                    row.spawn((
                        ImageNode::new(asset_server.load("editor/icons/lock.png"))
                            .with_color(theme::text_muted()),
                        Node {
                            width: Val::Px(13.0),
                            height: Val::Px(13.0),
                            ..default()
                        },
                    ));
                }
            }
        });
}

fn toggle_custom_component(
    activate: On<Activate>,
    toggles: Query<&InspectorCustomComponentToggle>,
    selection: Res<Selection>,
    attached: Query<&EntityCustomComponents>,
    mut state: ResMut<InspectorCustomComponentsUiState>,
) {
    let Ok(toggle) = toggles.get(activate.entity) else {
        return;
    };
    let Some(type_path) = selection
        .0
        .and_then(|entity| attached.get(entity).ok())
        .and_then(|components| components.0.get(toggle.0))
        .map(|component| component.type_path.clone())
    else {
        return;
    };
    state.expanded_component = if state.expanded_component.as_deref() == Some(type_path.as_str()) {
        None
    } else {
        Some(type_path)
    };
    state.revision = state.revision.wrapping_add(1);
}

fn open_custom_component_menu(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorAddCustomComponentButton>>,
    selection: Res<Selection>,
    mut state: ResMut<InspectorCustomComponentsUiState>,
) {
    if buttons.get(activate.entity).is_err() || selection.0.is_none() {
        return;
    }
    state.menu_open = !state.menu_open;
    state.source_filter = None;
    state.revision = state.revision.wrapping_add(1);
}

fn add_custom_component(
    activate: On<Activate>,
    options: Query<&InspectorCustomComponentOption>,
    registry: Res<RustComponentRegistry>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<&mut EntityCustomComponents>,
    )>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorCustomComponentsUiState>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(option) = options.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let Some(definition) = registry.get(&option.0) else {
        return;
    };
    let can_add = {
        let mut components = nodes.p1();
        components.get_mut(entity).is_ok_and(|components| {
            !components
                .0
                .iter()
                .any(|component| component.type_path == option.0)
        })
    };
    if !can_add {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            format!("Add {}", definition.name),
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut components = nodes.p1();
    let Ok(mut components) = components.get_mut(entity) else {
        return;
    };
    components.0.push(definition.instantiate());
    state.expanded_component = Some(option.0.clone());
    state.menu_open = false;
    state.source_filter = None;
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn reset_custom_component(
    activate: On<Activate>,
    buttons: Query<&InspectorCustomComponentReset>,
    registry: Res<RustComponentRegistry>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<&mut EntityCustomComponents>,
    )>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorCustomComponentsUiState>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let replacement = {
        let mut components = nodes.p1();
        let Ok(components) = components.get_mut(entity) else {
            return;
        };
        let Some(component) = components.0.get(button.0) else {
            return;
        };
        registry
            .get(&component.type_path)
            .map(|definition| definition.instantiate())
    };
    let Some(replacement) = replacement else {
        return;
    };
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            format!("Reset {}", replacement.display_name()),
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut components = nodes.p1();
    let Ok(mut components) = components.get_mut(entity) else {
        return;
    };
    let Some(component) = components.0.get_mut(button.0) else {
        return;
    };
    if *component == replacement {
        return;
    }
    *component = replacement;
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn remove_custom_component(
    activate: On<Activate>,
    buttons: Query<&InspectorCustomComponentRemove>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<&mut EntityCustomComponents>,
    )>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorCustomComponentsUiState>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let name = {
        let mut components = nodes.p1();
        let Ok(components) = components.get_mut(entity) else {
            return;
        };
        let Some(component) = components.0.get(button.0) else {
            return;
        };
        component.display_name().to_owned()
    };
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            format!("Remove {name}"),
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut components = nodes.p1();
    let Ok(mut components) = components.get_mut(entity) else {
        return;
    };
    if button.0 >= components.0.len() {
        return;
    }
    components.0.remove(button.0);
    state.expanded_component = None;
    state.menu_open = false;
    state.source_filter = None;
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn apply_custom_component_field_edits(
    inputs: Query<(&InspectorCustomFieldInput, &EditableText), Changed<EditableText>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<&mut EntityCustomComponents>,
    )>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Some(entity) = selection.0 else { return };
    for (input, text) in &inputs {
        let value = text.value().to_string();
        let changed = {
            let mut components = nodes.p1();
            components.get_mut(entity).ok().is_some_and(|components| {
                components
                    .0
                    .get(input.component)
                    .and_then(|component| component.fields.get(input.field))
                    .is_some_and(|field| field.value != value)
            })
        };
        if !changed {
            continue;
        }
        if let Some(history) = history.as_deref_mut() {
            let history_nodes = nodes.p0();
            history.begin(
                "Edit Custom Component",
                capture_scene_snapshot(&history_nodes, &selection, *mode),
            );
        }
        let mut components = nodes.p1();
        let Ok(mut components) = components.get_mut(entity) else {
            continue;
        };
        let Some(field) = components
            .0
            .get_mut(input.component)
            .and_then(|component| component.fields.get_mut(input.field))
        else {
            continue;
        };
        field.value = value;
        mark_document_changed(document.as_deref_mut());
    }
}

fn systems_section(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            InspectorSystemsSection,
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                border_radius: BorderRadius::ZERO,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|section| {
            section
                .spawn((
                    InspectorSystemsBody,
                    Node {
                        width: Val::Percent(100.0),
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::ZERO,
                        row_gap: Val::Px(6.0),
                        ..default()
                    },
                ))
                .with_children(|systems| {
                    system_group(
                        systems,
                        InspectorSystemGroupKind::AutoMatch,
                        "Auto Match",
                        "editor/icons/search.png",
                        Color::srgb(0.36, 0.68, 1.0),
                        |body| {
                            body.spawn((
                                InspectorNoMatchingSystems,
                                Text::new("No systems currently match this entity."),
                                TextFont {
                                    font_size: FontSize::Px(10.0),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                                Node {
                                    width: Val::Percent(100.0),
                                    display: Display::None,
                                    padding: UiRect::all(Val::Px(8.0)),
                                    ..default()
                                },
                            ));
                            for kind in InspectorSystemKind::ALL {
                                system_card(body, kind, asset_server);
                            }
                        },
                        asset_server,
                    );
                    system_group(
                        systems,
                        InspectorSystemGroupKind::ExplicitBindings,
                        "Explicit Bindings",
                        "editor/icons/link.png",
                        Color::srgb(0.94, 0.64, 0.38),
                        |body| {
                            body.spawn((
                                InspectorExplicitSystemsList,
                                Node {
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(5.0),
                                    ..default()
                                },
                            ));
                            body.spawn((
                                Button,
                                WidgetButton,
                                InspectorAddSystemButton,
                                Node {
                                    width: Val::Percent(100.0),
                                    min_height: Val::Px(31.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::bg_field()),
                                BorderColor::all(theme::border_soft()),
                                children![(
                                    Text::new("+  Add System"),
                                    TextFont {
                                        font_size: FontSize::Px(10.5),
                                        ..default()
                                    },
                                    TextColor(theme::text_primary()),
                                )],
                            ));
                        },
                        asset_server,
                    );
                });
        });
}

fn system_group(
    parent: &mut ChildSpawnerCommands,
    kind: InspectorSystemGroupKind,
    label: &str,
    icon_path: &'static str,
    icon_color: Color,
    build_body: impl FnOnce(&mut ChildSpawnerCommands),
    asset_server: &AssetServer,
) {
    parent
        .spawn((
            InspectorSystemGroup,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
        ))
        .with_children(|group| {
            group
                .spawn((
                    Button,
                    WidgetButton,
                    InspectorSystemGroupToggle(kind),
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(34.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(9.0)),
                        column_gap: Val::Px(7.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_toolbar()),
                    BorderColor::all(theme::border_soft()),
                ))
                .with_children(|header| {
                    header.spawn((
                        InspectorSystemGroupChevron(kind),
                        Text::new("v"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                        Node {
                            width: Val::Px(10.0),
                            ..default()
                        },
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        ImageNode::new(asset_server.load(icon_path)).with_color(icon_color),
                        Node {
                            width: Val::Px(15.0),
                            height: Val::Px(15.0),
                            ..default()
                        },
                    ));
                    header.spawn((
                        Text::new(label),
                        TextFont {
                            font_size: FontSize::Px(11.5),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        InspectorSystemGroupCount(kind),
                        Text::new("0"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                    ));
                });
            group
                .spawn((
                    InspectorSystemGroupBody(kind),
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(5.0)),
                        row_gap: Val::Px(5.0),
                        ..default()
                    },
                ))
                .with_children(build_body);
        });
}

fn component_summary_card(
    parent: &mut ChildSpawnerCommands,
    kind: InspectorComponentKind,
    asset_server: &AssetServer,
) {
    parent
        .spawn((
            InspectorComponentSummary(kind),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(34.0),
                display: Display::None,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                column_gap: Val::Px(7.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::bg_field()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|card| {
            card.spawn((
                ImageNode::new(asset_server.load(kind.icon_path())).with_color(theme::accent()),
                Node {
                    width: Val::Px(15.0),
                    height: Val::Px(15.0),
                    ..default()
                },
            ));
            card.spawn((
                Text::new(kind.label()),
                TextFont {
                    font_size: FontSize::Px(11.5),
                    ..default()
                },
                TextColor(theme::text_primary()),
            ));
            card.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            });
            card.spawn((
                InspectorComponentStatus(kind),
                Text::new("Preset"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
            ));
            if let Some(component) = kind.builtin() {
                card.spawn((
                    Button,
                    WidgetButton,
                    InspectorRemoveComponentButton(component),
                    Node {
                        min_width: Val::Px(48.0),
                        min_height: Val::Px(22.0),
                        display: Display::None,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        padding: UiRect::horizontal(Val::Px(5.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(theme::border_soft()),
                    children![(
                        Text::new("Remove"),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.94, 0.46, 0.50)),
                    )],
                ));
            }
        });
}

fn mesh3d_section(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            InspectorMesh3dSection,
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                padding: UiRect::all(Val::Px(7.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|mesh| {
            section(mesh, "Mesh3D");
            ui_input_label(mesh, "Mesh / Scene");
            mesh.spawn((
                InspectorMesh3dDropTarget,
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(58.0),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                    row_gap: Val::Px(3.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
                Pickable::default(),
            ))
            .observe(on_mesh3d_drag_enter)
            .observe(on_mesh3d_drag_leave)
            .observe(on_mesh3d_drop)
            .with_children(|target| {
                target.spawn((
                    InspectorMesh3dNameLabel,
                    Text::new("Default Cube"),
                    TextFont {
                        font_size: FontSize::Px(11.5),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                    Pickable::IGNORE,
                ));
                target.spawn((
                    InspectorMesh3dPathLabel,
                    Text::new("Drag a GLB, GLTF, or FBX from FileSystem"),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                    Pickable::IGNORE,
                ));
            });
            ui_text_input(mesh, InspectorMesh3dInput, "res://");
            mesh.spawn((
                Button,
                WidgetButton,
                InspectorMesh3dClearButton,
                Node {
                    width: Val::Px(58.0),
                    min_height: Val::Px(23.0),
                    display: Display::None,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(theme::border_soft()),
                children![(
                    Text::new("Clear"),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(Color::srgb(0.94, 0.46, 0.50)),
                )],
            ))
            .observe(clear_mesh3d_resource);
        });
}

fn system_card(
    parent: &mut ChildSpawnerCommands,
    kind: InspectorSystemKind,
    asset_server: &AssetServer,
) {
    parent
        .spawn((
            InspectorSystemCard(kind),
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::bg_field()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|card| {
            card.spawn((
                Button,
                WidgetButton,
                InspectorSystemToggle(kind),
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(36.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(7.0)),
                    column_gap: Val::Px(6.0),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|header| {
                header.spawn((
                    InspectorSystemChevron(kind),
                    Text::new(">"),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                    Node {
                        width: Val::Px(9.0),
                        ..default()
                    },
                ));
                header.spawn((
                    ImageNode::new(asset_server.load(kind.icon_path()))
                        .with_color(Color::srgb(0.42, 0.82, 0.55)),
                    Node {
                        width: Val::Px(15.0),
                        height: Val::Px(15.0),
                        ..default()
                    },
                ));
                header.spawn((
                    Text::new(kind.label()),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                    Node {
                        width: Val::Px(0.0),
                        min_width: Val::Px(0.0),
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                header.spawn((
                    Text::new("Rust"),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.94, 0.69, 0.39)),
                ));
            });
            card.spawn((
                InspectorSystemBody(kind),
                Node {
                    width: Val::Percent(100.0),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(Val::Px(9.0), Val::Px(7.0)),
                    row_gap: Val::Px(5.0),
                    border: UiRect::top(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_panel()),
                BorderColor::all(theme::border_soft()),
            ))
            .with_children(|details| {
                system_script_resource_row(details, kind.label(), true);
                system_detail_row(details, "Schedule", kind.schedule());
                system_status_row(details);
                system_detail_row(details, "Query", kind.query());
                system_detail_row(details, "Read", kind.reads());
                system_detail_row(details, "Write", kind.writes());
            });
        });
}

fn system_script_resource_row(parent: &mut ChildSpawnerCommands, display_name: &str, locked: bool) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(29.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("Script"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
                Node {
                    width: Val::Px(52.0),
                    min_width: Val::Px(52.0),
                    ..default()
                },
            ));
            row.spawn((
                Text::new(if locked {
                    format!("{display_name}  [locked]")
                } else {
                    display_name.to_owned()
                }),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(25.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(7.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ));
        });
}

fn system_status_row(parent: &mut ChildSpawnerCommands) {
    system_detail_row(parent, "Status", "●  Running");
}

fn system_detail_row(parent: &mut ChildSpawnerCommands, label: &str, value: &str) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Start,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
                Node {
                    width: Val::Px(52.0),
                    min_width: Val::Px(52.0),
                    ..default()
                },
            ));
            row.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}

fn explicit_system_card(
    parent: &mut ChildSpawnerCommands,
    index: usize,
    binding: &SceneSystemBinding,
    expanded: bool,
    asset_server: Option<&AssetServer>,
    registry: Option<&RustComponentRegistry>,
) {
    let definition = registry.and_then(|registry| registry.get_system(&binding.system_path));
    let name = definition
        .map(|definition| definition.name.clone())
        .unwrap_or_else(|| binding.display_name().to_owned());
    let path = if binding.script_path.trim().is_empty() {
        "<empty>".to_owned()
    } else {
        binding.script_path.clone()
    };
    parent
        .spawn((
            InspectorExplicitSystemCard,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::bg_field()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|card| {
            card.spawn((
                Button,
                WidgetButton,
                InspectorExplicitSystemToggle(index),
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(36.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(7.0)),
                    column_gap: Val::Px(6.0),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|header| {
                header.spawn((
                    Text::new(if expanded { "v" } else { ">" }),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                    Node {
                        width: Val::Px(9.0),
                        ..default()
                    },
                ));
                header.spawn((
                    Text::new("↪"),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.94, 0.64, 0.38)),
                ));
                header.spawn((
                    Text::new(name),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                    Node {
                        min_width: Val::Px(0.0),
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                header.spawn((
                    Text::new("Rust"),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.94, 0.69, 0.39)),
                ));
            });
            card.spawn((
                InspectorExplicitSystemBody,
                Node {
                    width: Val::Percent(100.0),
                    display: if expanded {
                        Display::Flex
                    } else {
                        Display::None
                    },
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(Val::Px(9.0), Val::Px(7.0)),
                    row_gap: Val::Px(5.0),
                    border: UiRect::top(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_panel()),
                BorderColor::all(theme::border_soft()),
            ))
            .with_children(|body| {
                body.spawn(Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(29.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new("Script"),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                        Node {
                            width: Val::Px(52.0),
                            min_width: Val::Px(52.0),
                            ..default()
                        },
                    ));
                    row.spawn((
                        Node {
                            width: Val::Px(0.0),
                            min_width: Val::Px(0.0),
                            flex_grow: 1.0,
                            height: Val::Px(27.0),
                            align_items: AlignItems::Center,
                            overflow: Overflow::clip(),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(theme::bg_field()),
                        BorderColor::all(theme::border_soft()),
                    ))
                    .with_children(|actions| {
                        actions
                            .spawn((
                                Button,
                                WidgetButton,
                                InspectorOpenSystemScriptButton(index),
                                InspectorSystemDropTarget(index),
                                Node {
                                    width: Val::Px(0.0),
                                    min_width: Val::Px(0.0),
                                    flex_grow: 1.0,
                                    height: Val::Percent(100.0),
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(6.0),
                                    padding: UiRect::horizontal(Val::Px(7.0)),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                                BorderColor::all(Color::NONE),
                                Pickable::default(),
                            ))
                            .observe(on_system_script_drag_enter)
                            .observe(on_system_script_drag_leave)
                            .observe(on_system_script_drop)
                            .with_children(|button| {
                                button.spawn((
                                    Text::new("{}"),
                                    TextFont {
                                        font_size: FontSize::Px(9.0),
                                        ..default()
                                    },
                                    TextColor(theme::accent()),
                                ));
                                button.spawn((
                                    InspectorSystemDropName,
                                    Text::new(system_script_file_name(&path)),
                                    TextFont {
                                        font_size: FontSize::Px(9.5),
                                        ..default()
                                    },
                                    TextColor(theme::text_primary()),
                                    Node {
                                        width: Val::Px(0.0),
                                        min_width: Val::Px(0.0),
                                        flex_grow: 1.0,
                                        ..default()
                                    },
                                ));
                                button.spawn((
                                    Text::new("v"),
                                    TextFont {
                                        font_size: FontSize::Px(8.5),
                                        ..default()
                                    },
                                    TextColor(theme::text_muted()),
                                ));
                            });
                        if !binding.script_path.trim().is_empty() {
                            actions
                                .spawn((
                                    Button,
                                    WidgetButton,
                                    InspectorClearSystemScriptButton(index),
                                    Node {
                                        width: Val::Px(26.0),
                                        min_width: Val::Px(26.0),
                                        height: Val::Percent(100.0),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        border: UiRect::left(Val::Px(1.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                    BorderColor::all(theme::border_soft()),
                                ))
                                .with_children(|button| {
                                    if let Some(asset_server) = asset_server {
                                        button.spawn((
                                            ImageNode::new(
                                                asset_server.load("editor/icons/undo-2.png"),
                                            )
                                            .with_color(theme::text_muted()),
                                            Node {
                                                width: Val::Px(14.0),
                                                height: Val::Px(14.0),
                                                ..default()
                                            },
                                        ));
                                    } else {
                                        button.spawn((
                                            Text::new("R"),
                                            TextFont {
                                                font_size: FontSize::Px(9.0),
                                                ..default()
                                            },
                                            TextColor(theme::text_muted()),
                                        ));
                                    }
                                });
                        }
                    });
                });
                system_detail_row(
                    body,
                    "Status",
                    if !binding.enabled {
                        "Disabled"
                    } else if binding.script_path.trim().is_empty() {
                        "Unassigned"
                    } else {
                        "Bound"
                    },
                );
                body.spawn((
                    Button,
                    WidgetButton,
                    InspectorSystemEnabledButton(index),
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(29.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(7.0)),
                        column_gap: Val::Px(6.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::border_soft()),
                    children![
                        (
                            Text::new("Enabled"),
                            TextFont {
                                font_size: FontSize::Px(9.5),
                                ..default()
                            },
                            TextColor(theme::text_muted())
                        ),
                        (Node {
                            flex_grow: 1.0,
                            ..default()
                        }),
                        (
                            Text::new(if binding.enabled { "On" } else { "Off" }),
                            TextFont {
                                font_size: FontSize::Px(9.5),
                                ..default()
                            },
                            TextColor(if binding.enabled {
                                theme::accent()
                            } else {
                                theme::text_muted()
                            })
                        )
                    ],
                ));
                body.spawn((
                    Button,
                    WidgetButton,
                    InspectorSystemScheduleButton(index),
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(29.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(7.0)),
                        column_gap: Val::Px(6.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::border_soft()),
                    children![
                        (
                            Text::new("Schedule"),
                            TextFont {
                                font_size: FontSize::Px(9.5),
                                ..default()
                            },
                            TextColor(theme::text_muted())
                        ),
                        (Node {
                            flex_grow: 1.0,
                            ..default()
                        }),
                        (
                            Text::new(binding.schedule.label()),
                            TextFont {
                                font_size: FontSize::Px(9.5),
                                ..default()
                            },
                            TextColor(theme::text_primary())
                        ),
                        (
                            Text::new("v"),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(theme::text_muted())
                        )
                    ],
                ));
                explicit_system_order_row(
                    body,
                    index,
                    InspectorSystemOrderKind::Before,
                    binding.before.first().map(String::as_str),
                );
                explicit_system_order_row(
                    body,
                    index,
                    InspectorSystemOrderKind::After,
                    binding.after.first().map(String::as_str),
                );
                system_detail_row(
                    body,
                    "Query",
                    &definition
                        .map_or_else(|| "Unknown".into(), RustSystemDefinition::query_summary),
                );
                system_detail_row(
                    body,
                    "Access",
                    &definition
                        .map_or_else(|| "Unknown".into(), RustSystemDefinition::access_summary),
                );
                body.spawn((
                    Button,
                    WidgetButton,
                    InspectorRemoveSystemButton(index),
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(29.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::border_soft()),
                    children![(
                        Text::new("Remove"),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(Color::srgb(0.94, 0.46, 0.50)),
                    )],
                ));
            });
        });
}

fn explicit_system_order_row(
    parent: &mut ChildSpawnerCommands,
    index: usize,
    kind: InspectorSystemOrderKind,
    target: Option<&str>,
) {
    let label = match kind {
        InspectorSystemOrderKind::Before => "Before",
        InspectorSystemOrderKind::After => "After",
    };
    let value = target
        .and_then(|target| target.rsplit("::").next())
        .filter(|target| !target.is_empty())
        .unwrap_or("None");
    parent.spawn((
        Button,
        WidgetButton,
        InspectorSystemOrderButton { index, kind },
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(29.0),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(7.0)),
            column_gap: Val::Px(6.0),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(theme::bg_field()),
        BorderColor::all(theme::border_soft()),
        children![
            (
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_muted())
            ),
            (Node {
                flex_grow: 1.0,
                ..default()
            }),
            (
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_primary())
            ),
            (
                Text::new("v"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::text_muted())
            )
        ],
    ));
}

fn rebuild_explicit_systems(
    mut commands: Commands,
    asset_server: Option<Res<AssetServer>>,
    selection: Res<Selection>,
    bindings: Query<&EntitySystemBindings>,
    registry: Option<Res<RustComponentRegistry>>,
    lists: Query<Entity, With<InspectorExplicitSystemsList>>,
    mut state: ResMut<InspectorExplicitSystemsUiState>,
    mut last_selection: Local<Option<Entity>>,
    mut last_revision: Local<u64>,
) {
    let selection_changed = *last_selection != selection.0;
    if !selection_changed && *last_revision == state.revision {
        return;
    }
    let Ok(list) = lists.single() else {
        return;
    };
    if selection_changed {
        state.expanded_binding = None;
    }
    commands.entity(list).despawn_related::<Children>();
    if let Some(entity) = selection.0
        && let Ok(bindings) = bindings.get(entity)
    {
        commands.entity(list).with_children(|list| {
            for (index, binding) in bindings.0.iter().enumerate() {
                explicit_system_card(
                    list,
                    index,
                    binding,
                    state.expanded_binding == Some(index),
                    asset_server.as_deref(),
                    registry.as_deref(),
                );
            }
        });
    }
    *last_selection = selection.0;
    *last_revision = state.revision;
}

fn system_script_file_name(resource_path: &str) -> String {
    if resource_path == "<empty>" {
        return resource_path.to_owned();
    }
    resource_path
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(resource_path)
        .to_owned()
}

fn open_system_script_picker(
    activate: On<Activate>,
    buttons: Query<&InspectorOpenSystemScriptButton>,
    selection: Res<Selection>,
    bindings: Query<&EntitySystemBindings>,
    mut picker: ResMut<InspectorSystemScriptPickerState>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let Ok(bindings) = bindings.get(entity) else {
        return;
    };
    let Some(binding) = bindings.0.get(button.0) else {
        return;
    };
    picker.open = true;
    picker.binding_index = Some(button.0);
    picker.source_filter = None;
    picker.selected = (!binding.script_path.trim().is_empty()).then(|| binding.script_path.clone());
    picker.selected_system =
        (!binding.system_path.trim().is_empty()).then(|| binding.system_path.clone());
    picker.filter.clear();
    picker.revision = picker.revision.wrapping_add(1);
}

fn clear_explicit_system_script(
    activate: On<Activate>,
    buttons: Query<&InspectorClearSystemScriptButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntitySystemBindings>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut system_ui: ResMut<InspectorExplicitSystemsUiState>,
    mut picker: ResMut<InspectorSystemScriptPickerState>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let can_clear = {
        let mut bindings = nodes.p1();
        bindings.get_mut(entity).ok().is_some_and(|bindings| {
            bindings
                .0
                .get(button.0)
                .is_some_and(|binding| !binding.script_path.trim().is_empty())
        })
    };
    if !can_clear {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Clear System Script",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut bindings = nodes.p1();
    let Ok(mut bindings) = bindings.get_mut(entity) else {
        return;
    };
    let Some(binding) = bindings.0.get_mut(button.0) else {
        return;
    };
    binding.script_path.clear();
    binding.system_path.clear();
    picker.open = false;
    picker.binding_index = None;
    picker.source_filter = None;
    picker.selected = None;
    picker.selected_system = None;
    picker.revision = picker.revision.wrapping_add(1);
    system_ui.revision = system_ui.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn select_system_script_option(
    activate: On<Activate>,
    options: Query<&InspectorSystemScriptOption>,
    mut picker: ResMut<InspectorSystemScriptPickerState>,
) {
    let Ok(option) = options.get(activate.entity) else {
        return;
    };
    picker.selected = Some(option.resource_path.clone());
    picker.selected_system = Some(option.system_path.clone());
}

fn handle_system_script_picker_action(
    activate: On<Activate>,
    buttons: Query<&InspectorSystemScriptPickerButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntitySystemBindings>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut picker: ResMut<InspectorSystemScriptPickerState>,
    mut system_ui: ResMut<InspectorExplicitSystemsUiState>,
    mut filesystem: Option<ResMut<FileSystemState>>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    if button.0 == InspectorSystemScriptPickerAction::Cancel {
        picker.open = false;
        picker.binding_index = None;
        picker.source_filter = None;
        picker.selected = None;
        picker.selected_system = None;
        picker.revision = picker.revision.wrapping_add(1);
        return;
    }
    let (Some(entity), Some(index), Some(resource_path)) =
        (selection.0, picker.binding_index, picker.selected.clone())
    else {
        return;
    };
    let should_attach = {
        let mut bindings = nodes.p1();
        bindings.get_mut(entity).ok().is_some_and(|bindings| {
            bindings.0.get(index).is_some_and(|binding| {
                binding.script_path != resource_path
                    || binding.system_path != picker.selected_system.as_deref().unwrap_or_default()
            })
        })
    };
    if should_attach {
        if let Some(history) = history.as_deref_mut() {
            let history_nodes = nodes.p0();
            history.begin(
                "Attach System Script",
                capture_scene_snapshot(&history_nodes, &selection, *mode),
            );
        }
        let mut bindings = nodes.p1();
        let Ok(mut bindings) = bindings.get_mut(entity) else {
            return;
        };
        let Some(binding) = bindings.0.get_mut(index) else {
            return;
        };
        binding.script_path = resource_path.clone();
        binding.system_path = picker.selected_system.clone().unwrap_or_default();
        system_ui.revision = system_ui.revision.wrapping_add(1);
        if let Some(filesystem) = filesystem.as_deref_mut() {
            filesystem.status = format!("Attached {resource_path}");
            filesystem.revision = filesystem.revision.wrapping_add(1);
        }
        mark_document_changed(document.as_deref_mut());
    }
    picker.open = false;
    picker.binding_index = None;
    picker.source_filter = None;
    picker.selected = None;
    picker.selected_system = None;
    picker.revision = picker.revision.wrapping_add(1);
}

fn close_system_script_picker_on_escape(
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mut picker: ResMut<InspectorSystemScriptPickerState>,
) {
    if picker.open
        && keyboard
            .as_deref()
            .is_some_and(|keyboard| keyboard.just_pressed(KeyCode::Escape))
    {
        picker.open = false;
        picker.binding_index = None;
        picker.source_filter = None;
        picker.selected = None;
        picker.selected_system = None;
        picker.revision = picker.revision.wrapping_add(1);
    }
}

fn sync_system_script_picker_search(
    inputs: Query<
        &EditableText,
        (
            With<InspectorSystemScriptPickerSearch>,
            Changed<EditableText>,
        ),
    >,
    mut picker: ResMut<InspectorSystemScriptPickerState>,
) {
    for input in &inputs {
        let value = input.value().to_string();
        if picker.filter != value {
            picker.filter = value;
        }
    }
}

fn sync_system_script_picker_options(
    picker: Res<InspectorSystemScriptPickerState>,
    mut options: Query<(
        &InspectorSystemScriptOption,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    if !picker.is_changed() {
        return;
    }
    let filter = picker.filter.trim().to_ascii_lowercase();
    for (option, mut node, mut background, mut border) in &mut options {
        let visible = filter.is_empty()
            || option.resource_path.to_ascii_lowercase().contains(&filter)
            || option.system_path.to_ascii_lowercase().contains(&filter);
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        let selected = picker.selected_system.as_deref() == Some(option.system_path.as_str());
        background.0 = if selected {
            theme::bg_selected()
        } else {
            theme::bg_field()
        };
        *border = BorderColor::all(if selected {
            theme::accent()
        } else {
            Color::NONE
        });
    }
}

fn rebuild_system_script_picker(
    mut commands: Commands,
    picker: Res<InspectorSystemScriptPickerState>,
    filesystem: Option<Res<FileSystemState>>,
    registry: Option<Res<RustComponentRegistry>>,
    asset_server: Option<Res<AssetServer>>,
    hosts: Query<Entity, With<SystemScriptPickerHost>>,
    mut rendered_revision: Local<u64>,
) {
    if picker.revision == *rendered_revision {
        return;
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    *rendered_revision = picker.revision;
    commands.entity(host).despawn_related::<Children>();
    commands.entity(host).insert((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            display: if picker.open {
                Display::Flex
            } else {
                Display::None
            },
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(if picker.open {
            Color::srgba(0.02, 0.025, 0.035, 0.76)
        } else {
            Color::NONE
        }),
        Pickable::default(),
    ));
    if !picker.open {
        return;
    }

    let mut scripts: Vec<(String, String, String)> = registry
        .as_deref()
        .map(|registry| {
            registry
                .systems
                .iter()
                .filter(|system| {
                    picker
                        .source_filter
                        .as_deref()
                        .is_none_or(|source| system.source_path == source)
                })
                .map(|system| {
                    (
                        system.name.clone(),
                        system.source_path.clone(),
                        system.system_path.clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if scripts.is_empty() {
        scripts = filesystem
            .as_deref()
            .into_iter()
            .flat_map(|filesystem| filesystem.entries.iter())
            .filter(|entry| !entry.is_dir)
            .filter_map(|entry| {
                rust_script_resource_path_from_filesystem(&entry.relative)
                    .ok()
                    .map(|resource| (system_script_file_name(&resource), resource, String::new()))
            })
            .collect();
    }
    scripts.sort_by(|left, right| {
        left.0
            .to_ascii_lowercase()
            .cmp(&right.0.to_ascii_lowercase())
    });

    commands.entity(host).with_children(|root| {
        root.spawn((
            Node {
                width: Val::Px(560.0),
                max_width: Val::Percent(90.0),
                height: Val::Px(520.0),
                max_height: Val::Percent(86.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.125, 0.14, 0.165)),
            BorderColor::all(theme::border()),
            Pickable::default(),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(42.0),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_toolbar()),
                    BorderColor::all(theme::border_soft()),
                ))
                .with_children(|title| {
                    if let Some(asset_server) = asset_server.as_deref() {
                        title.spawn((
                            ImageNode::new(asset_server.load("editor/icons/folder-open.png"))
                                .with_color(theme::accent()),
                            Node {
                                width: Val::Px(16.0),
                                height: Val::Px(16.0),
                                ..default()
                            },
                        ));
                    }
                    title.spawn((
                        Text::new("Select System"),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ));
                    title.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    picker_dialog_button(
                        title,
                        InspectorSystemScriptPickerAction::Cancel,
                        "x",
                        false,
                    );
                });

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
                    ..default()
                })
                .with_children(|search| {
                    search.spawn((
                        InspectorSystemScriptPickerSearch,
                        EditableText::new(&picker.filter),
                        EditableTextFilter::new(|character| character != '\n' && character != '\r'),
                        TextCursorStyle::default(),
                        TextFont {
                            font_size: FontSize::Px(11.5),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(35.0),
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(theme::bg_field()),
                        BorderColor::all(theme::border_soft()),
                    ));
                });

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(0.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    padding: UiRect::horizontal(Val::Px(12.0)),
                    row_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|list| {
                    if scripts.is_empty() {
                        list.spawn((
                            Text::new("No Rust ECS systems found in res://src."),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(theme::text_muted()),
                            Node {
                                width: Val::Percent(100.0),
                                padding: UiRect::all(Val::Px(12.0)),
                                ..default()
                            },
                        ));
                    }
                    for (name, relative, system_path) in &scripts {
                        let selected = picker.selected_system.as_deref()
                            == Some(system_path.as_str())
                            || (system_path.is_empty()
                                && picker.selected.as_deref() == Some(relative.as_str()));
                        list.spawn((
                            Button,
                            WidgetButton,
                            InspectorSystemScriptOption {
                                resource_path: relative.clone(),
                                system_path: system_path.clone(),
                            },
                            Node {
                                width: Val::Percent(100.0),
                                min_height: Val::Px(48.0),
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(9.0),
                                padding: UiRect::horizontal(Val::Px(9.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(if selected {
                                theme::bg_selected()
                            } else {
                                theme::bg_field()
                            }),
                            BorderColor::all(if selected {
                                theme::accent()
                            } else {
                                Color::NONE
                            }),
                        ))
                        .with_children(|row| {
                            row.spawn((
                                Text::new("{}"),
                                TextFont {
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(theme::accent()),
                                Node {
                                    width: Val::Px(25.0),
                                    ..default()
                                },
                            ));
                            row.spawn(Node {
                                width: Val::Px(0.0),
                                min_width: Val::Px(0.0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(2.0),
                                ..default()
                            })
                            .with_children(|copy| {
                                copy.spawn((
                                    Text::new(name.clone()),
                                    TextFont {
                                        font_size: FontSize::Px(11.5),
                                        ..default()
                                    },
                                    TextColor(theme::text_primary()),
                                ));
                                copy.spawn((
                                    Text::new(relative.clone()),
                                    TextFont {
                                        font_size: FontSize::Px(9.5),
                                        ..default()
                                    },
                                    TextColor(theme::text_muted()),
                                ));
                            });
                            row.spawn((
                                Text::new(">"),
                                TextFont {
                                    font_size: FontSize::Px(10.0),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                            ));
                        });
                    }
                });

            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(52.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: Val::Px(8.0),
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        border: UiRect::top(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_toolbar()),
                    BorderColor::all(theme::border_soft()),
                ))
                .with_children(|footer| {
                    picker_dialog_button(
                        footer,
                        InspectorSystemScriptPickerAction::Cancel,
                        "Cancel",
                        false,
                    );
                    picker_dialog_button(
                        footer,
                        InspectorSystemScriptPickerAction::Confirm,
                        "Attach System",
                        true,
                    );
                });
        });
    });
}

fn picker_dialog_button(
    parent: &mut ChildSpawnerCommands,
    action: InspectorSystemScriptPickerAction,
    label: &str,
    primary: bool,
) {
    parent.spawn((
        Button,
        WidgetButton,
        InspectorSystemScriptPickerButton(action),
        Node {
            min_width: if label == "x" {
                Val::Px(28.0)
            } else {
                Val::Px(86.0)
            },
            height: Val::Px(32.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(if primary {
            theme::accent()
        } else {
            theme::bg_field()
        }),
        BorderColor::all(theme::border_soft()),
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(10.5),
                ..default()
            },
            TextColor(theme::text_primary()),
        )],
    ));
}

fn transform_section(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            InspectorTransformSection,
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|section| {
            section
                .spawn((
                    Button,
                    WidgetButton,
                    InspectorTransformToggle,
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(35.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(8.0)),
                        column_gap: Val::Px(7.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_toolbar()),
                    BorderColor::all(theme::border_soft()),
                ))
                .with_children(|header| {
                    header.spawn((
                        InspectorTransformChevron,
                        Text::new("v"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                        Node {
                            width: Val::Px(10.0),
                            ..default()
                        },
                    ));
                    header.spawn((
                        ImageNode::new(asset_server.load("editor/icons/move-3d.png"))
                            .with_color(theme::accent()),
                        Node {
                            width: Val::Px(16.0),
                            height: Val::Px(16.0),
                            ..default()
                        },
                    ));
                    header.spawn((
                        Text::new("Transform"),
                        TextFont {
                            font_size: FontSize::Px(12.5),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        ..default()
                    });
                    header
                        .spawn((
                            Node {
                                min_height: Val::Px(22.0),
                                padding: UiRect::horizontal(Val::Px(8.0)),
                                align_items: AlignItems::Center,
                                border_radius: BorderRadius::all(Val::Px(11.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.12, 0.22, 0.28)),
                        ))
                        .with_children(|chip| {
                            chip.spawn((
                                Text::new("Required"),
                                TextFont {
                                    font_size: FontSize::Px(10.0),
                                    ..default()
                                },
                                TextColor(theme::accent()),
                            ));
                        });
                    header.spawn((
                        ImageNode::new(asset_server.load("editor/icons/lock.png"))
                            .with_color(theme::text_muted()),
                        Node {
                            width: Val::Px(14.0),
                            height: Val::Px(14.0),
                            ..default()
                        },
                    ));
                });

            section
                .spawn((
                    InspectorTransformBody,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(7.0)),
                        row_gap: Val::Px(6.0),
                        ..default()
                    },
                ))
                .with_children(|body| {
                    body.spawn((
                        InspectorTransformSpace(SceneSpace::TwoD),
                        Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(6.0),
                            ..default()
                        },
                    ))
                    .with_children(|two_d| {
                        transform_property_group(
                            two_d,
                            SceneSpace::TwoD,
                            "Position",
                            &[
                                (InspectorField::PosX, Some("X"), " px"),
                                (InspectorField::PosY, Some("Y"), " px"),
                            ],
                            InspectorTransformGroup::Position,
                            false,
                            asset_server,
                        );
                        transform_property_group(
                            two_d,
                            SceneSpace::TwoD,
                            "Rotation",
                            &[(InspectorField::RotZ, None, "°")],
                            InspectorTransformGroup::Rotation,
                            false,
                            asset_server,
                        );
                        transform_property_group(
                            two_d,
                            SceneSpace::TwoD,
                            "Scale",
                            &[
                                (InspectorField::ScaleX, Some("X"), ""),
                                (InspectorField::ScaleY, Some("Y"), ""),
                            ],
                            InspectorTransformGroup::Scale,
                            true,
                            asset_server,
                        );
                    });

                    body.spawn((
                        InspectorTransformSpace(SceneSpace::ThreeD),
                        Node {
                            width: Val::Percent(100.0),
                            display: Display::None,
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(6.0),
                            ..default()
                        },
                    ))
                    .with_children(|three_d| {
                        transform_property_group(
                            three_d,
                            SceneSpace::ThreeD,
                            "Position",
                            &[
                                (InspectorField::PosX, Some("X"), ""),
                                (InspectorField::PosY, Some("Y"), ""),
                                (InspectorField::PosZ, Some("Z"), ""),
                            ],
                            InspectorTransformGroup::Position,
                            false,
                            asset_server,
                        );
                        transform_property_group(
                            three_d,
                            SceneSpace::ThreeD,
                            "Rotation",
                            &[
                                (InspectorField::RotX, Some("X"), "°"),
                                (InspectorField::RotY, Some("Y"), "°"),
                                (InspectorField::RotZ, Some("Z"), "°"),
                            ],
                            InspectorTransformGroup::Rotation,
                            false,
                            asset_server,
                        );
                        transform_property_group(
                            three_d,
                            SceneSpace::ThreeD,
                            "Scale",
                            &[
                                (InspectorField::ScaleX, Some("X"), ""),
                                (InspectorField::ScaleY, Some("Y"), ""),
                                (InspectorField::ScaleZ, Some("Z"), ""),
                            ],
                            InspectorTransformGroup::Scale,
                            true,
                            asset_server,
                        );
                    });
                });
        });
}

fn transform_property_group(
    parent: &mut ChildSpawnerCommands,
    space: SceneSpace,
    title: &str,
    inputs: &[(InspectorField, Option<&str>, &str)],
    group: InspectorTransformGroup,
    show_scale_link: bool,
    asset_server: &AssetServer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(42.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(64.0),
                    min_width: Val::Px(64.0),
                    ..default()
                },
                children![(
                    Text::new(title),
                    TextFont {
                        font_size: FontSize::Px(11.5),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                )],
            ));
            row.spawn((
                Node {
                    width: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    column_gap: Val::Px(3.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ))
            .with_children(|field| {
                field
                    .spawn(Node {
                        width: Val::Px(0.0),
                        min_width: Val::Px(0.0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(1.0),
                        ..default()
                    })
                    .with_children(|values| {
                        for (field, axis, suffix) in inputs.iter().copied() {
                            transform_input_line(values, space, field, axis, suffix);
                        }
                    });
                if show_scale_link {
                    transform_icon_button(
                        field,
                        InspectorScaleLinkButton,
                        "editor/icons/link.png",
                        asset_server,
                        "Lock scale proportions",
                    );
                }
                transform_icon_button(
                    field,
                    InspectorTransformKey(group),
                    "editor/icons/key-round.png",
                    asset_server,
                    "Insert key at the current animation time",
                );
                transform_icon_button(
                    field,
                    InspectorTransformReset(group),
                    "editor/icons/undo-2.png",
                    asset_server,
                    "Reset property",
                );
            });
        });
}

fn transform_input_line(
    parent: &mut ChildSpawnerCommands,
    space: SceneSpace,
    field: InspectorField,
    axis: Option<&str>,
    suffix: &str,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(22.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|line| {
            if let Some(axis) = axis {
                line.spawn((
                    Text::new(axis),
                    TextFont {
                        font_size: FontSize::Px(11.5),
                        ..default()
                    },
                    TextColor(transform_axis_color(axis)),
                    Node {
                        width: Val::Px(17.0),
                        min_width: Val::Px(17.0),
                        ..default()
                    },
                ));
            }
            line.spawn((
                InspectorTransformInput { field, space },
                EditableText::new("0.0"),
                EditableTextFilter::new(|character| {
                    character.is_ascii_digit() || matches!(character, '.' | '-' | '+' | 'e' | 'E')
                }),
                TextCursorStyle::default(),
                TextFont {
                    font_size: FontSize::Px(11.5),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: Val::Px(52.0),
                    min_width: Val::Px(52.0),
                    height: Val::Px(22.0),
                    align_items: AlignItems::Center,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            if !suffix.is_empty() {
                line.spawn((
                    Text::new(suffix),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                ));
            }
        });
}

fn transform_icon_button<M: Component + Clone>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    icon_path: &'static str,
    asset_server: &AssetServer,
    _tooltip: &str,
) {
    parent
        .spawn((
            Button,
            WidgetButton,
            marker,
            Node {
                width: Val::Px(23.0),
                min_width: Val::Px(23.0),
                height: Val::Px(23.0),
                padding: UiRect::all(Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
        ))
        .with_children(|button| {
            button.spawn((
                ImageNode::new(asset_server.load(icon_path)).with_color(theme::text_muted()),
                Node {
                    width: Val::Px(14.0),
                    height: Val::Px(14.0),
                    ..default()
                },
            ));
        });
}

fn transform_axis_color(axis: &str) -> Color {
    match axis {
        "X" => Color::srgb(0.92, 0.30, 0.38),
        "Y" => Color::srgb(0.45, 0.78, 0.25),
        "Z" => Color::srgb(0.28, 0.58, 1.0),
        _ => theme::text_muted(),
    }
}

fn add_component_button(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Button,
            WidgetButton,
            InspectorAddComponentButton,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(30.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::bg_field()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new("+  Add Component"),
                TextFont {
                    font_size: FontSize::Px(11.5),
                    ..default()
                },
                TextColor(theme::text_primary()),
            ));
        });
}

fn component_menu(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            InspectorComponentMenu,
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(5.0)),
                row_gap: Val::Px(2.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|menu| {
            for component in BuiltinComponent::ALL {
                menu.spawn((
                    Button,
                    WidgetButton,
                    InspectorComponentOption(component),
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(27.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(8.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    children![(
                        Text::new(component.label()),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    )],
                ));
            }
        });
}

#[allow(clippy::type_complexity)]
fn handle_component_action(
    activate: On<Activate>,
    add_buttons: Query<(), With<InspectorAddComponentButton>>,
    options: Query<&InspectorComponentOption>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut menu: ResMut<InspectorComponentMenuState>,
    mut history: Option<ResMut<SceneHistory>>,
    mut component_queries: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut AddedEntityComponents>)>,
    details: Query<(
        &SceneSpace,
        Has<Transform>,
        Has<Sprite>,
        Has<Camera2d>,
        Has<Mesh3d>,
        Has<SceneModel3D>,
        Has<Camera3d>,
        Has<DirectionalLight>,
        Has<PointLight>,
        Has<SpotLight>,
    )>,
    mut document: Option<ResMut<SceneDocument>>,
    mut commands: Commands,
) {
    if add_buttons.get(activate.entity).is_ok() {
        menu.open = selection.0.is_some() && !menu.open;
        return;
    }
    let Ok(option) = options.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else {
        menu.open = false;
        return;
    };
    let Ok((space, transform, sprite, camera_2d, mesh, model, camera_3d, directional, point, spot)) =
        details.get(entity)
    else {
        return;
    };
    let already_authored = {
        let mut added_components = component_queries.p1();
        added_components
            .get_mut(entity)
            .is_ok_and(|components| components.0.contains(&option.0))
    };
    if !option.0.supports(*space)
        || already_authored
        || component_is_present(
            option.0,
            transform,
            sprite,
            camera_2d,
            mesh || model,
            camera_3d,
            directional,
            point,
            spot,
        )
    {
        return;
    }

    if let Some(history) = history.as_deref_mut() {
        let history_nodes = component_queries.p0();
        history.begin(
            format!("Add {}", option.0.label()),
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut added_components = component_queries.p1();
    if let Ok(mut components) = added_components.get_mut(entity) {
        if !components.0.contains(&option.0) {
            components.0.push(option.0);
        }
    } else {
        commands
            .entity(entity)
            .insert(AddedEntityComponents(vec![option.0]));
    }
    if !transform {
        commands
            .entity(entity)
            .insert((Transform::default(), Visibility::Visible));
    }
    insert_builtin_component(&mut commands, entity, option.0);
    if let Some(document) = document.as_deref_mut() {
        document.open = true;
        document.dirty = true;
        document.bump_revision();
    }
    menu.open = false;
}

fn remove_component_action(
    activate: On<Activate>,
    buttons: Query<&InspectorRemoveComponentButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    kinds: Query<&EntityKind>,
    mut menu: ResMut<InspectorComponentMenuState>,
    mut history: Option<ResMut<SceneHistory>>,
    mut component_queries: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut AddedEntityComponents>)>,
    mut document: Option<ResMut<SceneDocument>>,
    mut commands: Commands,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    if button.0 == BuiltinComponent::Transform {
        return;
    }
    let Some(entity) = selection.0 else { return };
    let Ok(kind) = kinds.get(entity).copied() else {
        return;
    };
    let authored = {
        let mut components = component_queries.p1();
        components
            .get_mut(entity)
            .is_ok_and(|components| components.0.contains(&button.0))
    };
    if !authored {
        return;
    }

    if let Some(history) = history.as_deref_mut() {
        let history_nodes = component_queries.p0();
        history.begin(
            format!("Remove {}", button.0.label()),
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }

    let no_authored_components_remain = {
        let mut components = component_queries.p1();
        let Ok(mut components) = components.get_mut(entity) else {
            return;
        };
        components.0.retain(|component| *component != button.0);
        components.0.is_empty()
    };
    remove_builtin_component(&mut commands, entity, button.0);
    if no_authored_components_remain && !kind.is_spatial() {
        commands
            .entity(entity)
            .remove::<Transform>()
            .remove::<Visibility>();
    }
    mark_document_changed(document.as_deref_mut());
    menu.open = false;
}

#[allow(clippy::too_many_arguments)]
fn component_is_present(
    component: BuiltinComponent,
    transform: bool,
    sprite: bool,
    camera_2d: bool,
    mesh: bool,
    camera_3d: bool,
    directional: bool,
    point: bool,
    spot: bool,
) -> bool {
    match component {
        BuiltinComponent::Transform => transform,
        BuiltinComponent::Sprite => sprite,
        BuiltinComponent::Camera2D => camera_2d,
        BuiltinComponent::Mesh3D => mesh,
        BuiltinComponent::Camera3D => camera_3d,
        BuiltinComponent::DirectionalLight3D => directional,
        BuiltinComponent::PointLight3D => point,
        BuiltinComponent::SpotLight3D => spot,
    }
}

#[allow(clippy::type_complexity)]
fn sync_component_menu_visibility(
    selection: Res<Selection>,
    mut state: ResMut<InspectorComponentMenuState>,
    details: Query<(
        &SceneSpace,
        Option<&AddedEntityComponents>,
        Has<Transform>,
        Has<Sprite>,
        Has<Camera2d>,
        Has<Mesh3d>,
        Has<SceneModel3D>,
        Has<Camera3d>,
        Has<DirectionalLight>,
        Has<PointLight>,
        Has<SpotLight>,
    )>,
    mut menus: Query<
        &mut Node,
        (
            With<InspectorComponentMenu>,
            Without<InspectorComponentOption>,
        ),
    >,
    mut options: Query<(&InspectorComponentOption, &mut Node), Without<InspectorComponentMenu>>,
) {
    let selected = selection.0.and_then(|entity| details.get(entity).ok());
    if selected.is_none() {
        state.open = false;
    }
    for mut node in &mut menus {
        node.display = if state.open && selected.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    let Some((
        space,
        authored,
        transform,
        sprite,
        camera_2d,
        mesh,
        model,
        camera_3d,
        directional,
        point,
        spot,
    )) = selected
    else {
        for (_, mut node) in &mut options {
            node.display = Display::None;
        }
        return;
    };
    for (option, mut node) in &mut options {
        node.display = if option.0.supports(*space)
            && !authored.is_some_and(|components| components.0.contains(&option.0))
            && !component_is_present(
                option.0,
                transform,
                sprite,
                camera_2d,
                mesh || model,
                camera_3d,
                directional,
                point,
                spot,
            ) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn section(parent: &mut ChildSpawnerCommands, title: &str) {
    parent.spawn((
        Text::new(title),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(theme::accent()),
    ));
}

fn collision_section(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            InspectorCollisionSection,
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                padding: UiRect::all(Val::Px(7.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|collision| {
            collision
                .spawn(Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(24.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|header| {
                    header.spawn((
                        Text::new("CollisionRect2D"),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.95, 0.46, 0.48)),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        ..default()
                    });
                    header
                        .spawn((
                            Button,
                            WidgetButton,
                            InspectorCollisionReset,
                            Node {
                                width: Val::Px(24.0),
                                height: Val::Px(24.0),
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
                            ImageNode::new(asset_server.load("editor/icons/undo-2.png")),
                            Node {
                                width: Val::Px(13.0),
                                height: Val::Px(13.0),
                                ..default()
                            },
                        ));
                });
            collision
                .spawn((
                    Button,
                    WidgetButton,
                    InspectorCollisionEnabled,
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(28.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(8.0)),
                        column_gap: Val::Px(7.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::border_soft()),
                ))
                .with_children(|button| {
                    button.spawn((
                        InspectorCollisionEnabledIndicator,
                        Node {
                            width: Val::Px(12.0),
                            height: Val::Px(12.0),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(theme::accent()),
                        BorderColor::all(theme::accent()),
                    ));
                    button.spawn((
                        Text::new("Enabled"),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ));
                    button.spawn(Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        ..default()
                    });
                    button.spawn((
                        InspectorCollisionEnabledLabel,
                        Text::new("On"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                    ));
                });
            collision_numeric_row(collision, "Width", InspectorCollisionField::Width);
            collision_numeric_row(collision, "Height", InspectorCollisionField::Height);
            collision_numeric_row(collision, "Offset X", InspectorCollisionField::OffsetX);
            collision_numeric_row(collision, "Offset Y", InspectorCollisionField::OffsetY);
        });
}

fn collision_numeric_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    field: InspectorCollisionField,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: Val::Px(72.0),
                    min_width: Val::Px(72.0),
                    ..default()
                },
            ));
            collision_nudge_button(row, field, -1.0, "-");
            row.spawn((
                InspectorCollisionValueLabel(field),
                Text::new("0.0 px"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(23.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ));
            collision_nudge_button(row, field, 1.0, "+");
        });
}

fn collision_nudge_button(
    parent: &mut ChildSpawnerCommands,
    field: InspectorCollisionField,
    delta: f32,
    caption: &str,
) {
    parent.spawn((
        Button,
        WidgetButton,
        InspectorCollisionNudge { field, delta },
        Node {
            width: Val::Px(25.0),
            height: Val::Px(23.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(theme::bg_field()),
        BorderColor::all(theme::border_soft()),
        children![(
            Text::new(caption),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(theme::text_primary()),
        )],
    ));
}

fn sprite_section(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            InspectorSpriteSection,
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                padding: UiRect::all(Val::Px(7.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|sprite| {
            section(sprite, "Sprite2D");
            ui_input_label(sprite, "Texture");
            sprite
                .spawn((
                    InspectorSpriteImageDropTarget,
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(96.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::all(Val::Px(7.0)),
                        column_gap: Val::Px(9.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::border_soft()),
                    Pickable::default(),
                ))
                .observe(on_sprite_image_drag_enter)
                .observe(on_sprite_image_drag_leave)
                .observe(on_sprite_image_drop)
                .with_children(|target| {
                    target.spawn((
                        InspectorSpriteImagePreview,
                        ImageNode::default().with_mode(NodeImageMode::Stretch),
                        Node {
                            width: Val::Px(80.0),
                            min_width: Val::Px(80.0),
                            height: Val::Px(80.0),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.10, 0.11, 0.13)),
                        BorderColor::all(theme::border_soft()),
                        Pickable::IGNORE,
                    ));
                    target
                        .spawn((
                            Node {
                                width: Val::Px(0.0),
                                min_width: Val::Px(0.0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|details| {
                            details.spawn((
                                InspectorSpriteImageNameLabel,
                                Text::new("No texture assigned"),
                                TextFont {
                                    font_size: FontSize::Px(11.5),
                                    ..default()
                                },
                                TextColor(theme::text_primary()),
                                Pickable::IGNORE,
                            ));
                            details.spawn((
                                InspectorSpriteImagePathLabel,
                                Text::new("Drag an image from FileSystem"),
                                TextFont {
                                    font_size: FontSize::Px(9.5),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                                Pickable::IGNORE,
                            ));
                            details.spawn((
                                Text::new("PNG, BMP, TGA or KTX2"),
                                TextFont {
                                    font_size: FontSize::Px(8.5),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                                Pickable::IGNORE,
                            ));
                            details
                                .spawn((
                                    Button,
                                    WidgetButton,
                                    InspectorSpriteImageClearButton,
                                    Node {
                                        width: Val::Px(58.0),
                                        min_height: Val::Px(23.0),
                                        display: Display::None,
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        border: UiRect::all(Val::Px(1.0)),
                                        border_radius: BorderRadius::all(Val::Px(2.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                    BorderColor::all(theme::border_soft()),
                                    children![(
                                        Text::new("Clear"),
                                        TextFont {
                                            font_size: FontSize::Px(9.5),
                                            ..default()
                                        },
                                        TextColor(Color::srgb(0.94, 0.46, 0.50)),
                                    )],
                                ))
                                .observe(clear_sprite_image_resource);
                        });
                });
            ui_input_label(sprite, "Resource Path");
            ui_text_input(sprite, InspectorSpriteImageInput, "res://");

            section(sprite, "Sprite Sheet");
            sprite_numeric_row(sprite, "H Frames", InspectorSpriteField::HFrames, 1.0);
            sprite_numeric_row(sprite, "V Frames", InspectorSpriteField::VFrames, 1.0);
            sprite_frame_row(sprite, asset_server);

            section(sprite, "Rendering");
            sprite_toggle_row(sprite, "Visible", InspectorSpriteToggleKind::Visible);
            sprite_toggle_row(sprite, "Flip X", InspectorSpriteToggleKind::FlipX);
            sprite_toggle_row(sprite, "Flip Y", InspectorSpriteToggleKind::FlipY);
            sprite_numeric_row(sprite, "Z Index", InspectorSpriteField::ZIndex, 1.0);

            section(sprite, "Modulate");
            sprite.spawn((
                InspectorSpriteColorSwatch,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(22.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                BorderColor::all(theme::border_soft()),
            ));
            sprite_numeric_row(sprite, "Red", InspectorSpriteField::ColorR, 0.1);
            sprite_numeric_row(sprite, "Green", InspectorSpriteField::ColorG, 0.1);
            sprite_numeric_row(sprite, "Blue", InspectorSpriteField::ColorB, 0.1);
            sprite_numeric_row(sprite, "Alpha", InspectorSpriteField::ColorA, 0.1);

            section(sprite, "Region");
            sprite_toggle_row(sprite, "Enabled", InspectorSpriteToggleKind::Region);
            sprite
                .spawn((
                    InspectorSpriteRegionBody,
                    Node {
                        width: Val::Percent(100.0),
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        ..default()
                    },
                ))
                .with_children(|region| {
                    sprite_numeric_row(region, "X", InspectorSpriteField::RegionX, 1.0);
                    sprite_numeric_row(region, "Y", InspectorSpriteField::RegionY, 1.0);
                    sprite_numeric_row(region, "Width", InspectorSpriteField::RegionWidth, 1.0);
                    sprite_numeric_row(region, "Height", InspectorSpriteField::RegionHeight, 1.0);
                });

            section(sprite, "Pivot");
            sprite_numeric_row(sprite, "X", InspectorSpriteField::AnchorX, 0.1);
            sprite_numeric_row(sprite, "Y", InspectorSpriteField::AnchorY, 0.1);
        });
}

fn sprite_frame_row(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("Frame"),
                TextFont {
                    font_size: FontSize::Px(10.8),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: Val::Px(84.0),
                    min_width: Val::Px(84.0),
                    ..default()
                },
            ));
            sprite_nudge_button(row, InspectorSpriteField::Frame, -1.0, "-");
            row.spawn((
                Node {
                    width: Val::Px(0.0),
                    min_width: Val::Px(40.0),
                    flex_grow: 1.0,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                children![(
                    InspectorSpriteValueLabel(InspectorSpriteField::Frame),
                    Text::new("0"),
                    TextFont {
                        font_size: FontSize::Px(10.8),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                )],
            ));
            sprite_nudge_button(row, InspectorSpriteField::Frame, 1.0, "+");
            transform_icon_button(
                row,
                InspectorSpriteFrameKey,
                "editor/icons/key-round.png",
                asset_server,
                "Insert Sprite Frame key at the current animation time",
            );
        });
}

fn sprite_toggle_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    kind: InspectorSpriteToggleKind,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(29.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: Val::Px(84.0),
                    min_width: Val::Px(84.0),
                    ..default()
                },
            ));
            row.spawn((
                Button,
                WidgetButton,
                InspectorSpriteToggle(kind),
                Node {
                    width: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(27.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    column_gap: Val::Px(7.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ))
            .with_children(|button| {
                button.spawn((
                    InspectorSpriteToggleIndicator(kind),
                    Node {
                        width: Val::Px(12.0),
                        height: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(theme::border()),
                ));
                button.spawn((
                    InspectorSpriteToggleLabel(kind),
                    Text::new("Off"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                ));
            });
        });
}

fn sprite_numeric_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    field: InspectorSpriteField,
    step: f32,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(10.8),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: Val::Px(84.0),
                    min_width: Val::Px(84.0),
                    ..default()
                },
            ));
            sprite_nudge_button(row, field, -step, "-");
            row.spawn((
                Node {
                    width: Val::Px(0.0),
                    min_width: Val::Px(54.0),
                    flex_grow: 1.0,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                children![(
                    InspectorSpriteValueLabel(field),
                    Text::new("0.0"),
                    TextFont {
                        font_size: FontSize::Px(10.8),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                )],
            ));
            sprite_nudge_button(row, field, step, "+");
        });
}

fn sprite_nudge_button(
    parent: &mut ChildSpawnerCommands,
    field: InspectorSpriteField,
    delta: f32,
    caption: &str,
) {
    parent.spawn((
        Button,
        WidgetButton,
        InspectorSpriteNudge { field, delta },
        Node {
            width: Val::Px(22.0),
            min_width: Val::Px(22.0),
            height: Val::Px(22.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.07, 0.075, 0.085)),
        BorderColor::all(theme::border_soft()),
        children![(
            Text::new(caption),
            TextFont {
                font_size: FontSize::Px(10.5),
                ..default()
            },
            TextColor(theme::text_primary()),
        )],
    ));
}

fn ui_section(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            InspectorUiSection,
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
        ))
        .with_children(|ui| {
            section(ui, "Layout");
            ui_clip_contents_row(ui);
            ui_anchor_dropdown(ui);
            ui_transform_panel(ui, asset_server);

            ui_input_label(ui, "Content Alignment");
            ui.spawn(Node {
                width: Val::Percent(100.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(5.0),
                ..default()
            })
            .with_children(|alignments| {
                ui_action_button(
                    alignments,
                    InspectorAlignmentButton(AlignmentAxis::Horizontal),
                    "H: Start",
                    112.0,
                );
                ui_action_button(
                    alignments,
                    InspectorAlignmentButton(AlignmentAxis::Vertical),
                    "V: Start",
                    112.0,
                );
            });

            section(ui, "Content");

            ui.spawn((
                InspectorUiRow(InspectorUiRowKind::Text),
                Node {
                    width: Val::Percent(100.0),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    ..default()
                },
            ))
            .with_children(|row| {
                ui_input_label(row, "Text / Button Text");
                ui_text_input(row, InspectorUiTextInput, "");
            });

            ui.spawn((
                InspectorUiRow(InspectorUiRowKind::PanelColor),
                Node {
                    width: Val::Percent(100.0),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    ..default()
                },
            ))
            .with_children(|color| {
                ui_input_label(color, "Panel Color (RGBA)");
                ui_numeric_row(color, "red", InspectorUiField::ColorR, 0.05);
                ui_numeric_row(color, "green", InspectorUiField::ColorG, 0.05);
                ui_numeric_row(color, "blue", InspectorUiField::ColorB, 0.05);
                ui_numeric_row(color, "alpha", InspectorUiField::ColorA, 0.05);
            });

            ui.spawn((
                InspectorUiRow(InspectorUiRowKind::Image),
                Node {
                    width: Val::Percent(100.0),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    ..default()
                },
            ))
            .with_children(|row| {
                ui_input_label(row, "Texture");
                row.spawn((
                    InspectorUiImageDropTarget,
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(96.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::all(Val::Px(7.0)),
                        column_gap: Val::Px(9.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(theme::bg_field()),
                    BorderColor::all(theme::border_soft()),
                    Pickable::default(),
                ))
                .observe(on_ui_image_drag_enter)
                .observe(on_ui_image_drag_leave)
                .observe(on_ui_image_drop)
                .with_children(|target| {
                    target.spawn((
                        InspectorUiImagePreview,
                        ImageNode::default().with_mode(NodeImageMode::Stretch),
                        Node {
                            width: Val::Px(80.0),
                            min_width: Val::Px(80.0),
                            height: Val::Px(80.0),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.10, 0.11, 0.13)),
                        BorderColor::all(theme::border_soft()),
                        Pickable::IGNORE,
                    ));
                    target
                        .spawn((
                            Node {
                                width: Val::Px(0.0),
                                min_width: Val::Px(0.0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|details| {
                            details.spawn((
                                InspectorUiImageNameLabel,
                                Text::new("No texture assigned"),
                                TextFont {
                                    font_size: FontSize::Px(11.5),
                                    ..default()
                                },
                                TextColor(theme::text_primary()),
                                Pickable::IGNORE,
                            ));
                            details.spawn((
                                InspectorUiImagePathLabel,
                                Text::new("Drag an image from FileSystem"),
                                TextFont {
                                    font_size: FontSize::Px(9.5),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                                Pickable::IGNORE,
                            ));
                            details.spawn((
                                Text::new("PNG, BMP, TGA or KTX2"),
                                TextFont {
                                    font_size: FontSize::Px(8.5),
                                    ..default()
                                },
                                TextColor(theme::text_muted()),
                                Pickable::IGNORE,
                            ));
                            details
                                .spawn((
                                    Button,
                                    WidgetButton,
                                    InspectorUiImageClearButton,
                                    Node {
                                        width: Val::Px(58.0),
                                        min_height: Val::Px(23.0),
                                        display: Display::None,
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        border: UiRect::all(Val::Px(1.0)),
                                        border_radius: BorderRadius::all(Val::Px(2.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                    BorderColor::all(theme::border_soft()),
                                    children![(
                                        Text::new("Clear"),
                                        TextFont {
                                            font_size: FontSize::Px(9.5),
                                            ..default()
                                        },
                                        TextColor(Color::srgb(0.94, 0.46, 0.50)),
                                    )],
                                ))
                                .observe(clear_ui_image_resource);
                        });
                });
                ui_input_label(row, "Resource Path");
                ui_text_input(row, InspectorUiImageInput, "res://");
            });
        });
}

fn ui_clip_contents_row(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(30.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("Clip Contents"),
                TextFont {
                    font_size: FontSize::Px(11.5),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: Val::Px(84.0),
                    min_width: Val::Px(84.0),
                    ..default()
                },
            ));
            row.spawn((
                Button,
                WidgetButton,
                InspectorUiClipButton,
                Node {
                    width: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(28.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    column_gap: Val::Px(7.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ))
            .with_children(|button| {
                button.spawn((
                    InspectorUiClipIndicator,
                    Node {
                        width: Val::Px(12.0),
                        height: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(theme::border()),
                ));
                button.spawn((
                    InspectorUiClipLabel,
                    Text::new("Off"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                ));
            });
        });
}

fn ui_anchor_dropdown(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|dropdown| {
            dropdown
                .spawn(Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(30.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new("Anchors Preset"),
                        TextFont {
                            font_size: FontSize::Px(11.5),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                        Node {
                            width: Val::Px(84.0),
                            min_width: Val::Px(84.0),
                            ..default()
                        },
                    ));
                    row.spawn((
                        Button,
                        WidgetButton,
                        InspectorAnchorDropdownButton,
                        Node {
                            width: Val::Px(0.0),
                            min_width: Val::Px(0.0),
                            flex_grow: 1.0,
                            min_height: Val::Px(30.0),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(Val::Px(8.0)),
                            column_gap: Val::Px(7.0),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(theme::bg_field()),
                        BorderColor::all(theme::border_soft()),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Node {
                                width: Val::Px(14.0),
                                height: Val::Px(14.0),
                                border: UiRect::all(Val::Px(2.0)),
                                ..default()
                            },
                            BorderColor::all(theme::accent()),
                        ));
                        button.spawn((
                            InspectorAnchorDropdownLabel,
                            Text::new("Top Left"),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(theme::text_primary()),
                            Node {
                                width: Val::Px(0.0),
                                min_width: Val::Px(0.0),
                                flex_grow: 1.0,
                                ..default()
                            },
                        ));
                        button.spawn((
                            Text::new("v"),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::text_muted()),
                        ));
                    });
                });
            dropdown
                .spawn((
                    InspectorAnchorDropdownMenu,
                    Node {
                        width: Val::Percent(100.0),
                        max_height: Val::Px(300.0),
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(4.0)),
                        row_gap: Val::Px(2.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.075, 0.08, 0.09)),
                    BorderColor::all(theme::border()),
                ))
                .with_children(|menu| {
                    anchor_dropdown_status(menu, "Custom");
                    for preset in AnchorPreset::ALL {
                        anchor_dropdown_option(menu, preset);
                    }
                });
        });
}

fn anchor_dropdown_status(parent: &mut ChildSpawnerCommands, label: &str) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(theme::text_muted()),
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(24.0),
            padding: UiRect::horizontal(Val::Px(28.0)),
            align_items: AlignItems::Center,
            ..default()
        },
    ));
}

fn anchor_dropdown_option(parent: &mut ChildSpawnerCommands, preset: AnchorPreset) {
    parent
        .spawn((
            Button,
            WidgetButton,
            InspectorAnchorPreset(preset),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(7.0)),
                column_gap: Val::Px(8.0),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|option| {
            option.spawn((
                Node {
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(theme::text_muted()),
            ));
            option.spawn((
                Text::new(preset.label()),
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(theme::text_primary()),
            ));
        });
}

fn ui_transform_panel(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::bg_panel()),
            BorderColor::all(theme::border_soft()),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Button,
                    WidgetButton,
                    InspectorUiTransformToggle,
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(32.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(8.0)),
                        column_gap: Val::Px(7.0),
                        ..default()
                    },
                    BackgroundColor(theme::bg_toolbar()),
                ))
                .with_children(|header| {
                    header.spawn((
                        InspectorUiTransformChevron,
                        Text::new("v"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::text_muted()),
                    ));
                    header.spawn((
                        Text::new("Transform"),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(theme::text_primary()),
                    ));
                });
            panel
                .spawn((
                    InspectorUiTransformBody,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(6.0)),
                        row_gap: Val::Px(4.0),
                        border: UiRect::top(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(theme::border_soft()),
                ))
                .with_children(|body| {
                    ui_transform_group(
                        body,
                        "Size",
                        &[
                            (InspectorUiField::Width, "X", 1.0),
                            (InspectorUiField::Height, "Y", 1.0),
                        ],
                        InspectorUiTransformGroup::Size,
                        false,
                        asset_server,
                    );
                    ui_transform_group(
                        body,
                        "Position",
                        &[
                            (InspectorUiField::PositionX, "X", 1.0),
                            (InspectorUiField::PositionY, "Y", 1.0),
                        ],
                        InspectorUiTransformGroup::Position,
                        false,
                        asset_server,
                    );
                    ui_transform_group(
                        body,
                        "Rotation",
                        &[(InspectorUiField::Rotation, "", 1.0)],
                        InspectorUiTransformGroup::Rotation,
                        false,
                        asset_server,
                    );
                    ui_transform_group(
                        body,
                        "Scale",
                        &[
                            (InspectorUiField::ScaleX, "X", 0.1),
                            (InspectorUiField::ScaleY, "Y", 0.1),
                        ],
                        InspectorUiTransformGroup::Scale,
                        true,
                        asset_server,
                    );
                    ui_transform_group(
                        body,
                        "Pivot Offset",
                        &[
                            (InspectorUiField::PivotOffsetX, "X", 1.0),
                            (InspectorUiField::PivotOffsetY, "Y", 1.0),
                        ],
                        InspectorUiTransformGroup::PivotOffset,
                        false,
                        asset_server,
                    );
                    ui_transform_group(
                        body,
                        "Pivot Ratio",
                        &[
                            (InspectorUiField::PivotRatioX, "X", 0.1),
                            (InspectorUiField::PivotRatioY, "Y", 0.1),
                        ],
                        InspectorUiTransformGroup::PivotRatio,
                        false,
                        asset_server,
                    );
                    ui_transform_group(
                        body,
                        "Minimum Size",
                        &[
                            (InspectorUiField::MinimumWidth, "X", 1.0),
                            (InspectorUiField::MinimumHeight, "Y", 1.0),
                        ],
                        InspectorUiTransformGroup::MinimumSize,
                        false,
                        asset_server,
                    );
                    ui_transform_group(
                        body,
                        "Offsets",
                        &[
                            (InspectorUiField::MarginLeft, "L", 1.0),
                            (InspectorUiField::MarginTop, "T", 1.0),
                            (InspectorUiField::MarginRight, "R", 1.0),
                            (InspectorUiField::MarginBottom, "B", 1.0),
                        ],
                        InspectorUiTransformGroup::Offsets,
                        false,
                        asset_server,
                    );
                });
        });
}

fn ui_transform_group(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    fields: &[(InspectorUiField, &str, f32)],
    group: InspectorUiTransformGroup,
    show_link: bool,
    asset_server: &AssetServer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(38.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(10.8),
                    ..default()
                },
                TextColor(theme::text_primary()),
                Node {
                    width: Val::Px(78.0),
                    min_width: Val::Px(78.0),
                    ..default()
                },
            ));
            row.spawn((
                Node {
                    width: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(3.0)),
                    row_gap: Val::Px(2.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ))
            .with_children(|values| {
                for (field, axis, step) in fields.iter().copied() {
                    ui_transform_value_line(values, field, axis, step);
                }
            });
            if show_link {
                transform_icon_button(
                    row,
                    InspectorUiScaleLinkButton,
                    "editor/icons/link.png",
                    asset_server,
                    "Lock scale proportions",
                );
            }
            if let Some(property) = ui_animation_property(group) {
                transform_icon_button(
                    row,
                    InspectorUiTransformKey(property),
                    "editor/icons/key-round.png",
                    asset_server,
                    "Insert key at the current animation time",
                );
            }
            transform_icon_button(
                row,
                InspectorUiTransformReset(group),
                "editor/icons/undo-2.png",
                asset_server,
                "Reset property",
            );
        });
}

const fn ui_animation_property(
    group: InspectorUiTransformGroup,
) -> Option<AnimationTransformProperty> {
    match group {
        InspectorUiTransformGroup::Position => Some(AnimationTransformProperty::Position),
        InspectorUiTransformGroup::Rotation => Some(AnimationTransformProperty::Rotation),
        InspectorUiTransformGroup::Scale => Some(AnimationTransformProperty::Scale),
        InspectorUiTransformGroup::Size
        | InspectorUiTransformGroup::PivotOffset
        | InspectorUiTransformGroup::PivotRatio
        | InspectorUiTransformGroup::MinimumSize
        | InspectorUiTransformGroup::Offsets => None,
    }
}

fn ui_transform_value_line(
    parent: &mut ChildSpawnerCommands,
    field: InspectorUiField,
    axis: &str,
    step: f32,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(23.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|line| {
            line.spawn((
                Text::new(axis),
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(transform_axis_color(axis)),
                Node {
                    width: Val::Px(13.0),
                    min_width: Val::Px(13.0),
                    ..default()
                },
            ));
            ui_value_nudge_button(line, field, -step, "-");
            line.spawn((
                InspectorUiValueLabel(field),
                Node {
                    width: Val::Px(0.0),
                    min_width: Val::Px(42.0),
                    flex_grow: 1.0,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                children![(
                    Text::new("0.0"),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                )],
            ));
            ui_value_nudge_button(line, field, step, "+");
        });
}

fn ui_value_nudge_button(
    parent: &mut ChildSpawnerCommands,
    field: InspectorUiField,
    delta: f32,
    caption: &str,
) {
    parent.spawn((
        Button,
        WidgetButton,
        InspectorUiNudge { field, delta },
        Node {
            width: Val::Px(21.0),
            min_width: Val::Px(21.0),
            height: Val::Px(21.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.07, 0.075, 0.085)),
        BorderColor::all(theme::border_soft()),
        children![(
            Text::new(caption),
            TextFont {
                font_size: FontSize::Px(10.5),
                ..default()
            },
            TextColor(theme::text_primary()),
        )],
    ));
}

fn ui_input_label(parent: &mut ChildSpawnerCommands, label: &str) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(theme::text_muted()),
    ));
}

fn ui_text_input<M: Component + Clone>(parent: &mut ChildSpawnerCommands, marker: M, value: &str) {
    parent.spawn((
        marker,
        EditableText::new(value),
        TextCursorStyle::default(),
        TextFont {
            font_size: FontSize::Px(11.5),
            ..default()
        },
        TextColor(theme::text_primary()),
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(30.0),
            padding: UiRect::axes(Val::Px(7.0), Val::Px(5.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(theme::bg_field()),
        BorderColor::all(theme::border_soft()),
    ));
}

fn entity_script_type_path(source_path: &str) -> String {
    let value = source_path
        .strip_prefix("res://")
        .unwrap_or(source_path)
        .trim_end_matches(".rs")
        .replace('/', "::")
        .replace('-', "_");
    format!("script::{value}")
}

fn on_entity_script_drag_enter(
    enter: On<Pointer<DragEnter>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    targets: Query<(), With<InspectorEntityScriptDropTarget>>,
    mut chrome: Query<(&mut BackgroundColor, &mut BorderColor)>,
) {
    if targets.get(enter.entity).is_err() {
        return;
    }
    let valid = dragged_filesystem_entry(enter.dragged, &rows, &parents)
        .filter(|(_, is_dir)| !*is_dir)
        .is_some_and(|(relative, _)| rust_script_resource_path_from_filesystem(&relative).is_ok());
    if valid {
        if let Ok((mut background, mut border)) = chrome.get_mut(enter.entity) {
            background.0 = theme::bg_selected();
            *border = BorderColor::all(theme::accent());
        }
    }
}

fn on_entity_script_drag_leave(
    leave: On<Pointer<DragLeave>>,
    targets: Query<(), With<InspectorEntityScriptDropTarget>>,
    mut chrome: Query<(&mut BackgroundColor, &mut BorderColor)>,
) {
    if targets.get(leave.entity).is_err() {
        return;
    }
    if let Ok((mut background, mut border)) = chrome.get_mut(leave.entity) {
        background.0 = theme::bg_field();
        *border = BorderColor::all(theme::border_soft());
    }
}

fn on_entity_script_drop(
    mut drop: On<Pointer<DragDrop>>,
    targets: Query<(), With<InspectorEntityScriptDropTarget>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    selection: Res<Selection>,
    registry: Res<RustComponentRegistry>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntityScriptBinding>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut filesystem: ResMut<FileSystemState>,
    mut output: Option<ResMut<OutputLog>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut chrome: Query<(&mut BackgroundColor, &mut BorderColor)>,
) {
    if targets.get(drop.entity).is_err() {
        return;
    }
    if let Ok((mut background, mut border)) = chrome.get_mut(drop.entity) {
        background.0 = theme::bg_field();
        *border = BorderColor::all(theme::border_soft());
    }
    if drop.button != PointerButton::Primary {
        return;
    }
    let Some((relative, is_dir)) = dragged_filesystem_entry(drop.dropped, &rows, &parents) else {
        return;
    };
    if is_dir {
        filesystem.status = "Drop a Rust source file, not a folder.".into();
        return;
    }
    let source_path = match rust_script_resource_path_from_filesystem(&relative) {
        Ok(path) => path,
        Err(error) => {
            filesystem.status = error;
            return;
        }
    };
    let Some(entity) = selection.0 else {
        filesystem.status = "Select an Entity before attaching a script.".into();
        return;
    };
    let candidates = registry
        .entity_scripts
        .iter()
        .filter(|callback| callback.source_path == source_path)
        .collect::<Vec<_>>();
    let callbacks = candidates
        .iter()
        .filter(|callback| callback.valid)
        .map(|callback| SceneEntityScriptCallback {
            function_path: callback.function_path.clone(),
            lifecycle: callback.lifecycle,
            enabled: true,
        })
        .collect::<Vec<_>>();
    if callbacks.is_empty() {
        let detail = candidates
            .iter()
            .filter_map(|callback| callback.diagnostic.as_deref())
            .next()
            .unwrap_or("No Start, Update, FixedUpdate, or PostUpdate function accepting In<Entity> was found");
        let message = format!("Cannot attach {source_path}: {detail}");
        filesystem.status.clone_from(&message);
        filesystem.revision = filesystem.revision.wrapping_add(1);
        if let Some(output) = output.as_deref_mut() {
            output.push(OutputLevel::Error, message);
        }
        return;
    }
    let script = SceneEntityScript {
        source_path: source_path.clone(),
        type_path: entity_script_type_path(&source_path),
        enabled: true,
        callbacks,
    };
    let changed = nodes
        .p1()
        .get_mut(entity)
        .ok()
        .is_some_and(|binding| binding.0.as_ref() != Some(&script));
    if !changed {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Attach Entity Script",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    if let Ok(mut binding) = nodes.p1().get_mut(entity) {
        binding.0 = Some(script);
    }
    filesystem.status = format!("Attached {source_path} to Entity");
    mark_document_changed(document.as_deref_mut());
    drop.propagate(false);
}

fn clear_entity_script(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorClearEntityScriptButton>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntityScriptBinding>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut filesystem: Option<ResMut<FileSystemState>>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(entity) = selection.0 else {
        return;
    };
    let has_script = nodes
        .p1()
        .get_mut(entity)
        .is_ok_and(|binding| binding.0.is_some());
    if !has_script {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Clear Entity Script",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    if let Ok(mut binding) = nodes.p1().get_mut(entity) {
        binding.0 = None;
    }
    if let Some(filesystem) = filesystem.as_deref_mut() {
        filesystem.status = "Cleared Entity script".into();
    }
    mark_document_changed(document.as_deref_mut());
}

fn open_entity_script(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorOpenEntityScriptButton>>,
    selection: Res<Selection>,
    scripts: Query<&EntityScriptBinding>,
    project: Option<Res<ProjectRoot>>,
    mut filesystem: Option<ResMut<FileSystemState>>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let (Some(project), Some(filesystem)) = (project.as_deref(), filesystem.as_deref_mut()) else {
        return;
    };
    let Some(script) = selection
        .0
        .and_then(|entity| scripts.get(entity).ok())
        .and_then(|binding| binding.0.as_ref())
    else {
        return;
    };
    let Some(relative) = script.source_path.strip_prefix("res://") else {
        filesystem.status = "Entity script has an invalid resource path.".into();
        return;
    };
    let Some(path) = project.resolve_existing(relative) else {
        filesystem.status = format!("Entity script is missing: {}", script.source_path);
        return;
    };
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer.exe")
        .arg(&path)
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&path).spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(&path).spawn();
    filesystem.status = match result {
        Ok(_) => format!("Opened {}", script.source_path),
        Err(error) => format!("Could not open {}: {error}", script.source_path),
    };
    filesystem.revision = filesystem.revision.wrapping_add(1);
}

fn rebuild_entity_script_lifecycles(
    mut commands: Commands,
    selection: Res<Selection>,
    scripts: Query<&EntityScriptBinding>,
    lists: Query<Entity, With<InspectorEntityScriptLifecycleList>>,
    registry: Res<RustComponentRegistry>,
    mut last_selection: Local<Option<Entity>>,
    mut last_registry_revision: Local<u64>,
    mut last_callbacks: Local<Vec<SceneEntityScriptCallback>>,
) {
    let callbacks = selection
        .0
        .and_then(|entity| scripts.get(entity).ok())
        .and_then(|binding| binding.0.as_ref())
        .map(|script| script.callbacks.clone())
        .unwrap_or_default();
    if *last_selection == selection.0
        && *last_registry_revision == registry.revision
        && *last_callbacks == callbacks
    {
        return;
    }
    let Ok(list) = lists.single() else {
        return;
    };
    commands.entity(list).despawn_related::<Children>();
    commands.entity(list).with_children(|list| {
        if callbacks.is_empty() {
            list.spawn((
                Text::new("No lifecycle functions found"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(theme::text_muted()),
            ));
        }
        for callback in &callbacks {
            let name = callback
                .function_path
                .rsplit("::")
                .next()
                .unwrap_or(&callback.function_path);
            list.spawn((
                Button,
                WidgetButton,
                InspectorEntityScriptCallbackToggle(callback.function_path.clone()),
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(24.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(7.0)),
                    column_gap: Val::Px(6.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::bg_field()),
                BorderColor::all(theme::border_soft()),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(callback.lifecycle.label()),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(theme::accent()),
                    Node {
                        width: Val::Px(68.0),
                        ..default()
                    },
                ));
                row.spawn((
                    Text::new(name.to_owned()),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                row.spawn((
                    Text::new(if callback.enabled { "On" } else { "Off" }),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(if callback.enabled {
                        theme::accent()
                    } else {
                        theme::text_muted()
                    }),
                ));
            });
        }
    });
    *last_selection = selection.0;
    *last_registry_revision = registry.revision;
    *last_callbacks = callbacks;
}

fn toggle_entity_script_callback(
    activate: On<Activate>,
    buttons: Query<&InspectorEntityScriptCallbackToggle>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntityScriptBinding>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else {
        return;
    };
    let exists = nodes.p1().get_mut(entity).ok().is_some_and(|binding| {
        binding.0.as_ref().is_some_and(|script| {
            script
                .callbacks
                .iter()
                .any(|callback| callback.function_path == button.0)
        })
    });
    if !exists {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Toggle Entity Script Lifecycle",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    if let Ok(mut binding) = nodes.p1().get_mut(entity)
        && let Some(script) = binding.0.as_mut()
        && let Some(callback) = script
            .callbacks
            .iter_mut()
            .find(|callback| callback.function_path == button.0)
    {
        callback.enabled = !callback.enabled;
        mark_document_changed(document.as_deref_mut());
    }
}

fn report_entity_script_diagnostics(
    registry: Option<Res<RustComponentRegistry>>,
    mut output: Option<ResMut<OutputLog>>,
    mut last_revision: Local<u64>,
) {
    let Some(registry) = registry.as_deref() else {
        return;
    };
    if *last_revision == registry.revision {
        return;
    }
    *last_revision = registry.revision;
    let Some(output) = output.as_deref_mut() else {
        return;
    };
    for diagnostic in registry
        .entity_scripts
        .iter()
        .filter_map(|script| script.diagnostic.as_deref())
    {
        output.push(OutputLevel::Error, diagnostic);
    }
}

fn on_custom_component_drag_enter(
    enter: On<Pointer<DragEnter>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    targets: Query<(), With<InspectorCustomComponentDropTarget>>,
    mut chrome: Query<(&mut BackgroundColor, &mut BorderColor)>,
) {
    if targets.get(enter.entity).is_err() {
        return;
    }
    let valid = dragged_filesystem_entry(enter.dragged, &rows, &parents)
        .filter(|(_, is_dir)| !*is_dir)
        .is_some_and(|(relative, _)| rust_script_resource_path_from_filesystem(&relative).is_ok());
    if valid && let Ok((mut background, mut border)) = chrome.get_mut(enter.entity) {
        background.0 = theme::bg_selected();
        *border = BorderColor::all(theme::accent());
    }
}

fn on_custom_component_drag_leave(
    leave: On<Pointer<DragLeave>>,
    targets: Query<(), With<InspectorCustomComponentDropTarget>>,
    mut chrome: Query<(&mut BackgroundColor, &mut BorderColor)>,
) {
    if targets.get(leave.entity).is_err() {
        return;
    }
    if let Ok((mut background, mut border)) = chrome.get_mut(leave.entity) {
        background.0 = Color::NONE;
        *border = BorderColor::all(Color::NONE);
    }
}

fn on_custom_component_drop(
    mut drop: On<Pointer<DragDrop>>,
    targets: Query<(), With<InspectorCustomComponentDropTarget>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    registry: Res<RustComponentRegistry>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<&mut EntityCustomComponents>,
    )>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorCustomComponentsUiState>,
    mut filesystem: ResMut<FileSystemState>,
    mut document: Option<ResMut<SceneDocument>>,
    mut chrome: Query<(&mut BackgroundColor, &mut BorderColor)>,
) {
    if targets.get(drop.entity).is_err() {
        return;
    }
    if let Ok((mut background, mut border)) = chrome.get_mut(drop.entity) {
        background.0 = Color::NONE;
        *border = BorderColor::all(Color::NONE);
    }
    if drop.button != PointerButton::Primary {
        return;
    }
    let Some((relative, is_dir)) = dragged_filesystem_entry(drop.dropped, &rows, &parents) else {
        return;
    };
    if is_dir {
        filesystem.status = "Drop a Rust source file, not a folder.".into();
        return;
    }
    let source_path = match rust_script_resource_path_from_filesystem(&relative) {
        Ok(path) => path,
        Err(error) => {
            filesystem.status = error;
            return;
        }
    };
    let candidates: Vec<_> = registry
        .components
        .iter()
        .filter(|definition| definition.source_path == source_path)
        .collect();
    let Some(entity) = selection.0 else {
        filesystem.status = "Select an Entity before adding a component.".into();
        return;
    };
    if candidates.len() == 0 {
        filesystem.status = format!("No #[derive(Component)] type found in {source_path}");
    } else if candidates.len() > 1 {
        state.menu_open = true;
        state.source_filter = Some(source_path);
        state.revision = state.revision.wrapping_add(1);
    } else {
        let definition = candidates[0];
        let already = nodes.p1().get_mut(entity).ok().is_some_and(|components| {
            components
                .0
                .iter()
                .any(|component| component.type_path == definition.type_path)
        });
        if already {
            filesystem.status = format!("{} is already attached", definition.name);
            return;
        }
        if let Some(history) = history.as_deref_mut() {
            let history_nodes = nodes.p0();
            history.begin(
                format!("Add {}", definition.name),
                capture_scene_snapshot(&history_nodes, &selection, *mode),
            );
        }
        if let Ok(mut components) = nodes.p1().get_mut(entity) {
            components.0.push(definition.instantiate());
        }
        state.expanded_component = Some(definition.type_path.clone());
        state.menu_open = false;
        state.source_filter = None;
        state.revision = state.revision.wrapping_add(1);
        filesystem.status = format!("Attached {}", definition.name);
        mark_document_changed(document.as_deref_mut());
    }
    drop.propagate(false);
}

fn dragged_filesystem_entry(
    mut entity: Entity,
    rows: &Query<&FsTreeRow>,
    parents: &Query<&ChildOf>,
) -> Option<(String, bool)> {
    for _ in 0..16 {
        if let Ok(row) = rows.get(entity) {
            return Some((row.relative.clone(), row.is_dir));
        }
        entity = parents.get(entity).ok()?.parent();
    }
    None
}

fn set_ui_image_drop_style(
    active: bool,
    targets: &mut Query<(&mut BackgroundColor, &mut BorderColor), With<InspectorUiImageDropTarget>>,
) {
    let Ok((mut background, mut border)) = targets.single_mut() else {
        return;
    };
    background.0 = if active {
        Color::srgb(0.10, 0.20, 0.26)
    } else {
        theme::bg_field()
    };
    *border = BorderColor::all(if active {
        theme::accent()
    } else {
        theme::border_soft()
    });
}

fn on_ui_image_drag_enter(
    enter: On<Pointer<DragEnter>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    mut targets: Query<(&mut BackgroundColor, &mut BorderColor), With<InspectorUiImageDropTarget>>,
) {
    let valid = dragged_filesystem_entry(enter.dragged, &rows, &parents)
        .filter(|(_, is_dir)| !*is_dir)
        .is_some_and(|(relative, _)| image_resource_path_from_filesystem(&relative).is_ok());
    if valid {
        set_ui_image_drop_style(true, &mut targets);
    }
}

fn on_ui_image_drag_leave(
    _leave: On<Pointer<DragLeave>>,
    mut targets: Query<(&mut BackgroundColor, &mut BorderColor), With<InspectorUiImageDropTarget>>,
) {
    set_ui_image_drop_style(false, &mut targets);
}

fn on_ui_image_drop(
    mut drop: On<Pointer<DragDrop>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    selection: Res<Selection>,
    kinds: Query<&EntityKind>,
    mut inputs: Query<&mut EditableText, With<InspectorUiImageInput>>,
    mut binding: ResMut<InspectorUiInputBinding>,
    mut filesystem: ResMut<FileSystemState>,
    mut targets: Query<(&mut BackgroundColor, &mut BorderColor), With<InspectorUiImageDropTarget>>,
) {
    set_ui_image_drop_style(false, &mut targets);
    if drop.button != PointerButton::Primary {
        return;
    }
    let Some(entity) = selection.0 else {
        filesystem.status = "Select a UI Image entity before assigning a texture.".into();
        return;
    };
    if !kinds
        .get(entity)
        .is_ok_and(|kind| *kind == EntityKind::Image)
    {
        filesystem.status = "Textures can only be dropped onto a UI Image entity.".into();
        return;
    }
    let Some((relative, is_dir)) = dragged_filesystem_entry(drop.dropped, &rows, &parents) else {
        return;
    };
    if is_dir {
        filesystem.status = "Drop an image file, not a folder.".into();
        return;
    }
    let resource_path = match image_resource_path_from_filesystem(&relative) {
        Ok(path) => path,
        Err(error) => {
            filesystem.status = error;
            return;
        }
    };
    let Ok(mut input) = inputs.single_mut() else {
        return;
    };
    if input.value().to_string() != resource_path {
        input.editor_mut().set_text(&resource_path);
    }
    binding.suppress_changes = false;
    filesystem.status = format!("Assigned {resource_path} to UI Image");
    drop.propagate(false);
}

fn clear_ui_image_resource(
    _activate: On<Activate>,
    mut inputs: Query<&mut EditableText, With<InspectorUiImageInput>>,
    mut binding: ResMut<InspectorUiInputBinding>,
) {
    let Ok(mut input) = inputs.single_mut() else {
        return;
    };
    if !input.value().to_string().is_empty() {
        input.editor_mut().set_text("");
    }
    binding.suppress_changes = false;
}

fn set_sprite_image_drop_style(
    active: bool,
    targets: &mut Query<
        (&mut BackgroundColor, &mut BorderColor),
        With<InspectorSpriteImageDropTarget>,
    >,
) {
    let Ok((mut background, mut border)) = targets.single_mut() else {
        return;
    };
    background.0 = if active {
        Color::srgb(0.10, 0.20, 0.26)
    } else {
        theme::bg_field()
    };
    *border = BorderColor::all(if active {
        theme::accent()
    } else {
        theme::border_soft()
    });
}

fn set_mesh3d_drop_style(
    active: bool,
    targets: &mut Query<(&mut BackgroundColor, &mut BorderColor), With<InspectorMesh3dDropTarget>>,
) {
    let Ok((mut background, mut border)) = targets.single_mut() else {
        return;
    };
    background.0 = if active {
        Color::srgb(0.10, 0.20, 0.26)
    } else {
        theme::bg_field()
    };
    *border = BorderColor::all(if active {
        theme::accent()
    } else {
        theme::border_soft()
    });
}

fn on_mesh3d_drag_enter(
    enter: On<Pointer<DragEnter>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    mut targets: Query<(&mut BackgroundColor, &mut BorderColor), With<InspectorMesh3dDropTarget>>,
) {
    let valid = dragged_filesystem_entry(enter.dragged, &rows, &parents)
        .filter(|(_, is_dir)| !*is_dir)
        .is_some_and(|(relative, _)| model_resource_path_from_filesystem(&relative).is_ok());
    if valid {
        set_mesh3d_drop_style(true, &mut targets);
    }
}

fn on_mesh3d_drag_leave(
    _leave: On<Pointer<DragLeave>>,
    mut targets: Query<(&mut BackgroundColor, &mut BorderColor), With<InspectorMesh3dDropTarget>>,
) {
    set_mesh3d_drop_style(false, &mut targets);
}

fn on_mesh3d_drop(
    mut drop: On<Pointer<DragDrop>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    selection: Res<Selection>,
    kinds: Query<&EntityKind>,
    mut inputs: Query<&mut EditableText, With<InspectorMesh3dInput>>,
    mut binding: ResMut<InspectorMesh3dInputBinding>,
    mut filesystem: ResMut<FileSystemState>,
    mut targets: Query<(&mut BackgroundColor, &mut BorderColor), With<InspectorMesh3dDropTarget>>,
) {
    set_mesh3d_drop_style(false, &mut targets);
    if drop.button != PointerButton::Primary {
        return;
    }
    let Some(entity) = selection.0 else {
        filesystem.status = "Select a Mesh3D entity before assigning a model.".into();
        return;
    };
    if !kinds
        .get(entity)
        .is_ok_and(|kind| *kind == EntityKind::Mesh3D)
    {
        filesystem.status = "Models can only be dropped onto a Mesh3D entity.".into();
        return;
    }
    let Some((relative, is_dir)) = dragged_filesystem_entry(drop.dropped, &rows, &parents) else {
        return;
    };
    if is_dir {
        filesystem.status = "Drop a model file, not a folder.".into();
        return;
    }
    let resource_path = match model_resource_path_from_filesystem(&relative) {
        Ok(path) => path,
        Err(error) => {
            filesystem.status = error;
            return;
        }
    };
    let Ok(mut input) = inputs.single_mut() else {
        return;
    };
    if input.value().to_string() != resource_path {
        input.editor_mut().set_text(&resource_path);
    }
    binding.suppress_changes = false;
    filesystem.status = format!("Assigned {resource_path} to Mesh3D");
    drop.propagate(false);
}

fn clear_mesh3d_resource(
    _activate: On<Activate>,
    mut inputs: Query<&mut EditableText, With<InspectorMesh3dInput>>,
    mut binding: ResMut<InspectorMesh3dInputBinding>,
) {
    let Ok(mut input) = inputs.single_mut() else {
        return;
    };
    if !input.value().to_string().is_empty() {
        input.editor_mut().set_text("");
    }
    binding.suppress_changes = false;
}

fn on_sprite_image_drag_enter(
    enter: On<Pointer<DragEnter>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    mut targets: Query<
        (&mut BackgroundColor, &mut BorderColor),
        With<InspectorSpriteImageDropTarget>,
    >,
) {
    let valid = dragged_filesystem_entry(enter.dragged, &rows, &parents)
        .filter(|(_, is_dir)| !*is_dir)
        .is_some_and(|(relative, _)| image_resource_path_from_filesystem(&relative).is_ok());
    if valid {
        set_sprite_image_drop_style(true, &mut targets);
    }
}

fn on_sprite_image_drag_leave(
    _leave: On<Pointer<DragLeave>>,
    mut targets: Query<
        (&mut BackgroundColor, &mut BorderColor),
        With<InspectorSpriteImageDropTarget>,
    >,
) {
    set_sprite_image_drop_style(false, &mut targets);
}

fn on_sprite_image_drop(
    mut drop: On<Pointer<DragDrop>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    selection: Res<Selection>,
    sprites: Query<(), With<SceneSprite2D>>,
    mut inputs: Query<&mut EditableText, With<InspectorSpriteImageInput>>,
    mut binding: ResMut<InspectorSpriteInputBinding>,
    mut filesystem: ResMut<FileSystemState>,
    mut targets: Query<
        (&mut BackgroundColor, &mut BorderColor),
        With<InspectorSpriteImageDropTarget>,
    >,
) {
    set_sprite_image_drop_style(false, &mut targets);
    if drop.button != PointerButton::Primary {
        return;
    }
    let Some(entity) = selection.0 else {
        filesystem.status = "Select a Sprite2D entity before assigning a texture.".into();
        return;
    };
    if sprites.get(entity).is_err() {
        filesystem.status = "Textures can only be dropped onto an entity with Sprite.".into();
        return;
    }
    let Some((relative, is_dir)) = dragged_filesystem_entry(drop.dropped, &rows, &parents) else {
        return;
    };
    if is_dir {
        filesystem.status = "Drop an image file, not a folder.".into();
        return;
    }
    let resource_path = match image_resource_path_from_filesystem(&relative) {
        Ok(path) => path,
        Err(error) => {
            filesystem.status = error;
            return;
        }
    };
    let Ok(mut input) = inputs.single_mut() else {
        return;
    };
    if input.value().to_string() != resource_path {
        input.editor_mut().set_text(&resource_path);
    }
    binding.suppress_changes = false;
    filesystem.status = format!("Assigned {resource_path} to Sprite2D");
    drop.propagate(false);
}

fn clear_sprite_image_resource(
    _activate: On<Activate>,
    mut inputs: Query<&mut EditableText, With<InspectorSpriteImageInput>>,
    mut binding: ResMut<InspectorSpriteInputBinding>,
) {
    let Ok(mut input) = inputs.single_mut() else {
        return;
    };
    if !input.value().to_string().is_empty() {
        input.editor_mut().set_text("");
    }
    binding.suppress_changes = false;
}

fn toggle_sprite_option(
    activate: On<Activate>,
    buttons: Query<&InspectorSpriteToggle>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneSprite2D>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(toggle) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Edit Sprite2D", before);
    }
    let mut sprites = nodes.p1();
    let Ok(mut sprite) = sprites.get_mut(entity) else {
        return;
    };
    match toggle.0 {
        InspectorSpriteToggleKind::Visible => sprite.visible = !sprite.visible,
        InspectorSpriteToggleKind::FlipX => sprite.flip_x = !sprite.flip_x,
        InspectorSpriteToggleKind::FlipY => sprite.flip_y = !sprite.flip_y,
        InspectorSpriteToggleKind::Region => sprite.region_enabled = !sprite.region_enabled,
    }
    mark_document_changed(document.as_deref_mut());
}

fn apply_sprite_nudge(
    activate: On<Activate>,
    nudges: Query<&InspectorSpriteNudge>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneSprite2D>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(nudge) = nudges.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Edit Sprite2D", before);
    }
    let mut sprites = nodes.p1();
    let Ok(mut sprite) = sprites.get_mut(entity) else {
        return;
    };
    let previous = sprite.clone();
    match nudge.field {
        InspectorSpriteField::HFrames => {
            sprite.hframes =
                (sprite.hframes as i64 + nudge.delta.round() as i64).clamp(1, 4096) as u32;
            sprite.frame = sprite.clamped_frame();
        }
        InspectorSpriteField::VFrames => {
            sprite.vframes =
                (sprite.vframes as i64 + nudge.delta.round() as i64).clamp(1, 4096) as u32;
            sprite.frame = sprite.clamped_frame();
        }
        InspectorSpriteField::Frame => {
            sprite.frame = (sprite.frame as i64 + nudge.delta.round() as i64)
                .clamp(0, sprite.frame_count().saturating_sub(1) as i64)
                as u32;
        }
        InspectorSpriteField::RegionX => {
            sprite.region_rect.0 = round_ui_value(sprite.region_rect.0 + nudge.delta).max(0.0)
        }
        InspectorSpriteField::RegionY => {
            sprite.region_rect.1 = round_ui_value(sprite.region_rect.1 + nudge.delta).max(0.0)
        }
        InspectorSpriteField::RegionWidth => {
            sprite.region_rect.2 = round_ui_value(sprite.region_rect.2 + nudge.delta).max(1.0)
        }
        InspectorSpriteField::RegionHeight => {
            sprite.region_rect.3 = round_ui_value(sprite.region_rect.3 + nudge.delta).max(1.0)
        }
        InspectorSpriteField::ColorR => {
            sprite.color.0 = round_ui_value(sprite.color.0 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorSpriteField::ColorG => {
            sprite.color.1 = round_ui_value(sprite.color.1 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorSpriteField::ColorB => {
            sprite.color.2 = round_ui_value(sprite.color.2 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorSpriteField::ColorA => {
            sprite.color.3 = round_ui_value(sprite.color.3 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorSpriteField::AnchorX => {
            sprite.anchor.0 = round_ui_value(sprite.anchor.0 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorSpriteField::AnchorY => {
            sprite.anchor.1 = round_ui_value(sprite.anchor.1 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorSpriteField::ZIndex => {
            sprite.z_index = (sprite.z_index + nudge.delta.round() as i32).clamp(-4096, 4096)
        }
    }
    if *sprite != previous {
        mark_document_changed(document.as_deref_mut());
    }
}

fn toggle_collision_enabled(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorCollisionEnabled>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneCollisionRect2D>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Toggle CollisionRect2D", before);
    }
    let mut collisions = nodes.p1();
    let Ok(mut collision) = collisions.get_mut(entity) else {
        return;
    };
    collision.enabled = !collision.enabled;
    mark_document_changed(document.as_deref_mut());
}

fn apply_collision_nudge(
    activate: On<Activate>,
    nudges: Query<&InspectorCollisionNudge>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneCollisionRect2D>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(nudge) = nudges.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Edit CollisionRect2D", before);
    }
    let mut collisions = nodes.p1();
    let Ok(mut collision) = collisions.get_mut(entity) else {
        return;
    };
    let previous = *collision;
    match nudge.field {
        InspectorCollisionField::Width => {
            collision.size.0 = round_ui_value(collision.size.0 + nudge.delta).max(1.0)
        }
        InspectorCollisionField::Height => {
            collision.size.1 = round_ui_value(collision.size.1 + nudge.delta).max(1.0)
        }
        InspectorCollisionField::OffsetX => {
            collision.offset.0 = round_ui_value(collision.offset.0 + nudge.delta)
        }
        InspectorCollisionField::OffsetY => {
            collision.offset.1 = round_ui_value(collision.offset.1 + nudge.delta)
        }
    }
    if *collision != previous {
        mark_document_changed(document.as_deref_mut());
    }
}

fn reset_collision_rect(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorCollisionReset>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneCollisionRect2D>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Reset CollisionRect2D", before);
    }
    let mut collisions = nodes.p1();
    let Ok(mut collision) = collisions.get_mut(entity) else {
        return;
    };
    if *collision != SceneCollisionRect2D::default() {
        *collision = SceneCollisionRect2D::default();
        mark_document_changed(document.as_deref_mut());
    }
}

fn sync_collision_labels(
    selection: Res<Selection>,
    collisions: Query<&SceneCollisionRect2D>,
    mut values: Query<
        (&InspectorCollisionValueLabel, &mut Text),
        Without<InspectorCollisionEnabledLabel>,
    >,
    mut enabled_labels: Query<
        &mut Text,
        (
            With<InspectorCollisionEnabledLabel>,
            Without<InspectorCollisionValueLabel>,
        ),
    >,
    mut enabled_indicators: Query<
        (&mut BackgroundColor, &mut BorderColor),
        With<InspectorCollisionEnabledIndicator>,
    >,
) {
    let collision = selection.0.and_then(|entity| collisions.get(entity).ok());
    for (field, mut text) in &mut values {
        let value = collision.map(|collision| match field.0 {
            InspectorCollisionField::Width => collision.size.0,
            InspectorCollisionField::Height => collision.size.1,
            InspectorCollisionField::OffsetX => collision.offset.0,
            InspectorCollisionField::OffsetY => collision.offset.1,
        });
        text.0 = value.map_or_else(|| "-".into(), |value| format!("{value:.1} px"));
    }
    let enabled = collision.is_some_and(|collision| collision.enabled);
    for mut text in &mut enabled_labels {
        text.0 = if enabled { "On" } else { "Off" }.into();
    }
    for (mut background, mut border) in &mut enabled_indicators {
        background.0 = if enabled {
            theme::accent()
        } else {
            Color::NONE
        };
        *border = BorderColor::all(if enabled {
            theme::accent()
        } else {
            theme::border()
        });
    }
}

fn sync_collision_visual(
    mut collisions: Query<(
        &SceneCollisionRect2D,
        &mut SceneSprite2D,
        &mut Sprite,
        &mut Anchor,
        &mut Visibility,
    )>,
) {
    for (collision, mut data, mut sprite, mut anchor, mut visibility) in &mut collisions {
        let size = Vec2::new(collision.size.0.max(1.0), collision.size.1.max(1.0));
        data.image_path.clear();
        data.color = (0.92, 0.20, 0.24, 0.25);
        data.visible = collision.enabled;
        data.region_enabled = false;
        data.hframes = 1;
        data.vframes = 1;
        data.frame = 0;
        data.region_rect = (0.0, 0.0, size.x, size.y);
        data.z_index = 100;
        data.anchor = (-collision.offset.0 / size.x, -collision.offset.1 / size.y);
        sprite.image = default();
        sprite.color = Color::srgba(0.92, 0.20, 0.24, 0.25);
        sprite.custom_size = Some(size);
        sprite.rect = None;
        anchor.0 = Vec2::new(data.anchor.0 - 0.5, 0.5 - data.anchor.1);
        *visibility = if collision.enabled {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn sync_sprite_labels(
    selection: Res<Selection>,
    sprites: Query<&SceneSprite2D>,
    mut value_labels: Query<
        (&InspectorSpriteValueLabel, &mut Text),
        Without<InspectorSpriteToggleLabel>,
    >,
    mut toggle_labels: Query<
        (&InspectorSpriteToggleLabel, &mut Text),
        Without<InspectorSpriteValueLabel>,
    >,
    mut toggle_indicators: Query<
        (
            &InspectorSpriteToggleIndicator,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Without<InspectorSpriteColorSwatch>,
    >,
    mut color_swatches: Query<
        &mut BackgroundColor,
        (
            With<InspectorSpriteColorSwatch>,
            Without<InspectorSpriteToggleIndicator>,
        ),
    >,
    mut region_bodies: Query<&mut Node, With<InspectorSpriteRegionBody>>,
) {
    let sprite = selection.0.and_then(|entity| sprites.get(entity).ok());
    for (field, mut text) in &mut value_labels {
        text.0 = sprite
            .map(|sprite| format_sprite_value(field.0, sprite))
            .unwrap_or_else(|| "-".into());
    }
    for (label, mut text) in &mut toggle_labels {
        let enabled = sprite.is_some_and(|sprite| sprite_toggle_value(sprite, label.0));
        text.0 = if enabled { "On" } else { "Off" }.into();
    }
    for (indicator, mut background, mut border) in &mut toggle_indicators {
        let enabled = sprite.is_some_and(|sprite| sprite_toggle_value(sprite, indicator.0));
        background.0 = if enabled {
            theme::accent()
        } else {
            Color::NONE
        };
        *border = BorderColor::all(if enabled {
            theme::accent()
        } else {
            theme::border()
        });
    }
    let color = sprite
        .map(|sprite| {
            Color::srgba(
                sprite.color.0,
                sprite.color.1,
                sprite.color.2,
                sprite.color.3,
            )
        })
        .unwrap_or(Color::WHITE);
    for mut background in &mut color_swatches {
        background.0 = color;
    }
    for mut node in &mut region_bodies {
        node.display = if sprite.is_some_and(|sprite| sprite.region_enabled) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn sprite_toggle_value(sprite: &SceneSprite2D, kind: InspectorSpriteToggleKind) -> bool {
    match kind {
        InspectorSpriteToggleKind::Visible => sprite.visible,
        InspectorSpriteToggleKind::FlipX => sprite.flip_x,
        InspectorSpriteToggleKind::FlipY => sprite.flip_y,
        InspectorSpriteToggleKind::Region => sprite.region_enabled,
    }
}

fn format_sprite_value(field: InspectorSpriteField, sprite: &SceneSprite2D) -> String {
    let value = match field {
        InspectorSpriteField::HFrames => return sprite.hframes.max(1).to_string(),
        InspectorSpriteField::VFrames => return sprite.vframes.max(1).to_string(),
        InspectorSpriteField::Frame => return sprite.clamped_frame().to_string(),
        InspectorSpriteField::RegionX => sprite.region_rect.0,
        InspectorSpriteField::RegionY => sprite.region_rect.1,
        InspectorSpriteField::RegionWidth => sprite.region_rect.2,
        InspectorSpriteField::RegionHeight => sprite.region_rect.3,
        InspectorSpriteField::ColorR => sprite.color.0,
        InspectorSpriteField::ColorG => sprite.color.1,
        InspectorSpriteField::ColorB => sprite.color.2,
        InspectorSpriteField::ColorA => sprite.color.3,
        InspectorSpriteField::AnchorX => sprite.anchor.0,
        InspectorSpriteField::AnchorY => sprite.anchor.1,
        InspectorSpriteField::ZIndex => return sprite.z_index.to_string(),
    };
    match field {
        InspectorSpriteField::RegionX
        | InspectorSpriteField::RegionY
        | InspectorSpriteField::RegionWidth
        | InspectorSpriteField::RegionHeight => format!("{value:.1} px"),
        InspectorSpriteField::ColorR
        | InspectorSpriteField::ColorG
        | InspectorSpriteField::ColorB
        | InspectorSpriteField::ColorA
        | InspectorSpriteField::AnchorX
        | InspectorSpriteField::AnchorY => format!("{value:.1}"),
        InspectorSpriteField::HFrames
        | InspectorSpriteField::VFrames
        | InspectorSpriteField::Frame
        | InspectorSpriteField::ZIndex => unreachable!(),
    }
}

fn ui_action_button<M: Component + Clone>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    caption: &str,
    width: f32,
) {
    parent.spawn((
        Button,
        WidgetButton,
        marker,
        Node {
            width: Val::Px(width),
            height: Val::Px(25.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(theme::bg_field()),
        BorderColor::all(theme::border_soft()),
        children![(
            Text::new(caption),
            TextFont {
                font_size: FontSize::Px(10.5),
                ..default()
            },
            TextColor(theme::text_primary()),
        )],
    ));
}

fn ui_numeric_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    field: InspectorUiField,
    step: f32,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(90.0),
                    ..default()
                },
                children![(
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(11.5),
                        ..default()
                    },
                    TextColor(theme::text_muted()),
                )],
            ));
            ui_nudge_button(row, field, -step, "-");
            row.spawn((
                InspectorUiValueLabel(field),
                Node {
                    width: Val::Px(72.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                children![(
                    Text::new("0.000"),
                    TextFont {
                        font_size: FontSize::Px(11.5),
                        ..default()
                    },
                    TextColor(theme::text_primary()),
                )],
            ));
            ui_nudge_button(row, field, step, "+");
        });
}

fn ui_nudge_button(
    parent: &mut ChildSpawnerCommands,
    field: InspectorUiField,
    delta: f32,
    caption: &str,
) {
    parent.spawn((
        Button,
        WidgetButton,
        InspectorUiNudge { field, delta },
        Node {
            width: Val::Px(28.0),
            height: Val::Px(22.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(theme::bg_panel()),
        BorderColor::all(theme::border()),
        children![(
            Text::new(caption),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(theme::text_primary()),
        )],
    ));
}

#[derive(Clone, Copy, Debug, Default)]
struct InspectorEntityFeatures {
    transform: bool,
    visibility: bool,
    collision_2d: bool,
    sprite: bool,
    camera_2d: bool,
    mesh_3d: bool,
    camera_3d: bool,
    directional_light: bool,
    point_light: bool,
    spot_light: bool,
    ui_layout: bool,
    ui_content: bool,
    animation_player: bool,
}

impl InspectorEntityFeatures {
    fn component_matches(self, kind: InspectorComponentKind) -> bool {
        match kind {
            InspectorComponentKind::Visibility => self.visibility,
            InspectorComponentKind::CollisionRect2D => self.collision_2d,
            InspectorComponentKind::Sprite => self.sprite,
            InspectorComponentKind::Camera2D => self.camera_2d,
            InspectorComponentKind::Mesh3D => self.mesh_3d,
            InspectorComponentKind::Camera3D => self.camera_3d,
            InspectorComponentKind::DirectionalLight => self.directional_light,
            InspectorComponentKind::PointLight => self.point_light,
            InspectorComponentKind::SpotLight => self.spot_light,
        }
    }

    fn system_matches(self, kind: InspectorSystemKind) -> bool {
        match kind {
            InspectorSystemKind::TransformPropagation => self.transform,
            InspectorSystemKind::VisibilityPropagation => self.visibility,
            InspectorSystemKind::SpriteRender => self.sprite && self.transform,
            InspectorSystemKind::Camera2D => self.camera_2d,
            InspectorSystemKind::MeshRender => self.mesh_3d && self.transform,
            InspectorSystemKind::Camera3D => self.camera_3d,
            InspectorSystemKind::LightManagement => {
                self.directional_light || self.point_light || self.spot_light
            }
            InspectorSystemKind::UiLayout => self.ui_layout,
            InspectorSystemKind::UiRender => self.ui_content,
        }
    }

    fn component_count(self) -> usize {
        usize::from(self.transform)
            + usize::from(self.ui_layout)
            + usize::from(self.ui_content)
            + usize::from(self.animation_player)
            + InspectorComponentKind::ALL
                .into_iter()
                .filter(|kind| self.component_matches(*kind))
                .count()
    }
}

#[allow(clippy::type_complexity)]
fn sync_ecs_layout(
    selection: Res<Selection>,
    details: Query<(
        Has<Transform>,
        Has<Visibility>,
        Has<Sprite>,
        Has<SceneCollisionRect2D>,
        Has<Camera2d>,
        Has<Mesh3d>,
        Has<SceneModel3D>,
        Has<Camera3d>,
        Has<DirectionalLight>,
        Has<PointLight>,
        Has<SpotLight>,
        Has<SceneUiLayout>,
        Has<SceneUiContent>,
        Has<SceneAnimationPlayer>,
    )>,
    state: Res<InspectorEcsUiState>,
    component_groups: Res<InspectorComponentGroupState>,
    system_ui: Res<InspectorExplicitSystemsUiState>,
    registry: Res<InspectorSystemRegistry>,
    explicit_bindings: Query<&EntitySystemBindings>,
    custom_components: Query<&EntityCustomComponents>,
    mut nodes: Query<
        (
            &mut Node,
            Has<InspectorComponentsSection>,
            Has<InspectorSystemsSection>,
            Has<InspectorComponentsBody>,
            Has<InspectorSystemsBody>,
            Has<InspectorAnimationSection>,
            Option<&InspectorComponentSummary>,
            Option<&InspectorSystemCard>,
            Option<&InspectorSystemBody>,
            Option<&InspectorSystemGroupBody>,
            Option<&InspectorComponentGroupBody>,
            Has<InspectorNoMatchingSystems>,
        ),
        Or<(
            With<InspectorComponentsSection>,
            With<InspectorSystemsSection>,
            With<InspectorComponentsBody>,
            With<InspectorSystemsBody>,
            With<InspectorAnimationSection>,
            With<InspectorComponentSummary>,
            With<InspectorSystemCard>,
            With<InspectorSystemBody>,
            With<InspectorSystemGroupBody>,
            With<InspectorComponentGroupBody>,
            With<InspectorNoMatchingSystems>,
        )>,
    >,
    mut labels: Query<
        (
            &mut Text,
            Has<InspectorComponentsCount>,
            Has<InspectorSystemsCount>,
            Has<InspectorComponentsChevron>,
            Has<InspectorSystemsChevron>,
            Option<&InspectorSystemChevron>,
            Option<&InspectorSystemGroupChevron>,
            Option<&InspectorSystemGroupCount>,
            Option<&InspectorComponentGroupChevron>,
            Option<&InspectorComponentGroupCount>,
            Option<&InspectorTabCount>,
        ),
        Or<(
            With<InspectorComponentsCount>,
            With<InspectorSystemsCount>,
            With<InspectorComponentsChevron>,
            With<InspectorSystemsChevron>,
            With<InspectorSystemChevron>,
            With<InspectorSystemGroupChevron>,
            With<InspectorSystemGroupCount>,
            With<InspectorComponentGroupChevron>,
            With<InspectorComponentGroupCount>,
            With<InspectorTabCount>,
        )>,
    >,
) {
    let features = selection.0.and_then(|entity| details.get(entity).ok()).map(
        |(
            transform,
            visibility,
            sprite,
            collision_2d,
            camera_2d,
            mesh_3d,
            model_3d,
            camera_3d,
            directional_light,
            point_light,
            spot_light,
            ui_layout,
            ui_content,
            animation_player,
        )| InspectorEntityFeatures {
            transform,
            visibility,
            collision_2d,
            sprite: sprite && !collision_2d,
            camera_2d,
            mesh_3d: mesh_3d || model_3d,
            camera_3d,
            directional_light,
            point_light,
            spot_light,
            ui_layout,
            ui_content,
            animation_player,
        },
    );
    let has_selection = features.is_some();
    let features = features.unwrap_or_default();
    let show_components_tab = state.active_tab == InspectorTabKind::Components;
    let show_systems_tab = state.active_tab == InspectorTabKind::Systems;
    let custom_count = selection
        .0
        .and_then(|entity| custom_components.get(entity).ok())
        .map_or(0, |components| components.0.len());

    for (
        mut node,
        components_section,
        systems_section,
        components_body,
        systems_body,
        animation_section,
        component,
        system_card,
        system_body,
        system_group_body,
        component_group_body,
        no_matching_systems,
    ) in &mut nodes
    {
        let visible = if components_section || systems_section {
            has_selection
                && ((components_section && show_components_tab)
                    || (systems_section && show_systems_tab))
        } else if animation_section {
            has_selection
                && show_components_tab
                && !state.components_collapsed
                && features.animation_player
        } else if components_body {
            has_selection && show_components_tab && !state.components_collapsed
        } else if systems_body {
            has_selection && show_systems_tab
        } else if let Some(group) = component_group_body {
            let collapsed = match group.0 {
                InspectorComponentGroupKind::Required => component_groups.required_collapsed,
                InspectorComponentGroupKind::BuiltIn => component_groups.builtin_collapsed,
                InspectorComponentGroupKind::Custom => component_groups.custom_collapsed,
            };
            has_selection && show_components_tab && !collapsed
        } else if let Some(group) = system_group_body {
            let collapsed = match group.0 {
                InspectorSystemGroupKind::AutoMatch => system_ui.auto_collapsed,
                InspectorSystemGroupKind::ExplicitBindings => system_ui.explicit_collapsed,
            };
            has_selection && show_systems_tab && !collapsed
        } else if let Some(component) = component {
            has_selection && show_components_tab && features.component_matches(component.0)
        } else if let Some(system) = system_card {
            has_selection
                && show_systems_tab
                && registry.contains(system.0)
                && features.system_matches(system.0)
        } else if let Some(system) = system_body {
            has_selection
                && show_systems_tab
                && registry.contains(system.0)
                && features.system_matches(system.0)
                && state.expanded_system == Some(system.0)
        } else if no_matching_systems {
            has_selection && show_systems_tab && registry.matching_count(features) == 0
        } else {
            false
        };
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }

    for (
        mut text,
        components_count,
        systems_count,
        components_arrow,
        systems_arrow,
        system_arrow,
        system_group_arrow,
        system_group_count,
        component_group_arrow,
        component_group_count,
        tab_count,
    ) in &mut labels
    {
        if components_count {
            text.0 = (features.component_count() + custom_count).to_string();
        } else if systems_count {
            let explicit_count = selection
                .0
                .and_then(|entity| explicit_bindings.get(entity).ok())
                .map_or(0, |bindings| bindings.0.len());
            text.0 = (registry.matching_count(features) + explicit_count).to_string();
        } else if components_arrow {
            text.0 = if state.components_collapsed { ">" } else { "v" }.into();
        } else if systems_arrow {
            text.0 = String::new();
        } else if let Some(system) = system_arrow {
            text.0 = if state.expanded_system == Some(system.0) {
                "v"
            } else {
                ">"
            }
            .into();
        } else if let Some(group) = system_group_arrow {
            let collapsed = match group.0 {
                InspectorSystemGroupKind::AutoMatch => system_ui.auto_collapsed,
                InspectorSystemGroupKind::ExplicitBindings => system_ui.explicit_collapsed,
            };
            text.0 = if collapsed { ">" } else { "v" }.into();
        } else if let Some(group) = system_group_count {
            text.0 = match group.0 {
                InspectorSystemGroupKind::AutoMatch => registry.matching_count(features),
                InspectorSystemGroupKind::ExplicitBindings => selection
                    .0
                    .and_then(|entity| explicit_bindings.get(entity).ok())
                    .map_or(0, |bindings| bindings.0.len()),
            }
            .to_string();
        } else if let Some(group) = component_group_arrow {
            let collapsed = match group.0 {
                InspectorComponentGroupKind::Required => component_groups.required_collapsed,
                InspectorComponentGroupKind::BuiltIn => component_groups.builtin_collapsed,
                InspectorComponentGroupKind::Custom => component_groups.custom_collapsed,
            };
            text.0 = if collapsed { ">" } else { "v" }.into();
        } else if let Some(group) = component_group_count {
            text.0 = match group.0 {
                InspectorComponentGroupKind::Required => {
                    usize::from(features.transform) + usize::from(features.visibility)
                }
                InspectorComponentGroupKind::BuiltIn => features
                    .component_count()
                    .saturating_sub(usize::from(features.transform))
                    .saturating_sub(usize::from(features.visibility)),
                InspectorComponentGroupKind::Custom => custom_count,
            }
            .to_string();
        } else if let Some(tab) = tab_count {
            text.0 = match tab.0 {
                InspectorTabKind::Components => features.component_count() + custom_count,
                InspectorTabKind::Systems => {
                    let explicit_count = selection
                        .0
                        .and_then(|entity| explicit_bindings.get(entity).ok())
                        .map_or(0, |bindings| bindings.0.len());
                    registry.matching_count(features) + explicit_count
                }
            }
            .to_string();
        }
    }
}

fn sync_component_ownership(
    selection: Res<Selection>,
    authored_components: Query<&AddedEntityComponents>,
    models: Query<(), With<SceneModel3D>>,
    mut statuses: Query<(&InspectorComponentStatus, &mut Text)>,
    mut remove_buttons: Query<(&InspectorRemoveComponentButton, &mut Node)>,
) {
    let authored = selection
        .0
        .and_then(|entity| authored_components.get(entity).ok());

    for (status, mut text) in &mut statuses {
        let next = match status.0.builtin() {
            None if status.0 == InspectorComponentKind::CollisionRect2D => "Preset",
            Some(BuiltinComponent::Mesh3D)
                if selection.0.is_some_and(|entity| models.contains(entity)) =>
            {
                "Model"
            }
            None => "Required",
            Some(component)
                if authored.is_some_and(|components| components.0.contains(&component)) =>
            {
                "Added"
            }
            Some(_) => "Preset",
        };
        if text.0 != next {
            text.0 = next.into();
        }
    }

    for (button, mut node) in &mut remove_buttons {
        let removable = button.0 != BuiltinComponent::Transform
            && authored.is_some_and(|components| components.0.contains(&button.0));
        node.display = if removable {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn sync_inspector_section_visibility(
    selection: Res<Selection>,
    details: Query<(
        Has<Transform>,
        Has<SceneUiLayout>,
        Has<SceneSprite2D>,
        Has<SceneCollisionRect2D>,
        Has<Mesh3d>,
        Has<SceneModel3D>,
        Option<&EntityKind>,
        Option<&SceneSpace>,
    )>,
    mut nodes: Query<
        (
            &mut Node,
            Has<InspectorTransformSection>,
            Has<InspectorUiSection>,
            Has<InspectorSpriteSection>,
            Has<InspectorCollisionSection>,
            Has<InspectorMesh3dSection>,
            Option<&InspectorTransformSpace>,
            Option<&InspectorUiRow>,
            Has<InspectorEntityHeader>,
            Has<InspectorAddComponentButton>,
        ),
        Or<(
            With<InspectorTransformSection>,
            With<InspectorUiSection>,
            With<InspectorSpriteSection>,
            With<InspectorCollisionSection>,
            With<InspectorMesh3dSection>,
            With<InspectorTransformSpace>,
            With<InspectorUiRow>,
            With<InspectorEntityHeader>,
            With<InspectorAddComponentButton>,
        )>,
    >,
) {
    let selected = selection.0.and_then(|entity| details.get(entity).ok());
    let has_selection = selected.is_some();
    let has_transform = selected.is_some_and(|(transform, _, _, _, _, _, _, _)| transform);
    let has_ui = selected.is_some_and(|(_, ui, _, _, _, _, _, _)| ui);
    let has_collision = selected.is_some_and(|(_, _, _, collision, _, _, _, _)| collision);
    let has_sprite =
        selected.is_some_and(|(_, _, sprite, collision, _, _, _, _)| sprite && !collision);
    let has_mesh3d = selected.is_some_and(|(_, _, _, _, mesh, model, _, _)| mesh || model);
    let kind = selected.and_then(|(_, _, _, _, _, _, kind, _)| kind.copied());
    let scene_space = selected.and_then(|(_, _, _, _, _, _, _, space)| space.copied());

    for (
        mut node,
        transform_section,
        ui_section,
        sprite_section,
        collision_section,
        mesh3d_section,
        space,
        ui_row,
        entity_header,
        add_button,
    ) in &mut nodes
    {
        let visible = if transform_section {
            has_transform
        } else if ui_section {
            has_ui
        } else if sprite_section {
            has_sprite
        } else if collision_section {
            has_collision
        } else if mesh3d_section {
            has_mesh3d && scene_space == Some(SceneSpace::ThreeD)
        } else if let Some(space) = space {
            has_transform && scene_space == Some(space.0)
        } else if let Some(row) = ui_row {
            has_ui
                && match row.0 {
                    InspectorUiRowKind::Text => {
                        matches!(kind, Some(EntityKind::Text | EntityKind::Button))
                    }
                    InspectorUiRowKind::PanelColor => matches!(kind, Some(EntityKind::Panel)),
                    InspectorUiRowKind::Image => matches!(kind, Some(EntityKind::Image)),
                }
        } else if entity_header || add_button {
            has_selection
        } else {
            true
        };
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn sync_ui_input_binding(
    selection: Res<Selection>,
    ids: Query<&SceneNodeId>,
    contents: Query<&SceneUiContent>,
    mut inputs: Query<(
        &mut EditableText,
        Has<InspectorUiTextInput>,
        Has<InspectorUiImageInput>,
    )>,
    mut binding: ResMut<InspectorUiInputBinding>,
) {
    let next_id = selection.0.and_then(|entity| ids.get(entity).ok()).copied();
    if binding.bound == next_id && !selection.is_changed() {
        binding.suppress_changes = false;
        return;
    }

    let content = selection.0.and_then(|entity| contents.get(entity).ok());
    for (mut input, is_text, is_image) in &mut inputs {
        let next = if is_text {
            content.map(|value| value.text.as_str()).unwrap_or("")
        } else if is_image {
            content.map(|value| value.image_path.as_str()).unwrap_or("")
        } else {
            continue;
        };
        if input.value().to_string() != next {
            input.editor_mut().set_text(next);
        }
    }
    binding.bound = next_id;
    binding.suppress_changes = true;
}

fn sync_sprite_input_binding(
    selection: Res<Selection>,
    ids: Query<&SceneNodeId>,
    sprites: Query<&SceneSprite2D>,
    mut inputs: Query<&mut EditableText, With<InspectorSpriteImageInput>>,
    mut binding: ResMut<InspectorSpriteInputBinding>,
) {
    let next_id = selection.0.and_then(|entity| ids.get(entity).ok()).copied();
    if binding.bound == next_id && !selection.is_changed() {
        binding.suppress_changes = false;
        return;
    }

    let next = selection
        .0
        .and_then(|entity| sprites.get(entity).ok())
        .map(|sprite| sprite.image_path.as_str())
        .unwrap_or("");
    for mut input in &mut inputs {
        if input.value().to_string() != next {
            input.editor_mut().set_text(next);
        }
    }
    binding.bound = next_id;
    binding.suppress_changes = true;
}

fn sync_mesh3d_input_binding(
    selection: Res<Selection>,
    ids: Query<&SceneNodeId>,
    models: Query<&SceneModel3D>,
    mut inputs: Query<&mut EditableText, With<InspectorMesh3dInput>>,
    mut binding: ResMut<InspectorMesh3dInputBinding>,
) {
    let next_id = selection.0.and_then(|entity| ids.get(entity).ok()).copied();
    if binding.bound == next_id && !selection.is_changed() {
        binding.suppress_changes = false;
        return;
    }

    let next = selection
        .0
        .and_then(|entity| models.get(entity).ok())
        .map(|model| model.resource_path.as_str())
        .unwrap_or("");
    for mut input in &mut inputs {
        if input.value().to_string() != next {
            input.editor_mut().set_text(next);
        }
    }
    binding.bound = next_id;
    binding.suppress_changes = true;
}

fn apply_ui_text_edits(
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    binding: Res<InspectorUiInputBinding>,
    text_inputs: Query<&EditableText, (With<InspectorUiTextInput>, Changed<EditableText>)>,
    image_inputs: Query<&EditableText, (With<InspectorUiImageInput>, Changed<EditableText>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<&SceneUiContent>,
        Query<&mut SceneUiContent>,
    )>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if binding.suppress_changes {
        return;
    }
    let Some(entity) = selection.0 else { return };
    let text = text_inputs
        .iter()
        .next()
        .map(|value| value.value().to_string());
    let image_path = image_inputs
        .iter()
        .next()
        .map(|value| value.value().to_string());
    if text.is_none() && image_path.is_none() {
        return;
    }
    let changed = nodes.p1().get(entity).is_ok_and(|content| {
        text.as_ref().is_some_and(|value| content.text != *value)
            || image_path
                .as_ref()
                .is_some_and(|value| content.image_path != *value)
    });
    if !changed {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Edit UI Content", before);
    }
    let mut contents = nodes.p2();
    let Ok(mut content) = contents.get_mut(entity) else {
        return;
    };
    if let Some(text) = text {
        content.text = text;
    }
    if let Some(image_path) = image_path {
        content.image_path = image_path;
    }
    mark_document_changed(document.as_deref_mut());
}

fn apply_sprite_text_edits(
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    binding: Res<InspectorSpriteInputBinding>,
    image_inputs: Query<&EditableText, (With<InspectorSpriteImageInput>, Changed<EditableText>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<&SceneSprite2D>,
        Query<&mut SceneSprite2D>,
    )>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if binding.suppress_changes {
        return;
    }
    let Some(entity) = selection.0 else { return };
    let Some(image_path) = image_inputs
        .iter()
        .next()
        .map(|value| value.value().to_string())
    else {
        return;
    };
    if !nodes
        .p1()
        .get(entity)
        .is_ok_and(|sprite| sprite.image_path != image_path)
    {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Assign Sprite2D Texture", before);
    }
    let mut sprites = nodes.p2();
    let Ok(mut sprite) = sprites.get_mut(entity) else {
        return;
    };
    sprite.image_path = image_path;
    mark_document_changed(document.as_deref_mut());
}

fn apply_mesh3d_text_edits(
    mut commands: Commands,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    binding: Res<InspectorMesh3dInputBinding>,
    inputs: Query<&EditableText, (With<InspectorMesh3dInput>, Changed<EditableText>)>,
    mut history: Option<ResMut<SceneHistory>>,
    nodes: Query<SceneSnapshotQuery>,
    current_models: Query<&SceneModel3D>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if binding.suppress_changes {
        return;
    }
    let Some(entity) = selection.0 else { return };
    let Some(raw_path) = inputs.iter().next().map(|value| value.value().to_string()) else {
        return;
    };
    let next_path = if raw_path.trim().is_empty() {
        String::new()
    } else if let Some(path) = scene_model_resource_path(&raw_path) {
        path
    } else {
        return;
    };
    let current_path = current_models
        .get(entity)
        .map(|model| model.resource_path.as_str())
        .unwrap_or("");
    if current_path == next_path {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        history.begin(
            "Assign Mesh3D Model",
            capture_scene_snapshot(&nodes, &selection, *mode),
        );
    }
    if next_path.is_empty() {
        commands
            .entity(entity)
            .remove::<SceneModel3D>()
            .remove::<WorldAssetRoot>()
            .remove::<NeedsModel3dFocus>()
            .insert(NeedsDefaultMesh3D);
    } else {
        commands.entity(entity).insert(SceneModel3D {
            resource_path: next_path,
        });
    }
    mark_document_changed(document.as_deref_mut());
}

fn sync_scene_sprite_render(
    asset_server: Option<Res<AssetServer>>,
    images: Option<Res<Assets<Image>>>,
    mut sprites: Query<(
        &SceneSprite2D,
        &mut Sprite,
        &mut Anchor,
        &mut Visibility,
        &mut Transform,
    )>,
) {
    for (data, mut sprite, mut anchor, mut visibility, mut transform) in &mut sprites {
        let image_path = scene_image_asset_path(&data.image_path);
        if let Some(asset_server) = asset_server.as_deref() {
            sprite.image = image_path
                .as_ref()
                .map(|path| asset_server.load(format!("project://{path}")))
                .unwrap_or_default();
        } else if image_path.is_none() {
            sprite.image = default();
        }
        sprite.color = Color::srgba(data.color.0, data.color.1, data.color.2, data.color.3);
        sprite.flip_x = data.flip_x;
        sprite.flip_y = data.flip_y;
        let texture_size = images.as_deref().and_then(|images| {
            images.get(&sprite.image).map(|image| {
                Vec2::new(
                    image.texture_descriptor.size.width as f32,
                    image.texture_descriptor.size.height as f32,
                )
            })
        });
        sprite.rect = texture_size
            .and_then(|size| scene_sprite_frame_rect(data, size))
            .or_else(|| scene_sprite_rect(data));
        sprite.custom_size = image_path.is_none().then_some(
            sprite
                .rect
                .map(|rect| rect.size())
                .unwrap_or(Vec2::splat(64.0)),
        );
        anchor.0 = Vec2::new(
            data.anchor.0.clamp(0.0, 1.0) - 0.5,
            0.5 - data.anchor.1.clamp(0.0, 1.0),
        );
        *visibility = if data.visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        transform.translation.z = data.z_index as f32;
    }
}

fn insert_sprite_frame_animation_key(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorSpriteFrameKey>>,
    mut commands: Commands,
) {
    if buttons.get(activate.entity).is_ok() {
        commands.trigger(InsertSpriteFrameKey);
    }
}

fn sync_ui_image_preview(
    selection: Res<Selection>,
    contents: Query<&SceneUiContent>,
    asset_server: Option<Res<AssetServer>>,
    mut previews: Query<&mut ImageNode, With<InspectorUiImagePreview>>,
    mut name_labels: Query<
        &mut Text,
        (
            With<InspectorUiImageNameLabel>,
            Without<InspectorUiImagePathLabel>,
        ),
    >,
    mut path_labels: Query<
        &mut Text,
        (
            With<InspectorUiImagePathLabel>,
            Without<InspectorUiImageNameLabel>,
        ),
    >,
    mut clear_buttons: Query<&mut Node, With<InspectorUiImageClearButton>>,
) {
    let raw_path = selection
        .0
        .and_then(|entity| contents.get(entity).ok())
        .map(|content| content.image_path.trim())
        .unwrap_or("");
    let asset_path = scene_image_asset_path(raw_path);
    let project_asset_path = asset_path.as_ref().map(|path| format!("project://{path}"));

    if let Some(asset_server) = asset_server.as_deref() {
        for mut preview in &mut previews {
            let next = project_asset_path
                .as_ref()
                .map(|path| asset_server.load(path.clone()))
                .unwrap_or_else(|| ImageNode::default().image);
            if preview.image != next {
                preview.image = next;
            }
        }
    }

    let name = asset_path
        .as_deref()
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .unwrap_or(if raw_path.is_empty() {
            "No texture assigned"
        } else {
            "Invalid texture path"
        });
    for mut text in &mut name_labels {
        if text.0 != name {
            text.0 = name.into();
        }
    }
    let path_text = if raw_path.is_empty() {
        "Drag an image from FileSystem"
    } else {
        raw_path
    };
    for mut text in &mut path_labels {
        if text.0 != path_text {
            text.0 = path_text.into();
        }
    }
    for mut node in &mut clear_buttons {
        node.display = if raw_path.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
    }
}

fn sync_sprite_image_preview(
    selection: Res<Selection>,
    sprites: Query<&SceneSprite2D>,
    asset_server: Option<Res<AssetServer>>,
    mut previews: Query<&mut ImageNode, With<InspectorSpriteImagePreview>>,
    mut name_labels: Query<
        &mut Text,
        (
            With<InspectorSpriteImageNameLabel>,
            Without<InspectorSpriteImagePathLabel>,
        ),
    >,
    mut path_labels: Query<
        &mut Text,
        (
            With<InspectorSpriteImagePathLabel>,
            Without<InspectorSpriteImageNameLabel>,
        ),
    >,
    mut clear_buttons: Query<&mut Node, With<InspectorSpriteImageClearButton>>,
) {
    let raw_path = selection
        .0
        .and_then(|entity| sprites.get(entity).ok())
        .map(|sprite| sprite.image_path.trim())
        .unwrap_or("");
    let asset_path = scene_image_asset_path(raw_path);
    let project_asset_path = asset_path.as_ref().map(|path| format!("project://{path}"));

    if let Some(asset_server) = asset_server.as_deref() {
        for mut preview in &mut previews {
            let next = project_asset_path
                .as_ref()
                .map(|path| asset_server.load(path.clone()))
                .unwrap_or_else(|| ImageNode::default().image);
            if preview.image != next {
                preview.image = next;
            }
        }
    }

    let name = asset_path
        .as_deref()
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .unwrap_or(if raw_path.is_empty() {
            "No texture assigned"
        } else {
            "Invalid texture path"
        });
    for mut text in &mut name_labels {
        if text.0 != name {
            text.0 = name.into();
        }
    }
    let path_text = if raw_path.is_empty() {
        "Drag an image from FileSystem"
    } else {
        raw_path
    };
    for mut text in &mut path_labels {
        if text.0 != path_text {
            text.0 = path_text.into();
        }
    }
    for mut node in &mut clear_buttons {
        node.display = if raw_path.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
    }
}

fn sync_mesh3d_labels(
    selection: Res<Selection>,
    models: Query<&SceneModel3D>,
    mut name_labels: Query<
        &mut Text,
        (
            With<InspectorMesh3dNameLabel>,
            Without<InspectorMesh3dPathLabel>,
        ),
    >,
    mut path_labels: Query<
        &mut Text,
        (
            With<InspectorMesh3dPathLabel>,
            Without<InspectorMesh3dNameLabel>,
        ),
    >,
    mut clear_buttons: Query<&mut Node, With<InspectorMesh3dClearButton>>,
) {
    let raw_path = selection
        .0
        .and_then(|entity| models.get(entity).ok())
        .map(|model| model.resource_path.trim())
        .unwrap_or("");
    let name = raw_path
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("Default Cube");
    for mut text in &mut name_labels {
        if text.0 != name {
            text.0 = name.into();
        }
    }
    let path_text = if raw_path.is_empty() {
        "Drag a GLB, GLTF, or FBX from FileSystem"
    } else {
        raw_path
    };
    for mut text in &mut path_labels {
        if text.0 != path_text {
            text.0 = path_text.into();
        }
    }
    for mut node in &mut clear_buttons {
        node.display = if raw_path.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
    }
}

fn toggle_ui_transform_section(
    activate: On<Activate>,
    toggles: Query<(), With<InspectorUiTransformToggle>>,
    mut state: ResMut<InspectorUiLayoutState>,
) {
    if toggles.get(activate.entity).is_ok() {
        state.transform_collapsed = !state.transform_collapsed;
    }
}

fn toggle_anchor_dropdown(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorAnchorDropdownButton>>,
    mut state: ResMut<InspectorUiLayoutState>,
) {
    if buttons.get(activate.entity).is_ok() {
        state.anchor_menu_open = !state.anchor_menu_open;
    }
}

fn toggle_ui_scale_link(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorUiScaleLinkButton>>,
    mut state: ResMut<InspectorUiLayoutState>,
) {
    if buttons.get(activate.entity).is_ok() {
        state.scale_linked = !state.scale_linked;
    }
}

fn toggle_ui_clip(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorUiClipButton>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneUiLayout>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Toggle UI Clip Contents", before);
    }
    let mut layouts = nodes.p1();
    let Ok(mut layout) = layouts.get_mut(entity) else {
        return;
    };
    layout.clip_contents = !layout.clip_contents;
    mark_document_changed(document.as_deref_mut());
}

fn insert_ui_transform_animation_key(
    activate: On<Activate>,
    buttons: Query<&InspectorUiTransformKey>,
    mut commands: Commands,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    commands.trigger(InsertAnimationPropertyKey { property: button.0 });
}

fn reset_ui_transform_group(
    activate: On<Activate>,
    resets: Query<&InspectorUiTransformReset>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    kinds: Query<&EntityKind>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneUiLayout>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(reset) = resets.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let Ok(kind) = kinds.get(entity) else { return };
    let defaults = kind.default_ui_layout().unwrap_or_default();
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Reset UI Transform", before);
    }
    let mut layouts = nodes.p1();
    let Ok(mut layout) = layouts.get_mut(entity) else {
        return;
    };
    let previous = *layout;
    match reset.0 {
        InspectorUiTransformGroup::Size => layout.size = defaults.size,
        InspectorUiTransformGroup::Position => layout.offset = (0.0, 0.0),
        InspectorUiTransformGroup::Rotation => layout.rotation = 0.0,
        InspectorUiTransformGroup::Scale => layout.scale = (1.0, 1.0),
        InspectorUiTransformGroup::PivotOffset => layout.pivot_offset = (0.0, 0.0),
        InspectorUiTransformGroup::PivotRatio => layout.pivot_ratio = (0.0, 0.0),
        InspectorUiTransformGroup::MinimumSize => layout.minimum_size = (0.0, 0.0),
        InspectorUiTransformGroup::Offsets => layout.margin = (0.0, 0.0, 0.0, 0.0),
    }
    if *layout != previous {
        mark_document_changed(document.as_deref_mut());
    }
}

fn apply_ui_nudge(
    activate: On<Activate>,
    nudges: Query<&InspectorUiNudge>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    state: Res<InspectorUiLayoutState>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(
        Query<SceneSnapshotQuery>,
        Query<(&mut SceneUiLayout, &mut SceneUiContent)>,
    )>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(nudge) = nudges.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Edit UI Properties", before);
    }
    let mut ui_nodes = nodes.p1();
    let Ok((mut layout, mut content)) = ui_nodes.get_mut(entity) else {
        return;
    };
    match nudge.field {
        InspectorUiField::PositionX => {
            layout.offset.0 = round_ui_value(layout.offset.0 + nudge.delta)
        }
        InspectorUiField::PositionY => {
            layout.offset.1 = round_ui_value(layout.offset.1 + nudge.delta)
        }
        InspectorUiField::Width => {
            layout.size.0 = round_ui_value(layout.size.0 + nudge.delta).max(1.0)
        }
        InspectorUiField::Height => {
            layout.size.1 = round_ui_value(layout.size.1 + nudge.delta).max(1.0)
        }
        InspectorUiField::MinimumWidth => {
            layout.minimum_size.0 = round_ui_value(layout.minimum_size.0 + nudge.delta).max(0.0)
        }
        InspectorUiField::MinimumHeight => {
            layout.minimum_size.1 = round_ui_value(layout.minimum_size.1 + nudge.delta).max(0.0)
        }
        InspectorUiField::Rotation => {
            layout.rotation = round_ui_value(layout.rotation + nudge.delta)
        }
        InspectorUiField::ScaleX | InspectorUiField::ScaleY if state.scale_linked => {
            let current = if nudge.field == InspectorUiField::ScaleX {
                layout.scale.0
            } else {
                layout.scale.1
            };
            let value = round_ui_value(current + nudge.delta);
            layout.scale = (value, value);
        }
        InspectorUiField::ScaleX => layout.scale.0 = round_ui_value(layout.scale.0 + nudge.delta),
        InspectorUiField::ScaleY => layout.scale.1 = round_ui_value(layout.scale.1 + nudge.delta),
        InspectorUiField::PivotOffsetX => {
            layout.pivot_offset.0 = round_ui_value(layout.pivot_offset.0 + nudge.delta)
        }
        InspectorUiField::PivotOffsetY => {
            layout.pivot_offset.1 = round_ui_value(layout.pivot_offset.1 + nudge.delta)
        }
        InspectorUiField::PivotRatioX => {
            layout.pivot_ratio.0 =
                round_ui_value(layout.pivot_ratio.0 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorUiField::PivotRatioY => {
            layout.pivot_ratio.1 =
                round_ui_value(layout.pivot_ratio.1 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorUiField::MarginLeft => {
            layout.margin.0 = round_ui_value(layout.margin.0 + nudge.delta)
        }
        InspectorUiField::MarginTop => {
            layout.margin.1 = round_ui_value(layout.margin.1 + nudge.delta)
        }
        InspectorUiField::MarginRight => {
            layout.margin.2 = round_ui_value(layout.margin.2 + nudge.delta)
        }
        InspectorUiField::MarginBottom => {
            layout.margin.3 = round_ui_value(layout.margin.3 + nudge.delta)
        }
        InspectorUiField::ColorR => {
            content.panel_color.0 = (content.panel_color.0 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorUiField::ColorG => {
            content.panel_color.1 = (content.panel_color.1 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorUiField::ColorB => {
            content.panel_color.2 = (content.panel_color.2 + nudge.delta).clamp(0.0, 1.0)
        }
        InspectorUiField::ColorA => {
            content.panel_color.3 = (content.panel_color.3 + nudge.delta).clamp(0.0, 1.0)
        }
    }
    mark_document_changed(document.as_deref_mut());
}

fn round_ui_value(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

fn apply_anchor_preset(
    activate: On<Activate>,
    presets: Query<&InspectorAnchorPreset>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut state: ResMut<InspectorUiLayoutState>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneUiLayout>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(preset) = presets.get(activate.entity) else {
        return;
    };
    state.anchor_menu_open = false;
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Set UI Anchor", before);
    }
    let mut layouts = nodes.p1();
    let Ok(mut layout) = layouts.get_mut(entity) else {
        return;
    };
    (layout.anchor_min, layout.anchor_max) = preset.0.anchors();
    layout.margin = (0.0, 0.0, 0.0, 0.0);
    let size = (
        layout.size.0.max(layout.minimum_size.0),
        layout.size.1.max(layout.minimum_size.1),
    );
    layout.offset = (
        if layout.anchor_min.0 == layout.anchor_max.0 {
            -layout.anchor_min.0 * size.0
        } else {
            0.0
        },
        if layout.anchor_min.1 == layout.anchor_max.1 {
            -layout.anchor_min.1 * size.1
        } else {
            0.0
        },
    );
    mark_document_changed(document.as_deref_mut());
}

fn cycle_ui_alignment(
    activate: On<Activate>,
    buttons: Query<&InspectorAlignmentButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneUiLayout>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Set UI Alignment", before);
    }
    let mut layouts = nodes.p1();
    let Ok(mut layout) = layouts.get_mut(entity) else {
        return;
    };
    match button.0 {
        AlignmentAxis::Horizontal => {
            layout.horizontal_alignment = next_alignment(layout.horizontal_alignment)
        }
        AlignmentAxis::Vertical => {
            layout.vertical_alignment = next_alignment(layout.vertical_alignment)
        }
    }
    mark_document_changed(document.as_deref_mut());
}

const fn next_alignment(value: UiAlignment) -> UiAlignment {
    match value {
        UiAlignment::Start => UiAlignment::Center,
        UiAlignment::Center => UiAlignment::End,
        UiAlignment::End => UiAlignment::Stretch,
        UiAlignment::Stretch => UiAlignment::Start,
    }
}

const fn alignment_label(value: UiAlignment) -> &'static str {
    match value {
        UiAlignment::Start => "Start",
        UiAlignment::Center => "Center",
        UiAlignment::End => "End",
        UiAlignment::Stretch => "Stretch",
    }
}

fn mark_document_changed(document: Option<&mut SceneDocument>) {
    if let Some(document) = document {
        document.open = true;
        document.dirty = true;
        document.bump_revision();
    }
}

fn sync_ui_labels(
    selection: Res<Selection>,
    layouts: Query<&SceneUiLayout>,
    contents: Query<&SceneUiContent>,
    value_roots: Query<(&InspectorUiValueLabel, &Children)>,
    alignment_roots: Query<(&InspectorAlignmentButton, &Children)>,
    mut preset_buttons: Query<(&InspectorAnchorPreset, &mut BackgroundColor)>,
    mut texts: Query<&mut Text>,
) {
    let layout = selection.0.and_then(|entity| layouts.get(entity).ok());
    let content = selection.0.and_then(|entity| contents.get(entity).ok());
    for (label, children) in &value_roots {
        let value = match label.0 {
            InspectorUiField::PositionX => layout.map(|value| value.offset.0),
            InspectorUiField::PositionY => layout.map(|value| value.offset.1),
            InspectorUiField::Width => layout.map(|value| value.size.0),
            InspectorUiField::Height => layout.map(|value| value.size.1),
            InspectorUiField::MinimumWidth => layout.map(|value| value.minimum_size.0),
            InspectorUiField::MinimumHeight => layout.map(|value| value.minimum_size.1),
            InspectorUiField::Rotation => layout.map(|value| value.rotation),
            InspectorUiField::ScaleX => layout.map(|value| value.scale.0),
            InspectorUiField::ScaleY => layout.map(|value| value.scale.1),
            InspectorUiField::PivotOffsetX => layout.map(|value| value.pivot_offset.0),
            InspectorUiField::PivotOffsetY => layout.map(|value| value.pivot_offset.1),
            InspectorUiField::PivotRatioX => layout.map(|value| value.pivot_ratio.0),
            InspectorUiField::PivotRatioY => layout.map(|value| value.pivot_ratio.1),
            InspectorUiField::MarginLeft => layout.map(|value| value.margin.0),
            InspectorUiField::MarginTop => layout.map(|value| value.margin.1),
            InspectorUiField::MarginRight => layout.map(|value| value.margin.2),
            InspectorUiField::MarginBottom => layout.map(|value| value.margin.3),
            InspectorUiField::ColorR => content.map(|value| value.panel_color.0),
            InspectorUiField::ColorG => content.map(|value| value.panel_color.1),
            InspectorUiField::ColorB => content.map(|value| value.panel_color.2),
            InspectorUiField::ColorA => content.map(|value| value.panel_color.3),
        };
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child) {
                text.0 = value.map_or_else(|| "-".into(), |value| format_ui_value(label.0, value));
            }
        }
    }
    for (button, children) in &alignment_roots {
        let value = layout.map(|layout| match button.0 {
            AlignmentAxis::Horizontal => layout.horizontal_alignment,
            AlignmentAxis::Vertical => layout.vertical_alignment,
        });
        let prefix = match button.0 {
            AlignmentAxis::Horizontal => "H",
            AlignmentAxis::Vertical => "V",
        };
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child) {
                text.0 = value.map_or_else(
                    || format!("{prefix}: -"),
                    |value| format!("{prefix}: {}", alignment_label(value)),
                );
            }
        }
    }
    for (button, mut background) in &mut preset_buttons {
        let active = layout.is_some_and(|layout| {
            let (min, max) = button.0.anchors();
            layout.anchor_min == min && layout.anchor_max == max
        });
        background.0 = if active { theme::accent() } else { Color::NONE };
    }
}

fn format_ui_value(field: InspectorUiField, value: f32) -> String {
    match field {
        InspectorUiField::PositionX
        | InspectorUiField::PositionY
        | InspectorUiField::Width
        | InspectorUiField::Height
        | InspectorUiField::MinimumWidth
        | InspectorUiField::MinimumHeight
        | InspectorUiField::PivotOffsetX
        | InspectorUiField::PivotOffsetY
        | InspectorUiField::MarginLeft
        | InspectorUiField::MarginTop
        | InspectorUiField::MarginRight
        | InspectorUiField::MarginBottom => format!("{value:.1} px"),
        InspectorUiField::Rotation => format!("{value:.1} deg"),
        InspectorUiField::ScaleX
        | InspectorUiField::ScaleY
        | InspectorUiField::PivotRatioX
        | InspectorUiField::PivotRatioY
        | InspectorUiField::ColorR
        | InspectorUiField::ColorG
        | InspectorUiField::ColorB
        | InspectorUiField::ColorA => format!("{value:.1}"),
    }
}

fn matching_anchor_preset(layout: &SceneUiLayout) -> Option<AnchorPreset> {
    AnchorPreset::ALL.into_iter().find(|preset| {
        let (min, max) = preset.anchors();
        layout.anchor_min == min && layout.anchor_max == max
    })
}

#[allow(clippy::type_complexity)]
fn sync_ui_layout_chrome(
    selection: Res<Selection>,
    layouts: Query<&SceneUiLayout>,
    mut state: ResMut<InspectorUiLayoutState>,
    mut layout_nodes: Query<
        (
            &mut Node,
            Has<InspectorUiTransformBody>,
            Has<InspectorAnchorDropdownMenu>,
        ),
        Or<(
            With<InspectorUiTransformBody>,
            With<InspectorAnchorDropdownMenu>,
        )>,
    >,
    mut chevrons: Query<&mut Text, With<InspectorUiTransformChevron>>,
    mut anchor_labels: Query<
        &mut Text,
        (
            With<InspectorAnchorDropdownLabel>,
            Without<InspectorUiTransformChevron>,
            Without<InspectorUiClipLabel>,
        ),
    >,
    mut clip_labels: Query<
        &mut Text,
        (
            With<InspectorUiClipLabel>,
            Without<InspectorAnchorDropdownLabel>,
            Without<InspectorUiTransformChevron>,
        ),
    >,
    mut button_chrome: ParamSet<(
        Query<
            (&mut BackgroundColor, &mut BorderColor),
            (
                With<InspectorUiScaleLinkButton>,
                Without<InspectorUiClipButton>,
                Without<InspectorUiClipIndicator>,
            ),
        >,
        Query<
            (&mut BackgroundColor, &mut BorderColor),
            (
                With<InspectorUiClipButton>,
                Without<InspectorUiScaleLinkButton>,
                Without<InspectorUiClipIndicator>,
            ),
        >,
        Query<
            (&mut BackgroundColor, &mut BorderColor),
            (
                With<InspectorUiClipIndicator>,
                Without<InspectorUiScaleLinkButton>,
                Without<InspectorUiClipButton>,
            ),
        >,
    )>,
) {
    let layout = selection.0.and_then(|entity| layouts.get(entity).ok());
    if selection.is_changed() || layout.is_none() {
        state.anchor_menu_open = false;
    }
    for (mut node, transform_body, anchor_menu) in &mut layout_nodes {
        if transform_body {
            node.display = if state.transform_collapsed {
                Display::None
            } else {
                Display::Flex
            };
        } else if anchor_menu {
            node.display = if state.anchor_menu_open && layout.is_some() {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
    for mut text in &mut chevrons {
        text.0 = if state.transform_collapsed { ">" } else { "v" }.into();
    }

    let preset_label = layout
        .and_then(matching_anchor_preset)
        .map(AnchorPreset::label)
        .unwrap_or("Custom");
    for mut text in &mut anchor_labels {
        text.0 = preset_label.into();
    }

    let clip_contents = layout.is_some_and(|layout| layout.clip_contents);
    for mut text in &mut clip_labels {
        text.0 = if clip_contents { "On" } else { "Off" }.into();
    }
    for (mut background, mut border) in &mut button_chrome.p0() {
        background.0 = if state.scale_linked {
            Color::srgb(0.10, 0.20, 0.26)
        } else {
            Color::NONE
        };
        *border = BorderColor::all(if state.scale_linked {
            theme::accent()
        } else {
            Color::NONE
        });
    }
    for (mut background, mut border) in &mut button_chrome.p1() {
        background.0 = if clip_contents {
            Color::srgb(0.08, 0.16, 0.20)
        } else {
            theme::bg_field()
        };
        *border = BorderColor::all(if clip_contents {
            theme::accent()
        } else {
            theme::border_soft()
        });
    }
    for (mut background, mut border) in &mut button_chrome.p2() {
        background.0 = if clip_contents {
            theme::accent()
        } else {
            Color::NONE
        };
        *border = BorderColor::all(if clip_contents {
            theme::accent()
        } else {
            theme::border()
        });
    }
}

fn sync_inspector_labels(
    selection: Res<Selection>,
    objects: Query<&EditableObject>,
    kinds: Query<&EntityKind>,
    node_ids: Query<&SceneNodeId>,
    scripts: Query<&EntityScriptBinding>,
    asset_server: Option<Res<AssetServer>>,
    mut text_labels: ParamSet<(
        Query<&mut Text, With<InspectorNameLabel>>,
        Query<&mut Text, With<InspectorEntityIdLabel>>,
        Query<&mut Text, With<InspectorEntityScriptName>>,
    )>,
    mut entity_icon: Query<&mut ImageNode, With<InspectorEntityIcon>>,
) {
    let mut name_labels = text_labels.p0();
    let Ok(mut name_text) = name_labels.single_mut() else {
        return;
    };

    let Some(entity) = selection.0 else {
        if name_text.0 != "Name: (none)" {
            name_text.0 = "Name: (none)".into();
        }
        drop(name_text);
        drop(name_labels);
        if let Ok(mut id_text) = text_labels.p1().single_mut() {
            id_text.0 = "ID: -".into();
        }
        if let Ok(mut script_text) = text_labels.p2().single_mut() {
            script_text.0 = "<empty>".into();
        }
        return;
    };

    let name = objects
        .get(entity)
        .map(|o| o.name.as_str())
        .unwrap_or("Unknown");
    let next_name = format!("Name: {name}");
    if name_text.0 != next_name {
        name_text.0 = next_name;
    }
    drop(name_text);
    drop(name_labels);
    if let Ok(mut id_text) = text_labels.p1().single_mut() {
        let next = node_ids.get(entity).map_or_else(
            |_| "ID: -".into(),
            |id| format!("ID: {}", short_scene_id(*id)),
        );
        if id_text.0 != next {
            id_text.0 = next;
        }
    }
    if let Ok(mut script_text) = text_labels.p2().single_mut() {
        let next = scripts
            .get(entity)
            .ok()
            .and_then(|binding| binding.0.as_ref())
            .map(|script| system_script_file_name(&script.source_path))
            .unwrap_or_else(|| "<empty>".into());
        if script_text.0 != next {
            script_text.0 = next;
        }
    }

    if selection.is_changed()
        && let Some(asset_server) = asset_server.as_deref()
        && let Ok(kind) = kinds.get(entity)
        && let Ok(mut icon) = entity_icon.single_mut()
    {
        icon.image = asset_server.load(kind.icon_path());
        icon.color = kind.icon_color();
    }
}

fn short_scene_id(id: SceneNodeId) -> String {
    let value = id.to_string();
    if value.len() <= 13 {
        return value;
    }
    format!("{}...{}", &value[..8], &value[value.len() - 4..])
}

fn sync_transform_inputs(
    selection: Res<Selection>,
    transforms: Query<&Transform>,
    focus: Option<Res<InputFocus>>,
    mut inputs: Query<(Entity, &InspectorTransformInput, &mut EditableText)>,
) {
    let transform = selection.0.and_then(|entity| transforms.get(entity).ok());
    let focused = focus.as_deref().and_then(InputFocus::get);
    for (entity, input, mut text) in &mut inputs {
        if focused == Some(entity) && !selection.is_changed() {
            continue;
        }
        let value = transform
            .map(|transform| transform_field_value(transform, input.field))
            .unwrap_or(0.0);
        let formatted = format_transform_value(value);
        if text.value().to_string() != formatted {
            text.editor_mut().set_text(&formatted);
        }
    }
}

fn format_transform_value(value: f32) -> String {
    let value = if value.abs() < 0.00005 { 0.0 } else { value };
    format!("{value:.1}")
}

fn round_transform_value(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

fn apply_transform_text_edits(
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    state: Res<InspectorTransformUiState>,
    spaces: Query<&SceneSpace>,
    inputs: Query<(&InspectorTransformInput, &EditableText), Changed<EditableText>>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut Transform>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Some(entity) = selection.0 else { return };
    let Ok(space) = spaces.get(entity).copied() else {
        return;
    };
    for (input, text) in &inputs {
        if input.space != space {
            continue;
        }
        let Ok(value) = text.value().to_string().trim().parse::<f32>() else {
            continue;
        };
        if !value.is_finite() {
            continue;
        }
        let value = round_transform_value(value);
        let unchanged = nodes.p1().get(entity).is_ok_and(|transform| {
            (transform_field_value(transform, input.field) - value).abs() < 0.0001
        });
        if unchanged {
            continue;
        }
        if let Some(history) = history.as_deref_mut() {
            let before = {
                let history_nodes = nodes.p0();
                capture_scene_snapshot(&history_nodes, &selection, *mode)
            };
            history.begin("Edit Transform", before);
        }
        let mut transforms = nodes.p1();
        let Ok(mut transform) = transforms.get_mut(entity) else {
            continue;
        };
        set_transform_field(
            &mut transform,
            input.field,
            value,
            space,
            state.scale_linked,
        );
        mark_document_changed(document.as_deref_mut());
    }
}

fn transform_field_value(transform: &Transform, field: InspectorField) -> f32 {
    let euler = transform.rotation.to_euler(EulerRot::XYZ);
    match field {
        InspectorField::PosX => transform.translation.x,
        InspectorField::PosY => transform.translation.y,
        InspectorField::PosZ => transform.translation.z,
        InspectorField::RotX => euler.0.to_degrees(),
        InspectorField::RotY => euler.1.to_degrees(),
        InspectorField::RotZ => euler.2.to_degrees(),
        InspectorField::ScaleX => transform.scale.x,
        InspectorField::ScaleY => transform.scale.y,
        InspectorField::ScaleZ => transform.scale.z,
    }
}

fn toggle_ecs_section(
    activate: On<Activate>,
    component_toggles: Query<(), With<InspectorComponentsToggle>>,
    system_toggles: Query<(), With<InspectorSystemsToggle>>,
    mut state: ResMut<InspectorEcsUiState>,
) {
    if component_toggles.get(activate.entity).is_ok() {
        state.components_collapsed = !state.components_collapsed;
    } else if system_toggles.get(activate.entity).is_ok() {
        state.systems_collapsed = !state.systems_collapsed;
    }
}

fn activate_inspector_tab(
    activate: On<Activate>,
    tabs: Query<&InspectorTab>,
    mut state: ResMut<InspectorEcsUiState>,
) {
    let Ok(tab) = tabs.get(activate.entity) else {
        return;
    };
    state.active_tab = tab.0;
}

fn sync_inspector_tab_chrome(
    state: Res<InspectorEcsUiState>,
    mut tabs: Query<(&InspectorTabChrome, &mut BorderColor, &Children)>,
    mut texts: Query<&mut TextColor>,
) {
    if !state.is_changed() {
        return;
    }
    for (chrome, mut border, children) in &mut tabs {
        let active = chrome.0 == state.active_tab;
        *border = BorderColor::all(if active { theme::accent() } else { Color::NONE });
        for child in children {
            if let Ok(mut color) = texts.get_mut(*child) {
                color.0 = if active {
                    theme::text_primary()
                } else {
                    theme::text_muted()
                };
            }
        }
    }
}

fn toggle_system_card(
    activate: On<Activate>,
    toggles: Query<&InspectorSystemToggle>,
    mut state: ResMut<InspectorEcsUiState>,
) {
    let Ok(toggle) = toggles.get(activate.entity) else {
        return;
    };
    state.expanded_system = if state.expanded_system == Some(toggle.0) {
        None
    } else {
        Some(toggle.0)
    };
}

fn toggle_system_group(
    activate: On<Activate>,
    toggles: Query<&InspectorSystemGroupToggle>,
    mut state: ResMut<InspectorExplicitSystemsUiState>,
) {
    let Ok(toggle) = toggles.get(activate.entity) else {
        return;
    };
    match toggle.0 {
        InspectorSystemGroupKind::AutoMatch => state.auto_collapsed = !state.auto_collapsed,
        InspectorSystemGroupKind::ExplicitBindings => {
            state.explicit_collapsed = !state.explicit_collapsed
        }
    }
}

fn toggle_component_group(
    activate: On<Activate>,
    toggles: Query<&InspectorComponentGroupToggle>,
    mut state: ResMut<InspectorComponentGroupState>,
) {
    let Ok(toggle) = toggles.get(activate.entity) else {
        return;
    };
    match toggle.0 {
        InspectorComponentGroupKind::Required => {
            state.required_collapsed = !state.required_collapsed
        }
        InspectorComponentGroupKind::BuiltIn => state.builtin_collapsed = !state.builtin_collapsed,
        InspectorComponentGroupKind::Custom => state.custom_collapsed = !state.custom_collapsed,
    }
}

fn toggle_explicit_system_card(
    activate: On<Activate>,
    toggles: Query<&InspectorExplicitSystemToggle>,
    mut state: ResMut<InspectorExplicitSystemsUiState>,
) {
    let Ok(toggle) = toggles.get(activate.entity) else {
        return;
    };
    state.expanded_binding = if state.expanded_binding == Some(toggle.0) {
        None
    } else {
        Some(toggle.0)
    };
    state.revision = state.revision.wrapping_add(1);
}

fn add_explicit_system(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorAddSystemButton>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntitySystemBindings>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorExplicitSystemsUiState>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Add System",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut bindings = nodes.p1();
    let Ok(mut bindings) = bindings.get_mut(entity) else {
        return;
    };
    bindings.0.push(SceneSystemBinding::default());
    state.expanded_binding = Some(bindings.0.len() - 1);
    state.explicit_collapsed = false;
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn add_animation_clip(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorAddAnimationButton>>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Add Animation",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(entity) else {
        return;
    };
    let name = next_available_animation_name(&player, "Animation");
    let _ = append_animation_clip(&mut player, &name);
    mark_document_changed(document.as_deref_mut());
}

fn toggle_animation_autoplay_menu(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorAnimationAutoplayButton>>,
    mut state: ResMut<InspectorAnimationUiState>,
) {
    if buttons.get(activate.entity).is_ok() {
        state.autoplay_open = !state.autoplay_open;
    }
}

fn select_animation_autoplay(
    activate: On<Activate>,
    options: Query<&InspectorAnimationAutoplayOption>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
    mut state: ResMut<InspectorAnimationUiState>,
) {
    let Ok(option) = options.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let next = {
        let mut players = nodes.p1();
        let Ok(player) = players.get_mut(entity) else {
            return;
        };
        option
            .0
            .and_then(|index| player.clips.get(index))
            .map(|clip| clip.name.clone())
            .unwrap_or_default()
    };
    state.autoplay_open = false;
    let changed = {
        let mut players = nodes.p1();
        players
            .get_mut(entity)
            .is_ok_and(|player| player.autoplay != next)
    };
    if !changed {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Change Animation Autoplay",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(entity) else {
        return;
    };
    player.autoplay = next;
    mark_document_changed(document.as_deref_mut());
}

fn toggle_animation_clip_loop(
    activate: On<Activate>,
    buttons: Query<&InspectorAnimationClipLoopButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let exists = {
        let mut players = nodes.p1();
        players
            .get_mut(entity)
            .is_ok_and(|player| button.0 < player.clips.len())
    };
    if !exists {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Toggle Animation Loop",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(entity) else {
        return;
    };
    player.clips[button.0].looped = !player.clips[button.0].looped;
    mark_document_changed(document.as_deref_mut());
}

fn remove_animation_clip(
    activate: On<Activate>,
    buttons: Query<&InspectorRemoveAnimationButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut SceneAnimationPlayer>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let exists = {
        let mut players = nodes.p1();
        players
            .get_mut(entity)
            .is_ok_and(|player| button.0 < player.clips.len())
    };
    if !exists {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Remove Animation",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut players = nodes.p1();
    let Ok(mut player) = players.get_mut(entity) else {
        return;
    };
    let removed = player.clips.remove(button.0);
    if player.autoplay == removed.name {
        player.autoplay.clear();
    }
    mark_document_changed(document.as_deref_mut());
}

fn remove_explicit_system(
    activate: On<Activate>,
    buttons: Query<&InspectorRemoveSystemButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntitySystemBindings>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorExplicitSystemsUiState>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let can_remove = {
        let mut bindings = nodes.p1();
        bindings
            .get_mut(entity)
            .is_ok_and(|bindings| button.0 < bindings.0.len())
    };
    if !can_remove {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Remove System",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut bindings = nodes.p1();
    let Ok(mut bindings) = bindings.get_mut(entity) else {
        return;
    };
    bindings.0.remove(button.0);
    state.expanded_binding = None;
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn cycle_explicit_system_schedule(
    activate: On<Activate>,
    buttons: Query<&InspectorSystemScheduleButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntitySystemBindings>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorExplicitSystemsUiState>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let exists = {
        let mut bindings = nodes.p1();
        bindings
            .get_mut(entity)
            .ok()
            .is_some_and(|bindings| button.0 < bindings.0.len())
    };
    if !exists {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Change System Schedule",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut bindings = nodes.p1();
    let Ok(mut bindings) = bindings.get_mut(entity) else {
        return;
    };
    let Some(binding) = bindings.0.get_mut(button.0) else {
        return;
    };
    binding.schedule = binding.schedule.next();
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn toggle_explicit_system_enabled(
    activate: On<Activate>,
    buttons: Query<&InspectorSystemEnabledButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntitySystemBindings>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorExplicitSystemsUiState>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let exists = {
        let mut bindings = nodes.p1();
        bindings
            .get_mut(entity)
            .ok()
            .is_some_and(|bindings| button.0 < bindings.0.len())
    };
    if !exists {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Toggle System",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut bindings = nodes.p1();
    let Ok(mut bindings) = bindings.get_mut(entity) else {
        return;
    };
    let Some(binding) = bindings.0.get_mut(button.0) else {
        return;
    };
    binding.enabled = !binding.enabled;
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn cycle_explicit_system_order(
    activate: On<Activate>,
    buttons: Query<&InspectorSystemOrderButton>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntitySystemBindings>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorExplicitSystemsUiState>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    let (current, candidates) = {
        let mut bindings = nodes.p1();
        let Ok(bindings) = bindings.get_mut(entity) else {
            return;
        };
        let Some(binding) = bindings.0.get(button.index) else {
            return;
        };
        let current = match button.kind {
            InspectorSystemOrderKind::Before => binding.before.first().cloned(),
            InspectorSystemOrderKind::After => binding.after.first().cloned(),
        };
        let candidates = bindings
            .0
            .iter()
            .enumerate()
            .filter(|(index, binding)| *index != button.index && !binding.system_path.is_empty())
            .map(|(_, binding)| binding.system_path.clone())
            .collect::<Vec<_>>();
        (current, candidates)
    };
    if candidates.is_empty() && current.is_none() {
        return;
    }
    let next = current
        .as_ref()
        .and_then(|current| candidates.iter().position(|candidate| candidate == current))
        .and_then(|index| candidates.get(index + 1).cloned())
        .or_else(|| {
            current
                .is_none()
                .then(|| candidates.first().cloned())
                .flatten()
        });
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Change System Order",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut bindings = nodes.p1();
    let Ok(mut bindings) = bindings.get_mut(entity) else {
        return;
    };
    let Some(binding) = bindings.0.get_mut(button.index) else {
        return;
    };
    let order = match button.kind {
        InspectorSystemOrderKind::Before => &mut binding.before,
        InspectorSystemOrderKind::After => &mut binding.after,
    };
    order.clear();
    if let Some(next) = next {
        order.push(next);
    }
    state.revision = state.revision.wrapping_add(1);
    mark_document_changed(document.as_deref_mut());
}

fn on_system_script_drag_enter(
    enter: On<Pointer<DragEnter>>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    targets: Query<&InspectorSystemDropTarget>,
    mut chrome: Query<(&mut BackgroundColor, &mut BorderColor)>,
) {
    if targets.get(enter.entity).is_err() {
        return;
    }
    let valid = dragged_filesystem_entry(enter.dragged, &rows, &parents)
        .filter(|(_, is_dir)| !*is_dir)
        .is_some_and(|(relative, _)| rust_script_resource_path_from_filesystem(&relative).is_ok());
    if valid && let Ok((mut background, mut border)) = chrome.get_mut(enter.entity) {
        background.0 = theme::bg_selected();
        *border = BorderColor::all(theme::accent());
    }
}

fn on_system_script_drag_leave(
    leave: On<Pointer<DragLeave>>,
    targets: Query<&InspectorSystemDropTarget>,
    mut chrome: Query<(&mut BackgroundColor, &mut BorderColor)>,
) {
    if targets.get(leave.entity).is_err() {
        return;
    }
    if let Ok((mut background, mut border)) = chrome.get_mut(leave.entity) {
        background.0 = theme::bg_field();
        *border = BorderColor::all(theme::border_soft());
    }
}

fn on_system_script_drop(
    mut drop: On<Pointer<DragDrop>>,
    targets: Query<&InspectorSystemDropTarget>,
    rows: Query<&FsTreeRow>,
    parents: Query<&ChildOf>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    registry: Res<RustComponentRegistry>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut EntitySystemBindings>)>,
    mut history: Option<ResMut<SceneHistory>>,
    mut state: ResMut<InspectorExplicitSystemsUiState>,
    mut picker: ResMut<InspectorSystemScriptPickerState>,
    mut filesystem: ResMut<FileSystemState>,
    mut document: Option<ResMut<SceneDocument>>,
    mut chrome: Query<(&mut BackgroundColor, &mut BorderColor)>,
) {
    let Ok(target) = targets.get(drop.entity) else {
        return;
    };
    if let Ok((mut background, mut border)) = chrome.get_mut(drop.entity) {
        background.0 = theme::bg_field();
        *border = BorderColor::all(theme::border_soft());
    }
    if drop.button != PointerButton::Primary {
        return;
    }
    let Some((relative, is_dir)) = dragged_filesystem_entry(drop.dropped, &rows, &parents) else {
        return;
    };
    if is_dir {
        filesystem.status = "Drop a Rust source file, not a folder.".into();
        return;
    }
    let resource_path = match rust_script_resource_path_from_filesystem(&relative) {
        Ok(path) => path,
        Err(error) => {
            filesystem.status = error;
            return;
        }
    };
    let Some(entity) = selection.0 else { return };
    let candidates: Vec<_> = registry
        .systems
        .iter()
        .filter(|definition| definition.source_path == resource_path)
        .collect();
    if candidates.is_empty() {
        filesystem.status = format!("No Bevy system function found in {resource_path}");
        return;
    }
    if candidates.len() > 1 {
        picker.open = true;
        picker.binding_index = Some(target.0);
        picker.source_filter = Some(resource_path.clone());
        picker.selected = None;
        picker.selected_system = None;
        picker.filter.clear();
        picker.revision = picker.revision.wrapping_add(1);
        filesystem.status = format!("Choose a system from {resource_path}");
        drop.propagate(false);
        return;
    }
    let definition = candidates[0];
    let should_attach = {
        let mut bindings = nodes.p1();
        bindings.get_mut(entity).ok().is_some_and(|bindings| {
            bindings.0.get(target.0).is_some_and(|binding| {
                binding.script_path != definition.source_path
                    || binding.system_path != definition.system_path
            })
        })
    };
    if !should_attach {
        return;
    }
    if let Some(history) = history.as_deref_mut() {
        let history_nodes = nodes.p0();
        history.begin(
            "Attach System Script",
            capture_scene_snapshot(&history_nodes, &selection, *mode),
        );
    }
    let mut bindings = nodes.p1();
    let Ok(mut bindings) = bindings.get_mut(entity) else {
        return;
    };
    let Some(binding) = bindings.0.get_mut(target.0) else {
        return;
    };
    binding.script_path = definition.source_path.clone();
    binding.system_path = definition.system_path.clone();
    state.revision = state.revision.wrapping_add(1);
    filesystem.status = format!("Attached {resource_path}");
    mark_document_changed(document.as_deref_mut());
    drop.propagate(false);
}

fn toggle_transform_section(
    activate: On<Activate>,
    toggles: Query<(), With<InspectorTransformToggle>>,
    mut state: ResMut<InspectorTransformUiState>,
) {
    if toggles.get(activate.entity).is_ok() {
        state.collapsed = !state.collapsed;
    }
}

fn toggle_scale_link(
    activate: On<Activate>,
    buttons: Query<(), With<InspectorScaleLinkButton>>,
    mut state: ResMut<InspectorTransformUiState>,
) {
    if buttons.get(activate.entity).is_ok() {
        state.scale_linked = !state.scale_linked;
    }
}

fn sync_transform_chrome(
    state: Res<InspectorTransformUiState>,
    mut bodies: Query<&mut Node, With<InspectorTransformBody>>,
    mut chevrons: Query<&mut Text, With<InspectorTransformChevron>>,
    mut scale_links: Query<
        (&mut BackgroundColor, &mut BorderColor),
        With<InspectorScaleLinkButton>,
    >,
) {
    for mut node in &mut bodies {
        node.display = if state.collapsed {
            Display::None
        } else {
            Display::Flex
        };
    }
    for mut text in &mut chevrons {
        text.0 = if state.collapsed { ">" } else { "v" }.into();
    }
    for (mut background, mut border) in &mut scale_links {
        background.0 = if state.scale_linked {
            Color::srgb(0.10, 0.20, 0.26)
        } else {
            Color::NONE
        };
        *border = BorderColor::all(if state.scale_linked {
            theme::accent()
        } else {
            Color::NONE
        });
    }
}

fn insert_transform_animation_key(
    activate: On<Activate>,
    buttons: Query<&InspectorTransformKey>,
    mut commands: Commands,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let property = match button.0 {
        InspectorTransformGroup::Position => AnimationTransformProperty::Position,
        InspectorTransformGroup::Rotation => AnimationTransformProperty::Rotation,
        InspectorTransformGroup::Scale => AnimationTransformProperty::Scale,
    };
    commands.trigger(InsertAnimationPropertyKey { property });
}

fn reset_transform_group(
    activate: On<Activate>,
    resets: Query<&InspectorTransformReset>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut Transform>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(reset) = resets.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else { return };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Reset Transform", before);
    }
    let mut transforms = nodes.p1();
    let Ok(mut transform) = transforms.get_mut(entity) else {
        return;
    };
    let changed = match reset.0 {
        InspectorTransformGroup::Position => {
            if transform.translation == Vec3::ZERO {
                false
            } else {
                transform.translation = Vec3::ZERO;
                true
            }
        }
        InspectorTransformGroup::Rotation => {
            if transform.rotation == Quat::IDENTITY {
                false
            } else {
                transform.rotation = Quat::IDENTITY;
                true
            }
        }
        InspectorTransformGroup::Scale => {
            if transform.scale == Vec3::ONE {
                false
            } else {
                transform.scale = Vec3::ONE;
                true
            }
        }
    };
    if changed {
        mark_document_changed(document.as_deref_mut());
    }
}

fn set_transform_field(
    transform: &mut Transform,
    field: InspectorField,
    value: f32,
    space: SceneSpace,
    scale_linked: bool,
) {
    match field {
        InspectorField::PosX => transform.translation.x = value,
        InspectorField::PosY => transform.translation.y = value,
        InspectorField::PosZ => transform.translation.z = value,
        InspectorField::RotX | InspectorField::RotY | InspectorField::RotZ => {
            let (mut x, mut y, mut z) = transform.rotation.to_euler(EulerRot::XYZ);
            match field {
                InspectorField::RotX => x = value.to_radians(),
                InspectorField::RotY => y = value.to_radians(),
                InspectorField::RotZ => z = value.to_radians(),
                _ => {}
            }
            transform.rotation = Quat::from_euler(EulerRot::XYZ, x, y, z);
        }
        InspectorField::ScaleX | InspectorField::ScaleY | InspectorField::ScaleZ
            if scale_linked =>
        {
            match space {
                SceneSpace::TwoD => {
                    transform.scale.x = value;
                    transform.scale.y = value;
                }
                SceneSpace::ThreeD => transform.scale = Vec3::splat(value),
            }
        }
        InspectorField::ScaleX => transform.scale.x = value,
        InspectorField::ScaleY => transform.scale.y = value,
        InspectorField::ScaleZ => transform.scale.z = value,
    }
}

fn apply_inspector_nudge(
    activate: On<Activate>,
    nudges: Query<&InspectorNudge>,
    selection: Res<Selection>,
    mode: Res<WorkspaceMode>,
    state: Res<InspectorTransformUiState>,
    spaces: Query<&SceneSpace>,
    mut history: Option<ResMut<SceneHistory>>,
    mut nodes: ParamSet<(Query<SceneSnapshotQuery>, Query<&mut Transform>)>,
    mut document: Option<ResMut<SceneDocument>>,
) {
    let Ok(nudge) = nudges.get(activate.entity) else {
        return;
    };
    let Some(entity) = selection.0 else {
        return;
    };
    let Ok(space) = spaces.get(entity).copied() else {
        return;
    };
    if let Some(history) = history.as_deref_mut() {
        let before = {
            let history_nodes = nodes.p0();
            capture_scene_snapshot(&history_nodes, &selection, *mode)
        };
        history.begin("Edit Transform", before);
    }
    let mut transforms = nodes.p1();
    let Ok(mut transform) = transforms.get_mut(entity) else {
        return;
    };

    apply_field(
        &mut transform,
        nudge.field,
        nudge.delta,
        space,
        state.scale_linked,
    );
    mark_document_changed(document.as_deref_mut());
}

fn apply_field(
    transform: &mut Transform,
    field: InspectorField,
    delta: f32,
    space: SceneSpace,
    scale_linked: bool,
) {
    let value = transform_field_value(transform, field) + delta;
    set_transform_field(transform, field, value, space, scale_linked);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hierarchy::{SceneNodeId, SceneParentId, SceneRootMenuState, SceneSiblingOrder},
        undo::{HistoryAction, HistoryActionButton, SceneHistoryPlugin},
        workspace::WorkspaceSelections,
    };

    fn ui_test_app(kind: EntityKind) -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: kind.label().into(),
                },
                kind,
                AddedEntityComponents::default(),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
                kind.default_ui_layout().unwrap(),
                kind.default_ui_content().unwrap(),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);
        (app, selected)
    }

    fn animation_test_app(player: SceneAnimationPlayer) -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "AnimationPlayer".into(),
                },
                EntityKind::AnimationPlayer,
                AddedEntityComponents::default(),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
                player,
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);
        (app, selected)
    }

    #[test]
    fn ui_transform_exposes_keys_for_supported_animation_groups() {
        assert_eq!(
            ui_animation_property(InspectorUiTransformGroup::Position),
            Some(AnimationTransformProperty::Position)
        );
        assert_eq!(
            ui_animation_property(InspectorUiTransformGroup::Rotation),
            Some(AnimationTransformProperty::Rotation)
        );
        assert_eq!(
            ui_animation_property(InspectorUiTransformGroup::Scale),
            Some(AnimationTransformProperty::Scale)
        );
        assert_eq!(ui_animation_property(InspectorUiTransformGroup::Size), None);
        assert_eq!(
            ui_animation_property(InspectorUiTransformGroup::PivotOffset),
            None
        );
    }

    #[test]
    fn animation_controls_add_select_loop_and_remove_clips() {
        let (mut app, selected) = animation_test_app(SceneAnimationPlayer::default());
        let add = app.world_mut().spawn(InspectorAddAnimationButton).id();
        app.world_mut().trigger(Activate { entity: add });
        app.world_mut().trigger(Activate { entity: add });
        assert_eq!(
            app.world()
                .get::<SceneAnimationPlayer>(selected)
                .unwrap()
                .clips
                .iter()
                .map(|clip| clip.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Animation", "Animation2"]
        );

        let autoplay = app
            .world_mut()
            .spawn(InspectorAnimationAutoplayOption(Some(1)))
            .id();
        app.world_mut().trigger(Activate { entity: autoplay });
        let loop_button = app
            .world_mut()
            .spawn(InspectorAnimationClipLoopButton(1))
            .id();
        app.world_mut().trigger(Activate {
            entity: loop_button,
        });
        let player = app.world().get::<SceneAnimationPlayer>(selected).unwrap();
        assert_eq!(player.autoplay, "Animation2");
        assert!(player.clips[1].looped);

        let remove = app
            .world_mut()
            .spawn(InspectorRemoveAnimationButton(1))
            .id();
        app.world_mut().trigger(Activate { entity: remove });
        let player = app.world().get::<SceneAnimationPlayer>(selected).unwrap();
        assert_eq!(player.clips.len(), 1);
        assert!(player.autoplay.is_empty());
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn animation_text_inputs_edit_speed_name_and_safe_length() {
        let player = SceneAnimationPlayer {
            autoplay: "Idle".into(),
            clips: vec![SceneAnimationClip {
                name: "Idle".into(),
                length: 3.0,
                tracks: vec![arisna_engine::SceneAnimationTrack {
                    keys: vec![arisna_engine::SceneAnimationKey {
                        time: 2.0,
                        value: "event".into(),
                    }],
                    ..default()
                }],
                ..default()
            }],
            ..default()
        };
        let (mut app, selected) = animation_test_app(player);
        let speed = app
            .world_mut()
            .spawn((InspectorAnimationSpeedInput, EditableText::new("1.00")))
            .id();
        let name = app
            .world_mut()
            .spawn((
                InspectorAnimationClipNameInput(0),
                EditableText::new("Idle"),
            ))
            .id();
        let length = app
            .world_mut()
            .spawn((
                InspectorAnimationClipLengthInput(0),
                EditableText::new("3.00"),
            ))
            .id();
        app.update();
        app.world_mut()
            .insert_resource(InputFocus::from_entity(speed));
        app.world_mut()
            .entity_mut(speed)
            .get_mut::<EditableText>()
            .unwrap()
            .editor_mut()
            .set_text("2.50");
        app.world_mut()
            .entity_mut(name)
            .get_mut::<EditableText>()
            .unwrap()
            .editor_mut()
            .set_text("Run");
        app.world_mut()
            .entity_mut(length)
            .get_mut::<EditableText>()
            .unwrap()
            .editor_mut()
            .set_text("0.50");
        app.update();

        let player = app.world().get::<SceneAnimationPlayer>(selected).unwrap();
        assert_eq!(player.speed, 2.5);
        assert_eq!(player.autoplay, "Run");
        assert_eq!(player.clips[0].name, "Run");
        assert_eq!(player.clips[0].length, 2.0);
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn added_component_is_applied_and_recorded_for_scene_persistence() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);

        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Logic".into(),
                },
                EntityKind::Empty,
                AddedEntityComponents::default(),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);
        let option = app
            .world_mut()
            .spawn(InspectorComponentOption(BuiltinComponent::Sprite))
            .id();

        app.world_mut().trigger(Activate { entity: option });
        app.world_mut().flush();

        let entity = app.world().entity(selected);
        assert!(entity.contains::<Sprite>());
        assert!(entity.contains::<Transform>());
        assert_eq!(
            entity.get::<AddedEntityComponents>().unwrap().0,
            vec![BuiltinComponent::Sprite]
        );
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn custom_component_add_reset_and_remove_use_discovered_definition() {
        use crate::rust_components::RustComponentFieldDefinition;

        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let definition = RustComponentDefinition {
            type_path: "sample_game::player::MovementState".into(),
            name: "MovementState".into(),
            source_path: "res://src/player.rs".into(),
            fields: vec![RustComponentFieldDefinition {
                name: "speed".into(),
                type_name: "f32".into(),
                default_value: "0.0".into(),
                editable: true,
            }],
            reflection_ready: false,
        };
        app.world_mut()
            .resource_mut::<RustComponentRegistry>()
            .components
            .push(definition.clone());
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Player".into(),
                },
                EntityKind::Empty2D,
                AddedEntityComponents::default(),
                EntityCustomComponents::default(),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);

        let add = app
            .world_mut()
            .spawn(InspectorCustomComponentOption(definition.type_path.clone()))
            .id();
        app.world_mut().trigger(Activate { entity: add });
        assert_eq!(
            app.world()
                .entity(selected)
                .get::<EntityCustomComponents>()
                .unwrap()
                .0,
            vec![definition.instantiate()]
        );

        app.world_mut()
            .entity_mut(selected)
            .get_mut::<EntityCustomComponents>()
            .unwrap()
            .0[0]
            .fields[0]
            .value = "12.5".into();
        let reset = app.world_mut().spawn(InspectorCustomComponentReset(0)).id();
        app.world_mut().trigger(Activate { entity: reset });
        assert_eq!(
            app.world()
                .entity(selected)
                .get::<EntityCustomComponents>()
                .unwrap()
                .0[0]
                .fields[0]
                .value,
            "0.0"
        );

        let remove = app
            .world_mut()
            .spawn(InspectorCustomComponentRemove(0))
            .id();
        app.world_mut().trigger(Activate { entity: remove });
        assert!(
            app.world()
                .entity(selected)
                .get::<EntityCustomComponents>()
                .unwrap()
                .0
                .is_empty()
        );
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn ui_layout_actions_edit_values_and_mark_the_scene_dirty() {
        let (mut app, selected) = ui_test_app(EntityKind::Panel);
        let nudge = app
            .world_mut()
            .spawn(InspectorUiNudge {
                field: InspectorUiField::Width,
                delta: 10.0,
            })
            .id();
        app.world_mut().trigger(Activate { entity: nudge });
        let dropdown = app.world_mut().spawn(InspectorAnchorDropdownButton).id();
        app.world_mut().trigger(Activate { entity: dropdown });
        assert!(
            app.world()
                .resource::<InspectorUiLayoutState>()
                .anchor_menu_open
        );
        let anchor = app
            .world_mut()
            .spawn(InspectorAnchorPreset(AnchorPreset::Center))
            .id();
        app.world_mut().trigger(Activate { entity: anchor });
        let alignment = app
            .world_mut()
            .spawn(InspectorAlignmentButton(AlignmentAxis::Horizontal))
            .id();
        app.world_mut().trigger(Activate { entity: alignment });
        app.world_mut().flush();

        let layout = app.world().entity(selected).get::<SceneUiLayout>().unwrap();
        assert_eq!(layout.size.0, 250.0);
        assert_eq!(layout.anchor_min, (0.5, 0.5));
        assert_eq!(layout.anchor_max, (0.5, 0.5));
        assert_eq!(layout.offset, (-125.0, -80.0));
        assert_eq!(layout.horizontal_alignment, UiAlignment::Center);
        assert!(
            !app.world()
                .resource::<InspectorUiLayoutState>()
                .anchor_menu_open
        );
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn ui_transform_controls_apply_linking_clamps_reset_and_clip() {
        let (mut app, selected) = ui_test_app(EntityKind::Panel);
        {
            let mut entity = app.world_mut().entity_mut(selected);
            let mut layout = entity.get_mut::<SceneUiLayout>().unwrap();
            layout.rotation = 12.0;
        }

        let scale = app
            .world_mut()
            .spawn(InspectorUiNudge {
                field: InspectorUiField::ScaleX,
                delta: 0.5,
            })
            .id();
        app.world_mut().trigger(Activate { entity: scale });
        let minimum = app
            .world_mut()
            .spawn(InspectorUiNudge {
                field: InspectorUiField::MinimumWidth,
                delta: -10.0,
            })
            .id();
        app.world_mut().trigger(Activate { entity: minimum });
        let pivot_ratio = app
            .world_mut()
            .spawn(InspectorUiNudge {
                field: InspectorUiField::PivotRatioX,
                delta: 2.0,
            })
            .id();
        app.world_mut().trigger(Activate {
            entity: pivot_ratio,
        });
        let clip = app.world_mut().spawn(InspectorUiClipButton).id();
        app.world_mut().trigger(Activate { entity: clip });
        let reset = app
            .world_mut()
            .spawn(InspectorUiTransformReset(
                InspectorUiTransformGroup::Rotation,
            ))
            .id();
        app.world_mut().trigger(Activate { entity: reset });
        app.world_mut().flush();

        let layout = app.world().entity(selected).get::<SceneUiLayout>().unwrap();
        assert_eq!(layout.scale, (1.5, 1.5));
        assert_eq!(layout.minimum_size.0, 0.0);
        assert_eq!(layout.pivot_ratio.0, 1.0);
        assert_eq!(layout.rotation, 0.0);
        assert!(layout.clip_contents);
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn anchor_dropdown_matches_godot_preset_order() {
        assert_eq!(AnchorPreset::ALL.len(), 16);
        assert_eq!(AnchorPreset::ALL[0].label(), "Top Left");
        assert_eq!(AnchorPreset::ALL[13].label(), "Wide VCenter");
        assert_eq!(AnchorPreset::ALL[14].label(), "Wide HCenter");
        assert_eq!(AnchorPreset::ALL[15].label(), "Full Rect");
    }

    #[test]
    fn panel_color_is_clamped_to_valid_rgba_range() {
        let (mut app, selected) = ui_test_app(EntityKind::Panel);
        let nudge = app
            .world_mut()
            .spawn(InspectorUiNudge {
                field: InspectorUiField::ColorA,
                delta: 4.0,
            })
            .id();
        app.world_mut().trigger(Activate { entity: nudge });
        app.world_mut().flush();

        assert_eq!(
            app.world()
                .entity(selected)
                .get::<SceneUiContent>()
                .unwrap()
                .panel_color
                .3,
            1.0
        );
    }

    #[test]
    fn editable_text_updates_ui_content() {
        let (mut app, selected) = ui_test_app(EntityKind::Text);
        let input = app
            .world_mut()
            .spawn((InspectorUiTextInput, EditableText::new("")))
            .id();
        app.update();
        app.world_mut()
            .entity_mut(input)
            .get_mut::<EditableText>()
            .unwrap()
            .editor_mut()
            .set_text("Hello UI");
        app.update();

        assert_eq!(
            app.world()
                .entity(selected)
                .get::<SceneUiContent>()
                .unwrap()
                .text,
            "Hello UI"
        );
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn editable_image_resource_updates_ui_content() {
        let (mut app, selected) = ui_test_app(EntityKind::Image);
        let input = app
            .world_mut()
            .spawn((InspectorUiImageInput, EditableText::new("")))
            .id();
        app.update();
        app.world_mut()
            .entity_mut(input)
            .get_mut::<EditableText>()
            .unwrap()
            .editor_mut()
            .set_text("res://ui/logo.png");
        app.update();

        assert_eq!(
            app.world()
                .entity(selected)
                .get::<SceneUiContent>()
                .unwrap()
                .image_path,
            "res://ui/logo.png"
        );
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn editable_image_resource_updates_sprite_and_render_data() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Sprite2D".into(),
                },
                EntityKind::Sprite2D,
                AddedEntityComponents::default(),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
                Transform::default(),
                Visibility::Visible,
                SceneSprite2D::default(),
                Sprite::sized(Vec2::splat(64.0)),
                Anchor::TOP_LEFT,
            ))
            .id();
        let input = app
            .world_mut()
            .spawn((InspectorSpriteImageInput, EditableText::new("")))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);
        app.update();
        app.world_mut()
            .entity_mut(input)
            .get_mut::<EditableText>()
            .unwrap()
            .editor_mut()
            .set_text("res://sprites/hero.png");
        app.update();

        assert_eq!(
            app.world()
                .entity(selected)
                .get::<SceneSprite2D>()
                .unwrap()
                .image_path,
            "res://sprites/hero.png"
        );
        assert_eq!(
            app.world()
                .entity(selected)
                .get::<Sprite>()
                .unwrap()
                .custom_size,
            None
        );
        assert_eq!(
            app.world().entity(selected).get::<Anchor>(),
            Some(&Anchor::TOP_LEFT)
        );
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn editable_mesh3d_resource_assigns_and_clears_model() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Mesh3D".into(),
                },
                EntityKind::Mesh3D,
                AddedEntityComponents::default(),
                SceneSpace::ThreeD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
                Transform::default(),
                Visibility::Visible,
            ))
            .id();
        let input = app
            .world_mut()
            .spawn((InspectorMesh3dInput, EditableText::new("")))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);
        app.update();

        app.world_mut()
            .entity_mut(input)
            .get_mut::<EditableText>()
            .unwrap()
            .editor_mut()
            .set_text("res://models/hero.glb");
        app.update();

        assert_eq!(
            app.world()
                .entity(selected)
                .get::<SceneModel3D>()
                .unwrap()
                .resource_path,
            "res://models/hero.glb"
        );

        app.world_mut()
            .entity_mut(input)
            .get_mut::<EditableText>()
            .unwrap()
            .editor_mut()
            .set_text("");
        app.update();

        let entity = app.world().entity(selected);
        assert!(!entity.contains::<SceneModel3D>());
        assert!(entity.contains::<NeedsDefaultMesh3D>());
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn sprite_controls_update_render_state_and_clamp_values() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Sprite2D".into(),
                },
                EntityKind::Sprite2D,
                AddedEntityComponents::default(),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
                Transform::default(),
                Visibility::Visible,
                SceneSprite2D::default(),
                Sprite::sized(Vec2::splat(64.0)),
                Anchor::TOP_LEFT,
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);

        for kind in [
            InspectorSpriteToggleKind::Visible,
            InspectorSpriteToggleKind::FlipX,
            InspectorSpriteToggleKind::Region,
        ] {
            let button = app.world_mut().spawn(InspectorSpriteToggle(kind)).id();
            app.world_mut().trigger(Activate { entity: button });
        }
        for (field, delta) in [
            (InspectorSpriteField::HFrames, 3.0),
            (InspectorSpriteField::VFrames, 1.0),
            (InspectorSpriteField::Frame, 99.0),
            (InspectorSpriteField::RegionWidth, -100.0),
            (InspectorSpriteField::AnchorX, 2.0),
            (InspectorSpriteField::ColorA, -2.0),
            (InspectorSpriteField::ZIndex, 3.0),
        ] {
            let nudge = app
                .world_mut()
                .spawn(InspectorSpriteNudge { field, delta })
                .id();
            app.world_mut().trigger(Activate { entity: nudge });
        }
        app.update();

        let entity = app.world().entity(selected);
        let data = entity.get::<SceneSprite2D>().unwrap();
        assert!(!data.visible);
        assert!(data.flip_x);
        assert!(data.region_enabled);
        assert_eq!((data.hframes, data.vframes, data.frame), (4, 2, 7));
        assert_eq!(data.region_rect.2, 1.0);
        assert_eq!(data.anchor.0, 1.0);
        assert_eq!(data.color.3, 0.0);
        assert_eq!(data.z_index, 3);
        assert_eq!(entity.get::<Visibility>(), Some(&Visibility::Hidden));
        assert_eq!(entity.get::<Transform>().unwrap().translation.z, 3.0);
        assert_eq!(
            entity.get::<Sprite>().unwrap().rect.map(|rect| rect.size()),
            Some(Vec2::new(1.0, 64.0))
        );
        assert_eq!(entity.get::<Anchor>(), Some(&Anchor::TOP_RIGHT));
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn transform_display_uses_one_decimal_and_normalizes_negative_zero() {
        assert_eq!(format_transform_value(12.345), "12.3");
        assert_eq!(format_transform_value(-0.0), "0.0");
        assert_eq!(round_transform_value(12.36), 12.4);
    }

    #[test]
    fn ecs_layout_counts_owned_components_and_matching_systems() {
        let sprite = InspectorEntityFeatures {
            transform: true,
            visibility: true,
            sprite: true,
            ..default()
        };
        let registry = InspectorSystemRegistry::default();
        assert_eq!(sprite.component_count(), 3);
        assert_eq!(registry.matching_count(sprite), 3);
        assert!(sprite.system_matches(InspectorSystemKind::SpriteRender));
        assert!(!sprite.system_matches(InspectorSystemKind::MeshRender));

        let ui = InspectorEntityFeatures {
            ui_layout: true,
            ui_content: true,
            ..default()
        };
        assert_eq!(ui.component_count(), 2);
        assert_eq!(registry.matching_count(ui), 2);

        let sprite_only_registry = InspectorSystemRegistry {
            registered: vec![InspectorSystemKind::SpriteRender],
        };
        assert_eq!(sprite_only_registry.matching_count(sprite), 1);
        assert_eq!(sprite_only_registry.matching_count(ui), 0);
    }

    #[test]
    fn inspector_added_component_can_be_removed_and_persisted() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Logic".into(),
                },
                EntityKind::Empty,
                AddedEntityComponents(vec![BuiltinComponent::Sprite]),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
                Transform::default(),
                Visibility::Visible,
                Sprite::sized(Vec2::splat(64.0)),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);
        let remove = app
            .world_mut()
            .spawn(InspectorRemoveComponentButton(BuiltinComponent::Sprite))
            .id();

        app.world_mut().trigger(Activate { entity: remove });
        app.world_mut().flush();

        let entity = app.world().entity(selected);
        assert!(!entity.contains::<Sprite>());
        assert!(!entity.contains::<Transform>());
        assert!(!entity.contains::<Visibility>());
        assert!(entity.get::<AddedEntityComponents>().unwrap().0.is_empty());
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn inspector_component_removal_supports_undo_and_redo() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<WorkspaceSelections>()
            .init_resource::<SceneRootMenuState>()
            .init_resource::<SceneDocument>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<bevy::gizmos::transform_gizmo::TransformGizmoState>()
            .add_plugins((InspectorPlugin, SceneHistoryPlugin));
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Logic".into(),
                },
                EntityKind::Empty,
                AddedEntityComponents(vec![BuiltinComponent::Sprite]),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
                Transform::default(),
                Visibility::Visible,
                Sprite::sized(Vec2::splat(64.0)),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);
        let remove = app
            .world_mut()
            .spawn(InspectorRemoveComponentButton(BuiltinComponent::Sprite))
            .id();

        app.world_mut().trigger(Activate { entity: remove });
        app.world_mut().flush();
        app.update();
        assert!(app.world().resource::<SceneHistory>().can_undo());

        let undo = app
            .world_mut()
            .spawn(HistoryActionButton(HistoryAction::Undo))
            .id();
        app.world_mut().trigger(Activate { entity: undo });
        app.world_mut().flush();
        let restored = app.world().resource::<Selection>().0.unwrap();
        assert!(app.world().entity(restored).contains::<Sprite>());
        assert_eq!(
            app.world()
                .entity(restored)
                .get::<AddedEntityComponents>()
                .unwrap()
                .0,
            vec![BuiltinComponent::Sprite]
        );

        let redo = app
            .world_mut()
            .spawn(HistoryActionButton(HistoryAction::Redo))
            .id();
        app.world_mut().trigger(Activate { entity: redo });
        app.world_mut().flush();
        let removed_again = app.world().resource::<Selection>().0.unwrap();
        assert!(!app.world().entity(removed_again).contains::<Sprite>());
        assert!(
            app.world()
                .entity(removed_again)
                .get::<AddedEntityComponents>()
                .unwrap()
                .0
                .is_empty()
        );
    }

    #[test]
    fn ecs_sections_and_system_cards_toggle_independently() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let components = app.world_mut().spawn(InspectorComponentsToggle).id();
        let systems = app.world_mut().spawn(InspectorSystemsToggle).id();
        let system_card = app
            .world_mut()
            .spawn(InspectorSystemToggle(InspectorSystemKind::SpriteRender))
            .id();

        app.world_mut().trigger(Activate { entity: components });
        app.world_mut().trigger(Activate { entity: systems });
        app.world_mut().trigger(Activate {
            entity: system_card,
        });

        let state = app.world().resource::<InspectorEcsUiState>();
        assert!(state.components_collapsed);
        assert!(state.systems_collapsed);
        assert_eq!(
            state.expanded_system,
            Some(InspectorSystemKind::SpriteRender)
        );
    }

    #[test]
    fn inspector_top_tabs_switch_active_content() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let systems = app
            .world_mut()
            .spawn(InspectorTab(InspectorTabKind::Systems))
            .id();
        let components = app
            .world_mut()
            .spawn(InspectorTab(InspectorTabKind::Components))
            .id();

        app.world_mut().trigger(Activate { entity: systems });
        assert_eq!(
            app.world().resource::<InspectorEcsUiState>().active_tab,
            InspectorTabKind::Systems
        );

        app.world_mut().trigger(Activate { entity: components });
        assert_eq!(
            app.world().resource::<InspectorEcsUiState>().active_tab,
            InspectorTabKind::Components
        );
    }

    #[test]
    fn explicit_system_binding_add_schedule_and_remove_are_functional() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Player".into(),
                },
                EntityKind::Empty2D,
                AddedEntityComponents::default(),
                EntitySystemBindings::default(),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);

        let add = app.world_mut().spawn(InspectorAddSystemButton).id();
        app.world_mut().trigger(Activate { entity: add });
        assert_eq!(
            app.world()
                .entity(selected)
                .get::<EntitySystemBindings>()
                .unwrap()
                .0,
            vec![SceneSystemBinding::default()]
        );

        let schedule = app.world_mut().spawn(InspectorSystemScheduleButton(0)).id();
        app.world_mut().trigger(Activate { entity: schedule });
        assert_eq!(
            app.world()
                .entity(selected)
                .get::<EntitySystemBindings>()
                .unwrap()
                .0[0]
                .schedule,
            arisna_engine::SceneSystemSchedule::FixedUpdate
        );

        let remove = app.world_mut().spawn(InspectorRemoveSystemButton(0)).id();
        app.world_mut().trigger(Activate { entity: remove });
        assert!(
            app.world()
                .entity(selected)
                .get::<EntitySystemBindings>()
                .unwrap()
                .0
                .is_empty()
        );
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn system_script_picker_attaches_and_clears_a_real_project_resource() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .init_resource::<FileSystemState>()
            .add_plugins(InspectorPlugin);
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Camera".into(),
                },
                EntityKind::Camera2D,
                AddedEntityComponents::default(),
                EntitySystemBindings(vec![SceneSystemBinding::default()]),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);
        {
            let mut picker = app
                .world_mut()
                .resource_mut::<InspectorSystemScriptPickerState>();
            picker.open = true;
            picker.binding_index = Some(0);
            picker.selected = Some("res://src/systems/camera_follow.rs".into());
        }

        let confirm = app
            .world_mut()
            .spawn(InspectorSystemScriptPickerButton(
                InspectorSystemScriptPickerAction::Confirm,
            ))
            .id();
        app.world_mut().trigger(Activate { entity: confirm });
        assert_eq!(
            app.world()
                .entity(selected)
                .get::<EntitySystemBindings>()
                .unwrap()
                .0[0]
                .script_path,
            "res://src/systems/camera_follow.rs"
        );
        assert!(
            !app.world()
                .resource::<InspectorSystemScriptPickerState>()
                .open
        );

        let clear = app
            .world_mut()
            .spawn(InspectorClearSystemScriptButton(0))
            .id();
        app.world_mut().trigger(Activate { entity: clear });
        assert!(
            app.world()
                .entity(selected)
                .get::<EntitySystemBindings>()
                .unwrap()
                .0[0]
                .script_path
                .is_empty()
        );
        assert!(app.world().resource::<SceneDocument>().dirty);
    }

    #[test]
    fn linked_scale_updates_only_visible_2d_axes() {
        let mut transform = Transform::from_scale(Vec3::new(1.0, 2.0, 7.0));
        set_transform_field(
            &mut transform,
            InspectorField::ScaleX,
            3.5,
            SceneSpace::TwoD,
            true,
        );

        assert_eq!(transform.scale, Vec3::new(3.5, 3.5, 7.0));
    }

    #[test]
    fn linked_scale_updates_all_3d_axes() {
        let mut transform = Transform::from_scale(Vec3::new(1.0, 2.0, 3.0));
        set_transform_field(
            &mut transform,
            InspectorField::ScaleY,
            4.0,
            SceneSpace::ThreeD,
            true,
        );

        assert_eq!(transform.scale, Vec3::splat(4.0));
    }

    #[test]
    fn transform_reset_is_applied_and_marks_scene_dirty() {
        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<WorkspaceMode>()
            .init_resource::<SceneDocument>()
            .init_resource::<SceneHistory>()
            .add_plugins(InspectorPlugin);
        let selected = app
            .world_mut()
            .spawn((
                EditableObject {
                    name: "Moved".into(),
                },
                EntityKind::Empty2D,
                AddedEntityComponents::default(),
                SceneSpace::TwoD,
                SceneNodeId::new(),
                SceneParentId(None),
                SceneSiblingOrder(0),
                Transform::from_xyz(25.0, 40.0, 3.0),
            ))
            .id();
        app.world_mut().resource_mut::<Selection>().0 = Some(selected);
        let reset = app
            .world_mut()
            .spawn(InspectorTransformReset(InspectorTransformGroup::Position))
            .id();

        app.world_mut().trigger(Activate { entity: reset });
        app.world_mut().flush();

        assert_eq!(
            app.world()
                .entity(selected)
                .get::<Transform>()
                .unwrap()
                .translation,
            Vec3::ZERO
        );
        assert!(app.world().resource::<SceneDocument>().dirty);
    }
}
