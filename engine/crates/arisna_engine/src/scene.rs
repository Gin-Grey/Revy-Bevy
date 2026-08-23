//! 编辑器与游戏共享的数据驱动场景格式和运行时加载。
//!
//! 本文件同时定义磁盘契约和运行时物化。修改 `SceneNodeData` 字段时必须保持
//! 编辑器保存、旧场景兼容、BSN Loader、热重载和运行时实体创建同步。

use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    time::SystemTime,
};

use bevy::ecs::system::{IntoSystem, SystemId};
use bevy::prelude::*;
use bevy::scene::bsn::{
    format_bsn, parse_bsn, BsnComponent, BsnComponentBody, BsnDocument, BsnEntity, BsnSpan,
    BsnStructField, BsnValue,
};
use bevy::scene::{ScenePatch, ScenePatchInstance};
use bevy::sprite::Anchor;
use bevy::ui::{UiTransform, Val2};
use bevy::world_serialization::{WorldAssetRoot, WorldInstance, WorldInstanceSpawner};
use bevy::{
    ecs::reflect::{AppTypeRegistry, ReflectComponent},
    reflect::{std_traits::ReflectDefault, PartialReflect, ReflectMut},
};
use bevy_ufbx::FbxLoaderSettings;
use serde::{Deserialize, Serialize};

use crate::{
    animation::{
        advance_runtime_animation_players, apply_runtime_animations,
        initialize_runtime_animation_players, RuntimeAnimationPlayback,
    },
    ProjectRoot,
};

pub const SCENE_FORMAT_VERSION: u32 = 2;

/// `.bsn` 场景的内存级文档模型。
///
/// `root` 和父子关系保存稳定字符串 ID，绝不能保存当前 World 的 Bevy Entity。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SceneFile {
    pub format_version: u32,
    pub root: String,
    pub entities: Vec<SceneNodeData>,
}

#[derive(Component, Reflect, Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[reflect(Component, Default)]
/// 单个场景实体的可持久化契约。
///
/// 预设类型、组件列表和专用数据分开保存，使旧场景能在新增组件后继续读取。
pub struct SceneNodeData {
    pub id: String,
    pub parent: Option<String>,
    pub order: u32,
    pub name: String,
    pub kind: String,
    /// Explicit workspace for logical `empty` entities. Older scenes omit it.
    #[serde(default)]
    pub space: Option<String>,
    /// Bevy-native components added in the Inspector beyond the entity preset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<String>,
    /// Project-defined Rust components authored through the Inspector. These
    /// values remain available at runtime even when the concrete Rust type has
    /// not been registered for reflection by the game.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_components: Vec<SceneCustomComponent>,
    /// Optional Rust script attached to this entity as a lifecycle owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_script: Option<SceneEntityScript>,
    /// Rust systems explicitly bound to this entity by the editor. Automatic
    /// ECS query matches are derived at runtime and are never serialized here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<SceneSystemBinding>,
    /// Layout authored for UI entity presets. It is optional so existing v2
    /// scenes remain forwards-compatible with the UI entity system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_layout: Option<SceneUiLayout>,
    /// Editable content and appearance for UI entity presets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_content: Option<SceneUiContent>,
    /// Editable appearance for Sprite2D presets and authored Sprite components.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite: Option<SceneSprite2D>,
    /// Imported 3D model instantiated by this scene node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<SceneModel3D>,
    /// Optional Godot-style animation player authored on this scene entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_player: Option<SceneAnimationPlayer>,
    /// Optional axis-aligned 2D collision rectangle authored by the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision_rect: Option<SceneCollisionRect2D>,
    pub translation: (f32, f32, f32),
    pub rotation: (f32, f32, f32, f32),
    pub scale: (f32, f32, f32),
}

#[derive(Reflect, Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SceneCustomComponent {
    /// Fully-qualified project type path, for example
    /// `jjxf_client::world::entities::MovementState`.
    pub type_path: String,
    /// Project-relative source path stored as `res://src/...`.
    pub source_path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<SceneCustomField>,
}

impl SceneCustomComponent {
    pub fn display_name(&self) -> &str {
        self.type_path
            .rsplit("::")
            .next()
            .unwrap_or(&self.type_path)
    }
}

#[derive(Reflect, Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SceneCustomField {
    pub name: String,
    pub type_name: String,
    /// Canonical editor value. Primitive fields use their Rust literal form;
    /// compound values use comma-separated scalar values.
    pub value: String,
}

#[derive(Reflect, Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SceneEntityScript {
    pub source_path: String,
    #[serde(default)]
    pub type_path: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callbacks: Vec<SceneEntityScriptCallback>,
}

#[derive(Reflect, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SceneEntityScriptCallback {
    pub function_path: String,
    pub lifecycle: EntityScriptLifecycle,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

#[derive(Reflect, Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum SceneSystemSchedule {
    Startup,
    #[default]
    Update,
    FixedUpdate,
    PostUpdate,
}

#[derive(Reflect, Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum EntityScriptLifecycle {
    Start,
    #[default]
    Update,
    FixedUpdate,
    PostUpdate,
}

impl EntityScriptLifecycle {
    pub const ALL: [Self; 4] = [
        Self::Start,
        Self::Update,
        Self::FixedUpdate,
        Self::PostUpdate,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Update => "Update",
            Self::FixedUpdate => "FixedUpdate",
            Self::PostUpdate => "PostUpdate",
        }
    }
}

impl SceneSystemSchedule {
    pub const ALL: [Self; 4] = [
        Self::Startup,
        Self::Update,
        Self::FixedUpdate,
        Self::PostUpdate,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
            Self::Update => "Update",
            Self::FixedUpdate => "FixedUpdate",
            Self::PostUpdate => "PostUpdate",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Startup => Self::Update,
            Self::Update => Self::FixedUpdate,
            Self::FixedUpdate => Self::PostUpdate,
            Self::PostUpdate => Self::Startup,
        }
    }
}

#[derive(Reflect, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SceneSystemBinding {
    /// Project-relative Rust source, stored as `res://src/...`.
    pub script_path: String,
    /// Fully-qualified Rust function path. Empty values preserve legacy
    /// file-level bindings until the user selects a concrete system.
    #[serde(default)]
    pub system_path: String,
    pub schedule: SceneSystemSchedule,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
}

impl Default for SceneSystemBinding {
    fn default() -> Self {
        Self {
            script_path: String::new(),
            system_path: String::new(),
            schedule: SceneSystemSchedule::Update,
            enabled: true,
            before: Vec::new(),
            after: Vec::new(),
        }
    }
}

impl SceneSystemBinding {
    pub fn display_name(&self) -> &str {
        if let Some(name) = self
            .system_path
            .rsplit("::")
            .next()
            .filter(|name| !name.is_empty())
        {
            return name;
        }
        Path::new(self.script_path.trim())
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Empty System")
    }
}

const fn enabled_by_default() -> bool {
    true
}

#[derive(Component, Reflect, Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[reflect(Component, Default)]
pub struct SceneModel3D {
    pub resource_path: String,
}

/// A scene-local animation player. Targets are stable scene node IDs, never
/// runtime Bevy `Entity` values, so clips remain valid after reload or nesting.
#[derive(Component, Reflect, Debug, Clone, Deserialize, Serialize, PartialEq)]
#[reflect(Component, Default)]
pub struct SceneAnimationPlayer {
    #[serde(default)]
    pub autoplay: String,
    #[serde(default = "default_animation_speed")]
    pub speed: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<SceneAnimationClip>,
}

impl Default for SceneAnimationPlayer {
    fn default() -> Self {
        Self {
            autoplay: String::new(),
            speed: 1.0,
            clips: Vec::new(),
        }
    }
}

#[derive(Reflect, Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct SceneAnimationClip {
    pub name: String,
    #[serde(default)]
    pub length: f32,
    #[serde(default)]
    pub looped: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<SceneAnimationTrack>,
}

#[derive(Reflect, Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct SceneAnimationTrack {
    pub target_node: String,
    pub property: String,
    #[serde(default)]
    pub kind: SceneAnimationTrackKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<SceneAnimationKey>,
}

#[derive(Reflect, Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum SceneAnimationTrackKind {
    #[default]
    Property,
    Transform,
    SpriteFrame,
    Bone,
    Animation,
    Event,
    Audio,
}

impl SceneAnimationTrackKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Property => "Property",
            Self::Transform => "Transform",
            Self::SpriteFrame => "SpriteFrame",
            Self::Bone => "Bone",
            Self::Animation => "Animation",
            Self::Event => "Event",
            Self::Audio => "Audio",
        }
    }

    pub fn from_label(value: &str) -> Result<Self, String> {
        match value.rsplit("::").next().unwrap_or(value) {
            "Property" => Ok(Self::Property),
            "Transform" => Ok(Self::Transform),
            "SpriteFrame" => Ok(Self::SpriteFrame),
            "Bone" => Ok(Self::Bone),
            "Animation" => Ok(Self::Animation),
            "Event" => Ok(Self::Event),
            "Audio" => Ok(Self::Audio),
            _ => Err(format!("unsupported animation track kind: {value}")),
        }
    }
}

#[derive(Reflect, Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct SceneAnimationKey {
    pub time: f32,
    /// Canonical editor value. The track property determines its runtime type.
    pub value: String,
}

const fn default_animation_speed() -> f32 {
    1.0
}

/// Editor-authored static 2D collision rectangle.
///
/// `offset` is measured from the entity's top-left corner in screen-space
/// pixels. The runtime converts it to Bevy's y-up world coordinates when it
/// builds the collision solver.
#[derive(Component, Reflect, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[reflect(Component, Default)]
pub struct SceneCollisionRect2D {
    pub size: (f32, f32),
    #[serde(default)]
    pub offset: (f32, f32),
    #[serde(default = "collision_enabled_by_default")]
    pub enabled: bool,
}

impl Default for SceneCollisionRect2D {
    fn default() -> Self {
        Self {
            size: (128.0, 32.0),
            offset: (0.0, 0.0),
            enabled: true,
        }
    }
}

const fn collision_enabled_by_default() -> bool {
    true
}

#[derive(Component, Reflect, Debug, Clone, Deserialize, Serialize, PartialEq)]
#[reflect(Component, Default)]
pub struct SceneUiContent {
    pub text: String,
    pub panel_color: (f32, f32, f32, f32),
    pub image_path: String,
}

impl Default for SceneUiContent {
    fn default() -> Self {
        Self {
            text: String::new(),
            panel_color: (0.18, 0.19, 0.22, 1.0),
            image_path: String::new(),
        }
    }
}

#[derive(Component, Reflect, Debug, Clone, Deserialize, Serialize, PartialEq)]
#[reflect(Component, Default)]
pub struct SceneSprite2D {
    pub image_path: String,
    pub color: (f32, f32, f32, f32),
    pub flip_x: bool,
    pub flip_y: bool,
    /// Number of horizontal cells in the sprite sheet.
    #[serde(default = "default_sprite_frame_count")]
    pub hframes: u32,
    /// Number of vertical cells in the sprite sheet.
    #[serde(default = "default_sprite_frame_count")]
    pub vframes: u32,
    /// Zero-based sprite-sheet frame index.
    #[serde(default)]
    pub frame: u32,
    #[serde(default = "default_sprite_visible")]
    pub visible: bool,
    #[serde(default)]
    pub region_enabled: bool,
    /// Source rectangle `(x, y, width, height)` in texture pixels.
    #[serde(default)]
    pub region_rect: (f32, f32, f32, f32),
    #[serde(default)]
    pub z_index: i32,
    /// Normalized pivot measured from the image's top-left corner.
    pub anchor: (f32, f32),
}

impl Default for SceneSprite2D {
    fn default() -> Self {
        Self {
            image_path: String::new(),
            color: (1.0, 1.0, 1.0, 1.0),
            flip_x: false,
            flip_y: false,
            hframes: 1,
            vframes: 1,
            frame: 0,
            visible: true,
            region_enabled: false,
            region_rect: (0.0, 0.0, 64.0, 64.0),
            z_index: 0,
            anchor: (0.0, 0.0),
        }
    }
}

const fn default_sprite_visible() -> bool {
    true
}

const fn default_sprite_frame_count() -> u32 {
    1
}

impl SceneSprite2D {
    /// Returns a non-zero sprite-sheet frame count without overflowing.
    pub fn frame_count(&self) -> u32 {
        self.hframes.max(1).saturating_mul(self.vframes.max(1))
    }

    /// Returns the frame that can actually be rendered by this sheet.
    pub fn clamped_frame(&self) -> u32 {
        self.frame.min(self.frame_count().saturating_sub(1))
    }
}

pub fn scene_sprite_rect(sprite: &SceneSprite2D) -> Option<Rect> {
    if !sprite.region_enabled {
        return None;
    }
    let position = Vec2::new(sprite.region_rect.0.max(0.0), sprite.region_rect.1.max(0.0));
    let size = Vec2::new(sprite.region_rect.2.max(1.0), sprite.region_rect.3.max(1.0));
    Some(Rect::from_corners(position, position + size))
}

/// Calculates the texture rectangle for the active sprite-sheet frame.
///
/// When Region is enabled it becomes the sheet's source area. A 1x1 sheet
/// keeps the legacy Region behavior unchanged.
pub fn scene_sprite_frame_rect(sprite: &SceneSprite2D, texture_size: Vec2) -> Option<Rect> {
    let hframes = sprite.hframes.max(1);
    let vframes = sprite.vframes.max(1);
    if hframes == 1 && vframes == 1 {
        return scene_sprite_rect(sprite);
    }

    let source = scene_sprite_rect(sprite)
        .unwrap_or_else(|| Rect::from_corners(Vec2::ZERO, texture_size.max(Vec2::ONE)));
    let cell_size = source.size() / Vec2::new(hframes as f32, vframes as f32);
    let frame = sprite.clamped_frame();
    let column = frame % hframes;
    let row = frame / hframes;
    let position = source.min + cell_size * Vec2::new(column as f32, row as f32);
    Some(Rect::from_corners(position, position + cell_size))
}

/// Returns the path understood by a game's default `AssetServer`, whose root
/// is the project's `assets` directory.
///
/// Editor-authored values are stored as `res://...`, while the editor preview
/// may use its named `project://` source. Both forms resolve to the same
/// project-relative asset path here.
pub fn scene_image_asset_path(value: &str) -> Option<String> {
    let mut path = value.trim().replace('\\', "/");
    if path.is_empty() || path == "res://" || path == "project://" {
        return None;
    }
    path = path
        .strip_prefix("project://")
        .or_else(|| path.strip_prefix("res://"))
        .unwrap_or(&path)
        .trim_start_matches('/')
        .to_owned();
    path = path.strip_prefix("assets/").unwrap_or(&path).to_owned();
    let segments: Vec<_> = path.split('/').collect();
    if segments.is_empty()
        || segments
            .iter()
            .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
        || segments[0].contains(':')
    {
        return None;
    }
    Some(segments.join("/"))
}

/// Canonical scene-file representation for an image resource.
pub fn scene_image_resource_path(value: &str) -> Option<String> {
    scene_image_asset_path(value).map(|path| format!("res://{path}"))
}

/// Returns a 3D scene path understood by Bevy's asset loaders.
pub fn scene_model_asset_path(value: &str) -> Option<String> {
    let path = scene_image_asset_path(value)?;
    Path::new(&path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "fbx" | "gltf" | "glb"
            )
        })
        .then_some(format!("{path}#Scene0"))
}

/// Canonical scene-file representation for an imported 3D model.
pub fn scene_model_resource_path(value: &str) -> Option<String> {
    scene_model_asset_path(value)
        .and_then(|path| path.strip_suffix("#Scene0").map(str::to_owned))
        .map(|path| format!("res://{path}"))
}

#[derive(Component, Reflect, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[reflect(Component, Default)]
pub struct SceneUiLayout {
    pub anchor_min: (f32, f32),
    pub anchor_max: (f32, f32),
    pub offset: (f32, f32),
    pub size: (f32, f32),
    #[serde(default)]
    pub minimum_size: (f32, f32),
    #[serde(default)]
    pub clip_contents: bool,
    #[serde(default)]
    pub rotation: f32,
    #[serde(default = "default_ui_scale")]
    pub scale: (f32, f32),
    #[serde(default)]
    pub pivot_offset: (f32, f32),
    #[serde(default)]
    pub pivot_ratio: (f32, f32),
    /// Left, top, right, and bottom margins in logical pixels.
    pub margin: (f32, f32, f32, f32),
    pub horizontal_alignment: UiAlignment,
    pub vertical_alignment: UiAlignment,
}

impl SceneUiLayout {
    pub const fn sized(width: f32, height: f32) -> Self {
        Self {
            anchor_min: (0.0, 0.0),
            anchor_max: (0.0, 0.0),
            offset: (0.0, 0.0),
            size: (width, height),
            minimum_size: (0.0, 0.0),
            clip_contents: false,
            rotation: 0.0,
            scale: (1.0, 1.0),
            pivot_offset: (0.0, 0.0),
            pivot_ratio: (0.0, 0.0),
            margin: (0.0, 0.0, 0.0, 0.0),
            horizontal_alignment: UiAlignment::Start,
            vertical_alignment: UiAlignment::Start,
        }
    }
}

impl Default for SceneUiLayout {
    fn default() -> Self {
        Self::sized(100.0, 100.0)
    }
}

const fn default_ui_scale() -> (f32, f32) {
    (1.0, 1.0)
}

#[derive(Reflect, Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UiAlignment {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSceneNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub order: u32,
    pub kind: String,
}

/// Runtime-visible custom component payload for a scene entity.
///
/// Games can consume this metadata directly. A project-side reflection
/// registry may additionally materialize the payload as concrete ECS types.
#[derive(Component, Reflect, Debug, Clone, Default, PartialEq, Eq)]
#[reflect(Component, Default)]
pub struct RuntimeCustomComponents(pub Vec<SceneCustomComponent>);

#[derive(Component, Reflect, Debug, Clone, Default, PartialEq, Eq)]
#[reflect(Component, Default)]
pub struct RuntimeEntityScript(pub Option<SceneEntityScript>);

/// Scene-authored system bindings attached to one runtime entity.
#[derive(Component, Reflect, Debug, Clone, Default, PartialEq, Eq)]
#[reflect(Component, Default)]
pub struct RuntimeSystemBindings(pub Vec<SceneSystemBinding>);

#[derive(Clone, Copy)]
struct RegisteredEntityScript {
    source_path: &'static str,
    function_path: &'static str,
    lifecycle: EntityScriptLifecycle,
    id: SystemId<In<Entity>>,
}

#[derive(Resource, Default)]
struct EntityScriptRuntime {
    scripts: Vec<RegisteredEntityScript>,
    /// A source file may expose more than one Start callback. Track the
    /// concrete function so every enabled callback runs exactly once.
    started: HashSet<(Entity, &'static str)>,
    reported_missing: HashSet<String>,
}

/// 注册一个由编辑器绑定的 Entity Script 生命周期函数。
///
/// `source_path` 必须与 Inspector 保存的项目资源路径一致，例如
/// `res://src/player.rs`。函数通过 `In<Entity>` 接收具体场景实体，其余参数
/// 可以继续使用普通 Bevy System 参数。
pub fn add_arisna_entity_script<'a, M>(
    app: &'a mut App,
    lifecycle: EntityScriptLifecycle,
    source_path: &'static str,
    system: impl IntoSystem<In<Entity>, (), M> + 'static,
) -> &'a mut App {
    add_arisna_entity_script_fn(app, lifecycle, source_path, source_path, system)
}

/// Revy-branded alias for [`add_arisna_entity_script`].
pub fn add_revy_entity_script<'a, M>(
    app: &'a mut App,
    lifecycle: EntityScriptLifecycle,
    source_path: &'static str,
    system: impl IntoSystem<In<Entity>, (), M> + 'static,
) -> &'a mut App {
    add_arisna_entity_script(app, lifecycle, source_path, system)
}

pub fn add_arisna_entity_script_fn<'a, M>(
    app: &'a mut App,
    lifecycle: EntityScriptLifecycle,
    source_path: &'static str,
    function_path: &'static str,
    system: impl IntoSystem<In<Entity>, (), M> + 'static,
) -> &'a mut App {
    ensure_entity_script_runtime(app);
    let id = app.world_mut().register_system(system);
    app.world_mut()
        .resource_mut::<EntityScriptRuntime>()
        .scripts
        .push(RegisteredEntityScript {
            source_path,
            function_path,
            lifecycle,
            id,
        });
    app
}

/// Revy-branded alias for [`add_arisna_entity_script_fn`].
pub fn add_revy_entity_script_fn<'a, M>(
    app: &'a mut App,
    lifecycle: EntityScriptLifecycle,
    source_path: &'static str,
    function_path: &'static str,
    system: impl IntoSystem<In<Entity>, (), M> + 'static,
) -> &'a mut App {
    add_arisna_entity_script_fn(app, lifecycle, source_path, function_path, system)
}

fn ensure_entity_script_runtime(app: &mut App) {
    if !app.world().contains_resource::<EntityScriptRuntime>() {
        app.init_resource::<EntityScriptRuntime>()
            .add_systems(
                Update,
                (dispatch_entity_script_start, dispatch_entity_script_update).chain(),
            )
            .add_systems(FixedUpdate, dispatch_entity_script_fixed_update)
            .add_systems(PostUpdate, dispatch_entity_script_post_update);
    }
}

fn dispatch_entity_script_start(world: &mut World) {
    dispatch_entity_scripts(world, EntityScriptLifecycle::Start);
}

fn dispatch_entity_script_update(world: &mut World) {
    dispatch_entity_scripts(world, EntityScriptLifecycle::Update);
}

fn dispatch_entity_script_fixed_update(world: &mut World) {
    dispatch_entity_scripts(world, EntityScriptLifecycle::FixedUpdate);
}

fn dispatch_entity_script_post_update(world: &mut World) {
    dispatch_entity_scripts(world, EntityScriptLifecycle::PostUpdate);
}

fn dispatch_entity_scripts(world: &mut World, lifecycle: EntityScriptLifecycle) {
    let bindings = {
        let mut query = world.query::<(Entity, &RuntimeEntityScript)>();
        query
            .iter(world)
            .filter_map(|(entity, binding)| {
                binding
                    .0
                    .as_ref()
                    .filter(|script| script.enabled && !script.source_path.is_empty())
                    .map(|script| (entity, script.source_path.clone()))
            })
            .collect::<Vec<_>>()
    };
    if bindings.is_empty() {
        return;
    }
    let registered = world.resource::<EntityScriptRuntime>().scripts.clone();
    for (entity, source_path) in bindings {
        let authored_callbacks = world
            .get::<RuntimeEntityScript>(entity)
            .and_then(|binding| binding.0.as_ref())
            .map(|script| script.callbacks.clone())
            .unwrap_or_default();
        for authored in authored_callbacks
            .iter()
            .filter(|callback| callback.enabled && callback.lifecycle == lifecycle)
        {
            let is_registered = registered.iter().any(|script| {
                script.source_path == source_path
                    && script.lifecycle == lifecycle
                    && script.function_path == authored.function_path
            });
            let missing_key = format!("{source_path}::{}", authored.function_path);
            if !is_registered
                && world
                    .resource_mut::<EntityScriptRuntime>()
                    .reported_missing
                    .insert(missing_key)
            {
                error!(
                    "Entity script callback {} from {source_path} is attached but not registered.",
                    authored.function_path
                );
            }
        }
        let callbacks = registered
            .iter()
            .filter(|script| script.source_path == source_path && script.lifecycle == lifecycle)
            .filter(|script| {
                authored_callbacks.is_empty()
                    || authored_callbacks.iter().any(|callback| {
                        callback.enabled
                            && callback.lifecycle == lifecycle
                            && callback.function_path == script.function_path
                    })
            })
            .copied()
            .collect::<Vec<_>>();
        if callbacks.is_empty() {
            let has_any_registration = registered
                .iter()
                .any(|script| script.source_path == source_path);
            if authored_callbacks.is_empty()
                && !has_any_registration
                && world
                    .resource_mut::<EntityScriptRuntime>()
                    .reported_missing
                    .insert(source_path.clone())
            {
                error!(
                    "Entity script {source_path} is attached but not registered. Call add_arisna_entity_script before App::run."
                );
            }
            continue;
        }
        for callback in callbacks {
            if lifecycle == EntityScriptLifecycle::Start
                && world
                    .resource::<EntityScriptRuntime>()
                    .started
                    .contains(&(entity, callback.function_path))
            {
                continue;
            }
            match world.run_system_with(callback.id, entity) {
                Ok(()) => {
                    if lifecycle == EntityScriptLifecycle::Start {
                        world
                            .resource_mut::<EntityScriptRuntime>()
                            .started
                            .insert((entity, callback.function_path));
                    }
                }
                Err(error) => error!(
                    "Could not run Entity script {} for {:?}: {error}",
                    callback.source_path, entity
                ),
            }
        }
    }
    let live_entities = {
        let mut query = world.query::<Entity>();
        query.iter(world).collect::<HashSet<_>>()
    };
    world
        .resource_mut::<EntityScriptRuntime>()
        .started
        .retain(|(entity, _)| live_entities.contains(entity));
}

#[derive(Clone, Copy)]
struct RegisteredRevySystem {
    path: &'static str,
    id: SystemId,
}

#[derive(Resource, Default)]
struct RevySystemRuntime {
    systems: Vec<RegisteredRevySystem>,
    startup_ran: HashSet<&'static str>,
    reported_missing: HashSet<String>,
}

/// 注册一个受场景显式绑定控制的项目 Rust System。
///
/// Rust 函数和强类型 ECS 参数仍由游戏代码拥有；编辑器只持久化稳定函数路径、
/// Schedule 和顺序元数据。
pub fn add_arisna_system<'a, M>(
    app: &'a mut App,
    _default_schedule: SceneSystemSchedule,
    system_path: &'static str,
    system: impl IntoSystem<(), (), M> + 'static,
) -> &'a mut App {
    if !app.world().contains_resource::<RevySystemRuntime>() {
        app.init_resource::<RevySystemRuntime>()
            .add_systems(
                Update,
                (dispatch_startup_systems, dispatch_update_systems).chain(),
            )
            .add_systems(FixedUpdate, dispatch_fixed_update_systems)
            .add_systems(PostUpdate, dispatch_post_update_systems);
    }
    let id = app.world_mut().register_system(system);
    app.world_mut()
        .resource_mut::<RevySystemRuntime>()
        .systems
        .push(RegisteredRevySystem {
            path: system_path,
            id,
        });
    app
}

/// Revy-branded alias for [`add_arisna_system`].
pub fn add_revy_system<'a, M>(
    app: &'a mut App,
    default_schedule: SceneSystemSchedule,
    system_path: &'static str,
    system: impl IntoSystem<(), (), M> + 'static,
) -> &'a mut App {
    add_arisna_system(app, default_schedule, system_path, system)
}

fn dispatch_startup_systems(world: &mut World) {
    dispatch_arisna_systems(world, SceneSystemSchedule::Startup);
}

fn dispatch_update_systems(world: &mut World) {
    dispatch_arisna_systems(world, SceneSystemSchedule::Update);
}

fn dispatch_fixed_update_systems(world: &mut World) {
    dispatch_arisna_systems(world, SceneSystemSchedule::FixedUpdate);
}

fn dispatch_post_update_systems(world: &mut World) {
    dispatch_arisna_systems(world, SceneSystemSchedule::PostUpdate);
}

fn dispatch_arisna_systems(world: &mut World, schedule: SceneSystemSchedule) {
    let bindings = {
        let mut query = world.query::<&RuntimeSystemBindings>();
        query
            .iter(world)
            .flat_map(|bindings| bindings.0.iter())
            .filter(|binding| binding.enabled && binding.schedule == schedule)
            .cloned()
            .collect::<Vec<_>>()
    };
    if bindings.is_empty() {
        return;
    }
    let registered = world.resource::<RevySystemRuntime>().systems.clone();
    for binding in &bindings {
        if binding.system_path.trim().is_empty() {
            continue;
        }
        if !registered
            .iter()
            .any(|system| binding_matches_system(binding, system.path))
        {
            let key = format!("{}::{:?}", binding.system_path, schedule);
            if world
                .resource_mut::<RevySystemRuntime>()
                .reported_missing
                .insert(key)
            {
                error!(
                    "Scene system {} is enabled for {:?} but was not registered.",
                    binding.system_path, schedule
                );
            }
        }
    }
    let mut active = registered
        .into_iter()
        .filter_map(|system| {
            bindings
                .iter()
                .find(|binding| binding_matches_system(binding, system.path))
                .cloned()
                .map(|binding| (system, binding))
        })
        .collect::<Vec<_>>();
    if schedule == SceneSystemSchedule::Startup {
        let ran = &world.resource::<RevySystemRuntime>().startup_ran;
        active.retain(|(system, _)| !ran.contains(system.path));
    }
    let ordered = order_bound_systems(&active);
    for system in ordered {
        match world.run_system(system.id) {
            Ok(()) => {
                if schedule == SceneSystemSchedule::Startup {
                    world
                        .resource_mut::<RevySystemRuntime>()
                        .startup_ran
                        .insert(system.path);
                }
            }
            Err(error) => error!("Could not run scene system {}: {error}", system.path),
        }
    }
}

fn binding_matches_system(binding: &SceneSystemBinding, system_path: &str) -> bool {
    if !binding.system_path.is_empty() {
        return binding.system_path == system_path;
    }
    let function_name = system_path.rsplit("::").next().unwrap_or(system_path);
    binding.display_name() == function_name
}

fn order_bound_systems(
    active: &[(RegisteredRevySystem, SceneSystemBinding)],
) -> Vec<RegisteredRevySystem> {
    let mut incoming = vec![0usize; active.len()];
    let mut edges = vec![Vec::new(); active.len()];
    let resolve = |name: &str| {
        active.iter().position(|(system, _)| {
            system.path == name || system.path.rsplit("::").next() == Some(name)
        })
    };
    for (index, (_, binding)) in active.iter().enumerate() {
        for target in &binding.before {
            if let Some(target) = resolve(target)
                && target != index
                && !edges[index].contains(&target)
            {
                edges[index].push(target);
                incoming[target] += 1;
            }
        }
        for target in &binding.after {
            if let Some(target) = resolve(target)
                && target != index
                && !edges[target].contains(&index)
            {
                edges[target].push(index);
                incoming[index] += 1;
            }
        }
    }
    let mut ready = (0..active.len())
        .filter(|&index| incoming[index] == 0)
        .collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(active.len());
    while !ready.is_empty() {
        let index = ready.remove(0);
        ordered.push(active[index].0);
        for &next in &edges[index] {
            incoming[next] -= 1;
            if incoming[next] == 0 {
                ready.push(next);
                ready.sort_unstable();
            }
        }
    }
    for (index, (system, _)) in active.iter().enumerate() {
        if incoming[index] > 0 {
            ordered.push(*system);
        }
    }
    ordered
}

#[derive(Resource, Debug, Clone)]
pub struct ActiveScene {
    pub relative_path: PathBuf,
    pub root_id: String,
}

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GamePaused(pub bool);

#[derive(Resource, Debug, Clone)]
struct SceneLaunch {
    relative_path: PathBuf,
    control_path: Option<PathBuf>,
}

#[derive(Resource, Debug)]
struct SceneHotReloadState {
    path: PathBuf,
    display_path: String,
    applied_hash: u64,
    observed_hash: u64,
    last_check: f64,
    last_error: Option<String>,
}

#[derive(Component)]
struct RuntimeGeneratedUiContent;

#[derive(Component)]
struct RuntimeGeneratedCamera;

/// 把编辑器场景加载到独立游戏 World 的运行时插件。
///
/// 插件从命令行接收项目相对场景路径，不接受任意外部绝对路径。
pub struct SceneRuntimePlugin {
    relative_path: PathBuf,
    control_path: Option<PathBuf>,
}

impl SceneRuntimePlugin {
    pub fn new(relative_path: impl Into<PathBuf>) -> Self {
        Self {
            relative_path: relative_path.into(),
            control_path: None,
        }
    }

    pub fn with_control_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.control_path = Some(path.into());
        self
    }

    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Result<Option<Self>, String> {
        let mut args = args.into_iter();
        let mut scene = None;
        let mut control = None;
        while let Some(argument) = args.next() {
            if argument == "--scene" {
                scene = Some(
                    args.next()
                        .map(PathBuf::from)
                        .ok_or_else(|| "--scene requires a project-relative path".to_string())?,
                );
            } else if argument == "--control" {
                control = Some(
                    args.next()
                        .map(PathBuf::from)
                        .ok_or_else(|| "--control requires a file path".to_string())?,
                );
            }
        }
        let Some(scene) = scene else {
            return Ok(None);
        };
        let mut plugin = Self::new(scene);
        if let Some(control) = control {
            plugin = plugin.with_control_file(control);
        }
        Ok(Some(plugin))
    }
}

impl Plugin for SceneRuntimePlugin {
    fn build(&self, app: &mut App) {
        ensure_entity_script_runtime(app);
        app.insert_resource(SceneLaunch {
            relative_path: self.relative_path.clone(),
            control_path: self.control_path.clone(),
        })
        .init_resource::<GamePaused>()
        .add_systems(Startup, load_active_scene)
        .add_systems(
            Update,
            (
                materialize_bsn_scene_nodes,
                hot_reload_bsn_scene,
                materialize_reflected_custom_components,
                sync_run_control,
                initialize_runtime_animation_players,
                advance_runtime_animation_players,
                apply_runtime_animations,
                sync_runtime_sprite_render,
            )
                .chain(),
        );
    }
}

pub fn load_scene_file(path: &Path) -> Result<SceneFile, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    parse_scene_source(path, &source)
}

fn parse_scene_source(path: &Path, source: &str) -> Result<SceneFile, String> {
    let scene: SceneFile = if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bsn"))
    {
        scene_file_from_bsn(&source)?
    } else {
        ron::from_str(&source).map_err(|error| format!("invalid legacy scene: {error}"))?
    };
    validate_scene(&scene)?;
    Ok(scene)
}

fn scene_content_hash(content: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Serializes Revy's persistent scene metadata and hierarchy as genuine Bevy Scene Notation.
pub fn scene_file_to_bsn(scene: &SceneFile) -> Result<String, String> {
    validate_scene(scene)?;
    let nodes: HashMap<_, _> = scene
        .entities
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut children: HashMap<Option<&str>, Vec<&SceneNodeData>> = HashMap::new();
    for node in &scene.entities {
        children
            .entry(node.parent.as_deref())
            .or_default()
            .push(node);
    }
    for siblings in children.values_mut() {
        siblings.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.id.cmp(&right.id))
        });
    }
    let root = nodes
        .get(scene.root.as_str())
        .copied()
        .ok_or_else(|| "root entity is missing".to_string())?;
    let document = BsnDocument {
        root: scene_node_to_bsn(root, &children),
    };
    Ok(format_bsn(&document))
}

fn scene_node_to_bsn(
    node: &SceneNodeData,
    children: &HashMap<Option<&str>, Vec<&SceneNodeData>>,
) -> BsnEntity {
    let mut components = vec![
        bsn_component(
            "Name",
            BsnComponentBody::Tuple(vec![BsnValue::String(node.name.clone())]),
        ),
        bsn_component(
            "SceneNodeData",
            BsnComponentBody::Struct(scene_node_fields(node)),
        ),
    ];
    if is_spatial_kind(&node.kind)
        || node.components.iter().any(|component| {
            matches!(
                component.as_str(),
                "transform"
                    | "sprite"
                    | "camera2d"
                    | "mesh3d"
                    | "camera3d"
                    | "directional_light3d"
                    | "point_light3d"
                    | "spot_light3d"
            )
        })
    {
        components.push(bsn_component(
            "Transform",
            BsnComponentBody::Struct(vec![
                bsn_field(
                    "translation",
                    constructor(
                        "Vec3",
                        [
                            number(node.translation.0),
                            number(node.translation.1),
                            number(node.translation.2),
                        ],
                    ),
                ),
                bsn_field(
                    "rotation",
                    constructor(
                        "Quat",
                        [
                            number(node.rotation.0),
                            number(node.rotation.1),
                            number(node.rotation.2),
                            number(node.rotation.3),
                        ],
                    ),
                ),
                bsn_field(
                    "scale",
                    constructor(
                        "Vec3",
                        [
                            number(node.scale.0),
                            number(node.scale.1),
                            number(node.scale.2),
                        ],
                    ),
                ),
            ]),
        ));
    }
    BsnEntity {
        name: None,
        cached_scene: None,
        components,
        children: children
            .get(&Some(node.id.as_str()))
            .into_iter()
            .flat_map(|children| children.iter())
            .map(|child| scene_node_to_bsn(child, children))
            .collect(),
        span: BsnSpan::default(),
    }
}

fn scene_node_fields(node: &SceneNodeData) -> Vec<BsnStructField> {
    vec![
        bsn_field("id", BsnValue::String(node.id.clone())),
        bsn_field("parent", option_string(node.parent.as_deref())),
        bsn_field("order", BsnValue::Number(node.order.to_string())),
        bsn_field("name", BsnValue::String(node.name.clone())),
        bsn_field("kind", BsnValue::String(node.kind.clone())),
        bsn_field("space", option_string(node.space.as_deref())),
        bsn_field(
            "components",
            BsnValue::List(
                node.components
                    .iter()
                    .cloned()
                    .map(BsnValue::String)
                    .collect(),
            ),
        ),
        bsn_field(
            "custom_components",
            custom_components_value(&node.custom_components),
        ),
        bsn_field(
            "entity_script",
            option_entity_script(node.entity_script.as_ref()),
        ),
        bsn_field("systems", system_bindings_value(&node.systems)),
        bsn_field("ui_layout", option_ui_layout(node.ui_layout)),
        bsn_field("ui_content", option_ui_content(node.ui_content.as_ref())),
        bsn_field("sprite", option_sprite(node.sprite.as_ref())),
        bsn_field("model", option_model(node.model.as_ref())),
        bsn_field(
            "animation_player",
            option_animation_player(node.animation_player.as_ref()),
        ),
        bsn_field(
            "collision_rect",
            option_collision_rect(node.collision_rect.as_ref()),
        ),
        bsn_field(
            "translation",
            BsnValue::Tuple(vec![
                number(node.translation.0),
                number(node.translation.1),
                number(node.translation.2),
            ]),
        ),
        bsn_field(
            "rotation",
            BsnValue::Tuple(vec![
                number(node.rotation.0),
                number(node.rotation.1),
                number(node.rotation.2),
                number(node.rotation.3),
            ]),
        ),
        bsn_field(
            "scale",
            BsnValue::Tuple(vec![
                number(node.scale.0),
                number(node.scale.1),
                number(node.scale.2),
            ]),
        ),
    ]
}

fn custom_components_value(components: &[SceneCustomComponent]) -> BsnValue {
    BsnValue::List(
        components
            .iter()
            .map(|component| BsnValue::Struct {
                type_path: Some("SceneCustomComponent".into()),
                fields: vec![
                    bsn_field("type_path", BsnValue::String(component.type_path.clone())),
                    bsn_field(
                        "source_path",
                        BsnValue::String(component.source_path.clone()),
                    ),
                    bsn_field(
                        "fields",
                        BsnValue::List(
                            component
                                .fields
                                .iter()
                                .map(|field| BsnValue::Struct {
                                    type_path: Some("SceneCustomField".into()),
                                    fields: vec![
                                        bsn_field("name", BsnValue::String(field.name.clone())),
                                        bsn_field(
                                            "type_name",
                                            BsnValue::String(field.type_name.clone()),
                                        ),
                                        bsn_field("value", BsnValue::String(field.value.clone())),
                                    ],
                                })
                                .collect(),
                        ),
                    ),
                ],
            })
            .collect(),
    )
}

fn option_entity_script(script: Option<&SceneEntityScript>) -> BsnValue {
    match script {
        None => BsnValue::Path("None".into()),
        Some(script) => BsnValue::Constructor {
            type_path: "Some".into(),
            fields: vec![BsnValue::Struct {
                type_path: Some("SceneEntityScript".into()),
                fields: vec![
                    bsn_field("source_path", BsnValue::String(script.source_path.clone())),
                    bsn_field("type_path", BsnValue::String(script.type_path.clone())),
                    bsn_field("enabled", BsnValue::Bool(script.enabled)),
                    bsn_field(
                        "callbacks",
                        BsnValue::List(
                            script
                                .callbacks
                                .iter()
                                .map(|callback| BsnValue::Struct {
                                    type_path: Some("SceneEntityScriptCallback".into()),
                                    fields: vec![
                                        bsn_field(
                                            "function_path",
                                            BsnValue::String(callback.function_path.clone()),
                                        ),
                                        bsn_field(
                                            "lifecycle",
                                            BsnValue::Path(callback.lifecycle.label().into()),
                                        ),
                                        bsn_field("enabled", BsnValue::Bool(callback.enabled)),
                                    ],
                                })
                                .collect(),
                        ),
                    ),
                ],
            }],
        },
    }
}

fn system_bindings_value(bindings: &[SceneSystemBinding]) -> BsnValue {
    BsnValue::List(
        bindings
            .iter()
            .map(|binding| BsnValue::Struct {
                type_path: Some("SceneSystemBinding".into()),
                fields: vec![
                    bsn_field("script_path", BsnValue::String(binding.script_path.clone())),
                    bsn_field("system_path", BsnValue::String(binding.system_path.clone())),
                    bsn_field("schedule", BsnValue::Path(binding.schedule.label().into())),
                    bsn_field("enabled", BsnValue::Bool(binding.enabled)),
                    bsn_field(
                        "before",
                        BsnValue::List(
                            binding
                                .before
                                .iter()
                                .cloned()
                                .map(BsnValue::String)
                                .collect(),
                        ),
                    ),
                    bsn_field(
                        "after",
                        BsnValue::List(
                            binding
                                .after
                                .iter()
                                .cloned()
                                .map(BsnValue::String)
                                .collect(),
                        ),
                    ),
                ],
            })
            .collect(),
    )
}

fn option_ui_layout(layout: Option<SceneUiLayout>) -> BsnValue {
    match layout {
        None => BsnValue::Path("None".into()),
        Some(layout) => BsnValue::Constructor {
            type_path: "Some".into(),
            fields: vec![BsnValue::Struct {
                type_path: Some("SceneUiLayout".into()),
                fields: vec![
                    bsn_field(
                        "anchor_min",
                        BsnValue::Tuple(vec![
                            number(layout.anchor_min.0),
                            number(layout.anchor_min.1),
                        ]),
                    ),
                    bsn_field(
                        "anchor_max",
                        BsnValue::Tuple(vec![
                            number(layout.anchor_max.0),
                            number(layout.anchor_max.1),
                        ]),
                    ),
                    bsn_field(
                        "offset",
                        BsnValue::Tuple(vec![number(layout.offset.0), number(layout.offset.1)]),
                    ),
                    bsn_field(
                        "size",
                        BsnValue::Tuple(vec![number(layout.size.0), number(layout.size.1)]),
                    ),
                    bsn_field(
                        "minimum_size",
                        BsnValue::Tuple(vec![
                            number(layout.minimum_size.0),
                            number(layout.minimum_size.1),
                        ]),
                    ),
                    bsn_field("clip_contents", BsnValue::Bool(layout.clip_contents)),
                    bsn_field("rotation", number(layout.rotation)),
                    bsn_field(
                        "scale",
                        BsnValue::Tuple(vec![number(layout.scale.0), number(layout.scale.1)]),
                    ),
                    bsn_field(
                        "pivot_offset",
                        BsnValue::Tuple(vec![
                            number(layout.pivot_offset.0),
                            number(layout.pivot_offset.1),
                        ]),
                    ),
                    bsn_field(
                        "pivot_ratio",
                        BsnValue::Tuple(vec![
                            number(layout.pivot_ratio.0),
                            number(layout.pivot_ratio.1),
                        ]),
                    ),
                    bsn_field(
                        "margin",
                        BsnValue::Tuple(vec![
                            number(layout.margin.0),
                            number(layout.margin.1),
                            number(layout.margin.2),
                            number(layout.margin.3),
                        ]),
                    ),
                    bsn_field(
                        "horizontal_alignment",
                        BsnValue::Path(format!(
                            "UiAlignment::{}",
                            alignment_variant(layout.horizontal_alignment)
                        )),
                    ),
                    bsn_field(
                        "vertical_alignment",
                        BsnValue::Path(format!(
                            "UiAlignment::{}",
                            alignment_variant(layout.vertical_alignment)
                        )),
                    ),
                ],
            }],
        },
    }
}

fn option_ui_content(content: Option<&SceneUiContent>) -> BsnValue {
    match content {
        None => BsnValue::Path("None".into()),
        Some(content) => BsnValue::Constructor {
            type_path: "Some".into(),
            fields: vec![BsnValue::Struct {
                type_path: Some("SceneUiContent".into()),
                fields: vec![
                    bsn_field("text", BsnValue::String(content.text.clone())),
                    bsn_field(
                        "panel_color",
                        BsnValue::Tuple(vec![
                            number(content.panel_color.0),
                            number(content.panel_color.1),
                            number(content.panel_color.2),
                            number(content.panel_color.3),
                        ]),
                    ),
                    bsn_field("image_path", BsnValue::String(content.image_path.clone())),
                ],
            }],
        },
    }
}

fn option_sprite(sprite: Option<&SceneSprite2D>) -> BsnValue {
    match sprite {
        None => BsnValue::Path("None".into()),
        Some(sprite) => BsnValue::Constructor {
            type_path: "Some".into(),
            fields: vec![BsnValue::Struct {
                type_path: Some("SceneSprite2D".into()),
                fields: vec![
                    bsn_field("image_path", BsnValue::String(sprite.image_path.clone())),
                    bsn_field(
                        "color",
                        BsnValue::Tuple(vec![
                            number(sprite.color.0),
                            number(sprite.color.1),
                            number(sprite.color.2),
                            number(sprite.color.3),
                        ]),
                    ),
                    bsn_field("flip_x", BsnValue::Bool(sprite.flip_x)),
                    bsn_field("flip_y", BsnValue::Bool(sprite.flip_y)),
                    bsn_field(
                        "hframes",
                        BsnValue::Number(sprite.hframes.max(1).to_string()),
                    ),
                    bsn_field(
                        "vframes",
                        BsnValue::Number(sprite.vframes.max(1).to_string()),
                    ),
                    bsn_field(
                        "frame",
                        BsnValue::Number(sprite.clamped_frame().to_string()),
                    ),
                    bsn_field("visible", BsnValue::Bool(sprite.visible)),
                    bsn_field("region_enabled", BsnValue::Bool(sprite.region_enabled)),
                    bsn_field(
                        "region_rect",
                        BsnValue::Tuple(vec![
                            number(sprite.region_rect.0),
                            number(sprite.region_rect.1),
                            number(sprite.region_rect.2),
                            number(sprite.region_rect.3),
                        ]),
                    ),
                    bsn_field("z_index", BsnValue::Number(sprite.z_index.to_string())),
                    bsn_field(
                        "anchor",
                        BsnValue::Tuple(vec![number(sprite.anchor.0), number(sprite.anchor.1)]),
                    ),
                ],
            }],
        },
    }
}

fn option_model(model: Option<&SceneModel3D>) -> BsnValue {
    match model {
        None => BsnValue::Path("None".into()),
        Some(model) => BsnValue::Constructor {
            type_path: "Some".into(),
            fields: vec![BsnValue::Struct {
                type_path: Some("SceneModel3D".into()),
                fields: vec![bsn_field(
                    "resource_path",
                    BsnValue::String(model.resource_path.clone()),
                )],
            }],
        },
    }
}

fn option_animation_player(player: Option<&SceneAnimationPlayer>) -> BsnValue {
    match player {
        None => BsnValue::Path("None".into()),
        Some(player) => BsnValue::Constructor {
            type_path: "Some".into(),
            fields: vec![BsnValue::Struct {
                type_path: Some("SceneAnimationPlayer".into()),
                fields: vec![
                    bsn_field("autoplay", BsnValue::String(player.autoplay.clone())),
                    bsn_field("speed", number(player.speed)),
                    bsn_field("clips", animation_clips_value(&player.clips)),
                ],
            }],
        },
    }
}

fn animation_clips_value(clips: &[SceneAnimationClip]) -> BsnValue {
    BsnValue::List(
        clips
            .iter()
            .map(|clip| BsnValue::Struct {
                type_path: Some("SceneAnimationClip".into()),
                fields: vec![
                    bsn_field("name", BsnValue::String(clip.name.clone())),
                    bsn_field("length", number(clip.length)),
                    bsn_field("looped", BsnValue::Bool(clip.looped)),
                    bsn_field("tracks", animation_tracks_value(&clip.tracks)),
                ],
            })
            .collect(),
    )
}

fn animation_tracks_value(tracks: &[SceneAnimationTrack]) -> BsnValue {
    BsnValue::List(
        tracks
            .iter()
            .map(|track| BsnValue::Struct {
                type_path: Some("SceneAnimationTrack".into()),
                fields: vec![
                    bsn_field("target_node", BsnValue::String(track.target_node.clone())),
                    bsn_field("property", BsnValue::String(track.property.clone())),
                    bsn_field("kind", BsnValue::Path(track.kind.label().into())),
                    bsn_field("keys", animation_keys_value(&track.keys)),
                ],
            })
            .collect(),
    )
}

fn animation_keys_value(keys: &[SceneAnimationKey]) -> BsnValue {
    BsnValue::List(
        keys.iter()
            .map(|key| BsnValue::Struct {
                type_path: Some("SceneAnimationKey".into()),
                fields: vec![
                    bsn_field("time", number(key.time)),
                    bsn_field("value", BsnValue::String(key.value.clone())),
                ],
            })
            .collect(),
    )
}

fn option_collision_rect(collision: Option<&SceneCollisionRect2D>) -> BsnValue {
    match collision {
        None => BsnValue::Path("None".into()),
        Some(collision) => BsnValue::Constructor {
            type_path: "Some".into(),
            fields: vec![BsnValue::Struct {
                type_path: Some("SceneCollisionRect2D".into()),
                fields: vec![
                    bsn_field(
                        "size",
                        BsnValue::Tuple(vec![number(collision.size.0), number(collision.size.1)]),
                    ),
                    bsn_field(
                        "offset",
                        BsnValue::Tuple(vec![
                            number(collision.offset.0),
                            number(collision.offset.1),
                        ]),
                    ),
                    bsn_field("enabled", BsnValue::Bool(collision.enabled)),
                ],
            }],
        },
    }
}

fn option_string(value: Option<&str>) -> BsnValue {
    match value {
        Some(value) => BsnValue::Constructor {
            type_path: "Some".into(),
            fields: vec![BsnValue::String(value.to_owned())],
        },
        None => BsnValue::Path("None".into()),
    }
}

fn alignment_variant(alignment: UiAlignment) -> &'static str {
    match alignment {
        UiAlignment::Start => "Start",
        UiAlignment::Center => "Center",
        UiAlignment::End => "End",
        UiAlignment::Stretch => "Stretch",
    }
}

fn bsn_component(type_path: &str, body: BsnComponentBody) -> BsnComponent {
    BsnComponent {
        type_path: type_path.into(),
        body,
        span: BsnSpan::default(),
    }
}

fn bsn_field(name: &str, value: BsnValue) -> BsnStructField {
    BsnStructField {
        name: name.into(),
        value,
        span: BsnSpan::default(),
    }
}

fn constructor<const N: usize>(type_path: &str, fields: [BsnValue; N]) -> BsnValue {
    BsnValue::Constructor {
        type_path: type_path.into(),
        fields: fields.into(),
    }
}

fn number(value: f32) -> BsnValue {
    BsnValue::Number(format!("{value:?}"))
}

/// Parses a Revy-authored BSN document back into its persistent scene metadata.
pub fn scene_file_from_bsn(source: &str) -> Result<SceneFile, String> {
    let document = parse_bsn(source).map_err(|error| format!("invalid BSN scene: {error}"))?;
    let mut entities = Vec::new();
    collect_bsn_nodes(&document.root, None, &mut entities)?;
    let root = entities
        .first()
        .map(|node| node.id.clone())
        .ok_or_else(|| "BSN scene contains no root entity".to_string())?;
    let scene = SceneFile {
        format_version: SCENE_FORMAT_VERSION,
        root,
        entities,
    };
    validate_scene(&scene)?;
    Ok(scene)
}

fn collect_bsn_nodes(
    entity: &BsnEntity,
    parent: Option<&str>,
    output: &mut Vec<SceneNodeData>,
) -> Result<(), String> {
    let component = entity
        .components
        .iter()
        .find(|component| final_type_name(&component.type_path) == "SceneNodeData")
        .ok_or_else(|| {
            format!(
                "entity at {}:{} is missing SceneNodeData",
                entity.span.line, entity.span.column
            )
        })?;
    let mut node = parse_scene_node_component(component)?;
    node.parent = parent.map(str::to_owned);
    let id = node.id.clone();
    output.push(node);
    for child in &entity.children {
        collect_bsn_nodes(child, Some(&id), output)?;
    }
    Ok(())
}

fn parse_scene_node_component(component: &BsnComponent) -> Result<SceneNodeData, String> {
    let BsnComponentBody::Struct(fields) = &component.body else {
        return Err(format!(
            "SceneNodeData at {}:{} must use named fields",
            component.span.line, component.span.column
        ));
    };
    Ok(SceneNodeData {
        id: string_field(fields, "id")?.to_owned(),
        parent: option_string_field(fields, "parent")?,
        order: u32_field(fields, "order")?,
        name: string_field(fields, "name")?.to_owned(),
        kind: string_field(fields, "kind")?.to_owned(),
        space: option_string_field(fields, "space")?,
        components: string_list_field(fields, "components")?,
        custom_components: optional_custom_components_field(fields, "custom_components")?,
        entity_script: optional_entity_script_field(fields, "entity_script")?,
        systems: optional_system_bindings_field(fields, "systems")?,
        ui_layout: option_layout_field(fields, "ui_layout")?,
        ui_content: option_content_field(fields, "ui_content")?,
        sprite: optional_sprite_field(fields, "sprite")?,
        model: optional_model_field(fields, "model")?,
        animation_player: optional_animation_player_field(fields, "animation_player")?,
        collision_rect: optional_collision_rect_field(fields, "collision_rect")?,
        translation: array3_tuple(tuple_f32_field::<3>(fields, "translation")?),
        rotation: array4_tuple(tuple_f32_field::<4>(fields, "rotation")?),
        scale: array3_tuple(tuple_f32_field::<3>(fields, "scale")?),
    })
}

fn optional_entity_script_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Option<SceneEntityScript>, String> {
    let Some(field) = fields.iter().find(|field| field.name == name) else {
        return Ok(None);
    };
    let BsnValue::Constructor { type_path, fields } = &field.value else {
        return match &field.value {
            BsnValue::Path(path) if final_type_name(path) == "None" => Ok(None),
            _ => Err("SceneEntityScript must be Some(...) or None".into()),
        };
    };
    if final_type_name(type_path) != "Some" || fields.len() != 1 {
        return Err("SceneEntityScript must use Some(...)".into());
    }
    let BsnValue::Struct { fields, .. } = &fields[0] else {
        return Err("SceneEntityScript must contain a struct".into());
    };
    Ok(Some(SceneEntityScript {
        source_path: string_field(fields, "source_path")?.to_owned(),
        type_path: optional_string_field(fields, "type_path", "")?.to_owned(),
        enabled: optional_bool_field(fields, "enabled", true)?,
        callbacks: fields
            .iter()
            .find(|field| field.name == "callbacks")
            .map(|field| parse_entity_script_callbacks(&field.value))
            .transpose()?
            .unwrap_or_default(),
    }))
}

fn parse_entity_script_callbacks(
    value: &BsnValue,
) -> Result<Vec<SceneEntityScriptCallback>, String> {
    let BsnValue::List(values) = value else {
        return Err("SceneEntityScript callbacks must be a list".into());
    };
    values
        .iter()
        .map(|value| {
            let BsnValue::Struct { fields, .. } = value else {
                return Err("SceneEntityScript callback must be a struct".into());
            };
            let BsnValue::Path(lifecycle_path) = field_value(fields, "lifecycle")? else {
                return Err("SceneEntityScript lifecycle must be a path".into());
            };
            let lifecycle = match final_type_name(lifecycle_path) {
                "Start" => EntityScriptLifecycle::Start,
                "Update" => EntityScriptLifecycle::Update,
                "FixedUpdate" => EntityScriptLifecycle::FixedUpdate,
                "PostUpdate" => EntityScriptLifecycle::PostUpdate,
                value => return Err(format!("unsupported Entity script lifecycle `{value}`")),
            };
            Ok(SceneEntityScriptCallback {
                function_path: string_field(fields, "function_path")?.to_owned(),
                lifecycle,
                enabled: optional_bool_field(fields, "enabled", true)?,
            })
        })
        .collect()
}

fn optional_custom_components_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Vec<SceneCustomComponent>, String> {
    let Some(value) = fields.iter().find(|field| field.name == name) else {
        return Ok(Vec::new());
    };
    let BsnValue::List(values) = &value.value else {
        return Err(format!("SceneNodeData field `{name}` must be a list"));
    };
    values
        .iter()
        .map(|value| {
            let BsnValue::Struct {
                type_path: Some(type_path),
                fields,
            } = value
            else {
                return Err(format!(
                    "SceneNodeData field `{name}` must contain SceneCustomComponent values"
                ));
            };
            if final_type_name(type_path) != "SceneCustomComponent" {
                return Err(format!(
                    "SceneNodeData field `{name}` contains an unsupported component type"
                ));
            }
            let field_values = match field_value(fields, "fields")? {
                BsnValue::List(values) => values,
                _ => return Err("SceneCustomComponent fields must be a list".into()),
            };
            let custom_fields = field_values
                .iter()
                .map(|value| {
                    let BsnValue::Struct {
                        type_path: Some(type_path),
                        fields,
                    } = value
                    else {
                        return Err(
                            "SceneCustomComponent fields must contain SceneCustomField values"
                                .to_string(),
                        );
                    };
                    if final_type_name(type_path) != "SceneCustomField" {
                        return Err("unsupported custom component field type".to_string());
                    }
                    Ok(SceneCustomField {
                        name: string_field(fields, "name")?.to_owned(),
                        type_name: string_field(fields, "type_name")?.to_owned(),
                        value: string_field(fields, "value")?.to_owned(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(SceneCustomComponent {
                type_path: string_field(fields, "type_path")?.to_owned(),
                source_path: string_field(fields, "source_path")?.to_owned(),
                fields: custom_fields,
            })
        })
        .collect()
}

fn field_value<'a>(fields: &'a [BsnStructField], name: &str) -> Result<&'a BsnValue, String> {
    fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.value)
        .ok_or_else(|| format!("SceneNodeData field `{name}` is missing"))
}

fn string_field<'a>(fields: &'a [BsnStructField], name: &str) -> Result<&'a str, String> {
    match field_value(fields, name)? {
        BsnValue::String(value) => Ok(value),
        _ => Err(format!("SceneNodeData field `{name}` must be a string")),
    }
}

fn option_string_field(fields: &[BsnStructField], name: &str) -> Result<Option<String>, String> {
    match field_value(fields, name)? {
        BsnValue::Path(path) if final_type_name(path) == "None" => Ok(None),
        BsnValue::Constructor { type_path, fields }
            if final_type_name(type_path) == "Some" && fields.len() == 1 =>
        {
            match &fields[0] {
                BsnValue::String(value) => Ok(Some(value.clone())),
                _ => Err(format!(
                    "SceneNodeData field `{name}` must contain a string"
                )),
            }
        }
        _ => Err(format!("SceneNodeData field `{name}` must be Some or None")),
    }
}

fn u32_field(fields: &[BsnStructField], name: &str) -> Result<u32, String> {
    match field_value(fields, name)? {
        BsnValue::Number(value) => value
            .replace('_', "")
            .trim_end_matches("u32")
            .parse()
            .map_err(|error| format!("invalid `{name}`: {error}")),
        _ => Err(format!("SceneNodeData field `{name}` must be a number")),
    }
}

fn string_list_field(fields: &[BsnStructField], name: &str) -> Result<Vec<String>, String> {
    match field_value(fields, name)? {
        BsnValue::List(values) => values
            .iter()
            .map(|value| match value {
                BsnValue::String(value) => Ok(value.clone()),
                _ => Err(format!("SceneNodeData field `{name}` must contain strings")),
            })
            .collect(),
        _ => Err(format!("SceneNodeData field `{name}` must be a list")),
    }
}

fn optional_system_bindings_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Vec<SceneSystemBinding>, String> {
    let Some(value) = fields.iter().find(|field| field.name == name) else {
        return Ok(Vec::new());
    };
    let BsnValue::List(values) = &value.value else {
        return Err(format!("SceneNodeData field `{name}` must be a list"));
    };
    values
        .iter()
        .map(|value| {
            let BsnValue::Struct {
                type_path: Some(type_path),
                fields,
            } = value
            else {
                return Err(format!(
                    "SceneNodeData field `{name}` must contain SceneSystemBinding values"
                ));
            };
            if final_type_name(type_path) != "SceneSystemBinding" {
                return Err(format!(
                    "SceneNodeData field `{name}` contains an unsupported binding type"
                ));
            }
            let script_path = string_field(fields, "script_path")?.to_owned();
            let system_path = optional_string_field(fields, "system_path", "")?.to_owned();
            let BsnValue::Path(schedule) = field_value(fields, "schedule")? else {
                return Err("SceneSystemBinding schedule must be a variant".into());
            };
            let schedule = match final_type_name(schedule) {
                "Startup" => SceneSystemSchedule::Startup,
                "Update" => SceneSystemSchedule::Update,
                "FixedUpdate" => SceneSystemSchedule::FixedUpdate,
                "PostUpdate" => SceneSystemSchedule::PostUpdate,
                value => return Err(format!("unknown system schedule `{value}`")),
            };
            Ok(SceneSystemBinding {
                script_path,
                system_path,
                schedule,
                enabled: optional_bool_field(fields, "enabled", true)?,
                before: optional_string_list_field(fields, "before")?,
                after: optional_string_list_field(fields, "after")?,
            })
        })
        .collect()
}

fn optional_string_list_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Vec<String>, String> {
    if fields.iter().any(|field| field.name == name) {
        string_list_field(fields, name)
    } else {
        Ok(Vec::new())
    }
}

fn optional_string_field<'a>(
    fields: &'a [BsnStructField],
    name: &str,
    default: &'a str,
) -> Result<&'a str, String> {
    if fields.iter().any(|field| field.name == name) {
        string_field(fields, name)
    } else {
        Ok(default)
    }
}

fn tuple_f32_field<const N: usize>(
    fields: &[BsnStructField],
    name: &str,
) -> Result<[f32; N], String> {
    let BsnValue::Tuple(values) = field_value(fields, name)? else {
        return Err(format!("SceneNodeData field `{name}` must be a tuple"));
    };
    if values.len() != N {
        return Err(format!("SceneNodeData field `{name}` expects {N} values"));
    }
    let mut output = [0.0; N];
    for (index, value) in values.iter().enumerate() {
        let BsnValue::Number(value) = value else {
            return Err(format!("SceneNodeData field `{name}` must contain numbers"));
        };
        output[index] = value
            .replace('_', "")
            .trim_end_matches("f32")
            .parse()
            .map_err(|error| format!("invalid `{name}`: {error}"))?;
    }
    Ok(output)
}

fn option_layout_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Option<SceneUiLayout>, String> {
    let Some(fields) = option_struct_fields(field_value(fields, name)?, "SceneUiLayout")? else {
        return Ok(None);
    };
    Ok(Some(SceneUiLayout {
        anchor_min: array2_tuple(tuple_f32_field::<2>(fields, "anchor_min")?),
        anchor_max: array2_tuple(tuple_f32_field::<2>(fields, "anchor_max")?),
        offset: array2_tuple(tuple_f32_field::<2>(fields, "offset")?),
        size: array2_tuple(tuple_f32_field::<2>(fields, "size")?),
        minimum_size: array2_tuple(optional_tuple_f32_field::<2>(
            fields,
            "minimum_size",
            [0.0, 0.0],
        )?),
        clip_contents: optional_bool_field(fields, "clip_contents", false)?,
        rotation: optional_f32_field(fields, "rotation", 0.0)?,
        scale: array2_tuple(optional_tuple_f32_field::<2>(fields, "scale", [1.0, 1.0])?),
        pivot_offset: array2_tuple(optional_tuple_f32_field::<2>(
            fields,
            "pivot_offset",
            [0.0, 0.0],
        )?),
        pivot_ratio: array2_tuple(optional_tuple_f32_field::<2>(
            fields,
            "pivot_ratio",
            [0.0, 0.0],
        )?),
        margin: array4_tuple(tuple_f32_field::<4>(fields, "margin")?),
        horizontal_alignment: alignment_field(fields, "horizontal_alignment")?,
        vertical_alignment: alignment_field(fields, "vertical_alignment")?,
    }))
}

fn option_content_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Option<SceneUiContent>, String> {
    let Some(fields) = option_struct_fields(field_value(fields, name)?, "SceneUiContent")? else {
        return Ok(None);
    };
    Ok(Some(SceneUiContent {
        text: string_field(fields, "text")?.to_owned(),
        panel_color: array4_tuple(tuple_f32_field::<4>(fields, "panel_color")?),
        image_path: string_field(fields, "image_path")?.to_owned(),
    }))
}

fn optional_sprite_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Option<SceneSprite2D>, String> {
    let Some(value) = fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.value)
    else {
        return Ok(None);
    };
    let Some(fields) = option_struct_fields(value, "SceneSprite2D")? else {
        return Ok(None);
    };
    let hframes = optional_u32_field(fields, "hframes", 1)?.max(1);
    let vframes = optional_u32_field(fields, "vframes", 1)?.max(1);
    let frame_count = hframes.saturating_mul(vframes);
    Ok(Some(SceneSprite2D {
        image_path: string_field(fields, "image_path")?.to_owned(),
        color: array4_tuple(tuple_f32_field::<4>(fields, "color")?),
        flip_x: bool_field(fields, "flip_x")?,
        flip_y: bool_field(fields, "flip_y")?,
        hframes,
        vframes,
        frame: optional_u32_field(fields, "frame", 0)?.min(frame_count.saturating_sub(1)),
        visible: optional_bool_field(fields, "visible", true)?,
        region_enabled: optional_bool_field(fields, "region_enabled", false)?,
        region_rect: array4_tuple(optional_tuple_f32_field::<4>(
            fields,
            "region_rect",
            [0.0, 0.0, 64.0, 64.0],
        )?),
        z_index: optional_i32_field(fields, "z_index", 0)?,
        anchor: array2_tuple(tuple_f32_field::<2>(fields, "anchor")?),
    }))
}

fn optional_model_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Option<SceneModel3D>, String> {
    let Some(value) = fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.value)
    else {
        return Ok(None);
    };
    let Some(fields) = option_struct_fields(value, "SceneModel3D")? else {
        return Ok(None);
    };
    Ok(Some(SceneModel3D {
        resource_path: string_field(fields, "resource_path")?.to_owned(),
    }))
}

fn optional_animation_player_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Option<SceneAnimationPlayer>, String> {
    let Some(value) = fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.value)
    else {
        // The field was introduced after the original v2 format.
        return Ok(None);
    };
    let Some(fields) = option_struct_fields(value, "SceneAnimationPlayer")? else {
        return Ok(None);
    };
    let clips = match field_value(fields, "clips")? {
        BsnValue::List(values) => values
            .iter()
            .map(parse_animation_clip)
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("SceneAnimationPlayer clips must be a list".into()),
    };
    Ok(Some(SceneAnimationPlayer {
        autoplay: optional_string_field(fields, "autoplay", "")?.to_owned(),
        speed: optional_f32_field(fields, "speed", 1.0)?,
        clips,
    }))
}

fn parse_animation_clip(value: &BsnValue) -> Result<SceneAnimationClip, String> {
    let BsnValue::Struct {
        type_path: Some(type_path),
        fields,
    } = value
    else {
        return Err("SceneAnimationPlayer clips must contain SceneAnimationClip values".into());
    };
    if final_type_name(type_path) != "SceneAnimationClip" {
        return Err(format!("unsupported animation clip type `{type_path}`"));
    }
    let tracks = match field_value(fields, "tracks")? {
        BsnValue::List(values) => values
            .iter()
            .map(parse_animation_track)
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("SceneAnimationClip tracks must be a list".into()),
    };
    Ok(SceneAnimationClip {
        name: string_field(fields, "name")?.to_owned(),
        length: optional_f32_field(fields, "length", 0.0)?,
        looped: optional_bool_field(fields, "looped", false)?,
        tracks,
    })
}

fn parse_animation_track(value: &BsnValue) -> Result<SceneAnimationTrack, String> {
    let BsnValue::Struct {
        type_path: Some(type_path),
        fields,
    } = value
    else {
        return Err("SceneAnimationClip tracks must contain SceneAnimationTrack values".into());
    };
    if final_type_name(type_path) != "SceneAnimationTrack" {
        return Err(format!("unsupported animation track type `{type_path}`"));
    }
    let kind = match field_value(fields, "kind")? {
        BsnValue::Path(path) => SceneAnimationTrackKind::from_label(path)?,
        _ => return Err("SceneAnimationTrack kind must be a variant".into()),
    };
    let keys = match field_value(fields, "keys")? {
        BsnValue::List(values) => values
            .iter()
            .map(parse_animation_key)
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("SceneAnimationTrack keys must be a list".into()),
    };
    Ok(SceneAnimationTrack {
        target_node: string_field(fields, "target_node")?.to_owned(),
        property: string_field(fields, "property")?.to_owned(),
        kind,
        keys,
    })
}

fn parse_animation_key(value: &BsnValue) -> Result<SceneAnimationKey, String> {
    let BsnValue::Struct {
        type_path: Some(type_path),
        fields,
    } = value
    else {
        return Err("SceneAnimationTrack keys must contain SceneAnimationKey values".into());
    };
    if final_type_name(type_path) != "SceneAnimationKey" {
        return Err(format!("unsupported animation key type `{type_path}`"));
    }
    Ok(SceneAnimationKey {
        time: optional_f32_field(fields, "time", 0.0)?,
        value: optional_string_field(fields, "value", "")?.to_owned(),
    })
}

fn optional_collision_rect_field(
    fields: &[BsnStructField],
    name: &str,
) -> Result<Option<SceneCollisionRect2D>, String> {
    let Some(value) = fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.value)
    else {
        return Ok(None);
    };
    let Some(fields) = option_struct_fields(value, "SceneCollisionRect2D")? else {
        return Ok(None);
    };
    Ok(Some(SceneCollisionRect2D {
        size: array2_tuple(tuple_f32_field::<2>(fields, "size")?),
        offset: array2_tuple(optional_tuple_f32_field::<2>(fields, "offset", [0.0, 0.0])?),
        enabled: optional_bool_field(fields, "enabled", true)?,
    }))
}

fn bool_field(fields: &[BsnStructField], name: &str) -> Result<bool, String> {
    match field_value(fields, name)? {
        BsnValue::Bool(value) => Ok(*value),
        _ => Err(format!("SceneNodeData field `{name}` must be a boolean")),
    }
}

fn optional_tuple_f32_field<const N: usize>(
    fields: &[BsnStructField],
    name: &str,
    default: [f32; N],
) -> Result<[f32; N], String> {
    if fields.iter().any(|field| field.name == name) {
        tuple_f32_field(fields, name)
    } else {
        Ok(default)
    }
}

fn optional_f32_field(fields: &[BsnStructField], name: &str, default: f32) -> Result<f32, String> {
    let Some(value) = fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.value)
    else {
        return Ok(default);
    };
    let BsnValue::Number(value) = value else {
        return Err(format!("SceneNodeData field `{name}` must be a number"));
    };
    value
        .replace('_', "")
        .trim_end_matches("f32")
        .parse()
        .map_err(|error| format!("invalid `{name}`: {error}"))
}

fn optional_bool_field(
    fields: &[BsnStructField],
    name: &str,
    default: bool,
) -> Result<bool, String> {
    if fields.iter().any(|field| field.name == name) {
        bool_field(fields, name)
    } else {
        Ok(default)
    }
}

fn optional_i32_field(fields: &[BsnStructField], name: &str, default: i32) -> Result<i32, String> {
    let Some(value) = fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.value)
    else {
        return Ok(default);
    };
    let BsnValue::Number(value) = value else {
        return Err(format!("SceneNodeData field `{name}` must be a number"));
    };
    value
        .replace('_', "")
        .trim_end_matches("i32")
        .parse()
        .map_err(|error| format!("invalid `{name}`: {error}"))
}

fn optional_u32_field(fields: &[BsnStructField], name: &str, default: u32) -> Result<u32, String> {
    if fields.iter().any(|field| field.name == name) {
        u32_field(fields, name)
    } else {
        Ok(default)
    }
}

fn option_struct_fields<'a>(
    value: &'a BsnValue,
    expected_type: &str,
) -> Result<Option<&'a [BsnStructField]>, String> {
    match value {
        BsnValue::Path(path) if final_type_name(path) == "None" => Ok(None),
        BsnValue::Constructor { type_path, fields }
            if final_type_name(type_path) == "Some" && fields.len() == 1 =>
        {
            match &fields[0] {
                BsnValue::Struct {
                    type_path: Some(type_path),
                    fields,
                } if final_type_name(type_path) == expected_type => Ok(Some(fields)),
                _ => Err(format!("Some(...) must contain `{expected_type}`")),
            }
        }
        _ => Err(format!("expected Some({expected_type} {{ ... }}) or None")),
    }
}

fn alignment_field(fields: &[BsnStructField], name: &str) -> Result<UiAlignment, String> {
    let BsnValue::Path(path) = field_value(fields, name)? else {
        return Err(format!("`{name}` must be a UiAlignment variant"));
    };
    match final_type_name(path) {
        "Start" => Ok(UiAlignment::Start),
        "Center" => Ok(UiAlignment::Center),
        "End" => Ok(UiAlignment::End),
        "Stretch" => Ok(UiAlignment::Stretch),
        value => Err(format!("unknown UiAlignment variant `{value}`")),
    }
}

fn final_type_name(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn array2_tuple(value: [f32; 2]) -> (f32, f32) {
    (value[0], value[1])
}

fn array3_tuple(value: [f32; 3]) -> (f32, f32, f32) {
    (value[0], value[1], value[2])
}

fn array4_tuple(value: [f32; 4]) -> (f32, f32, f32, f32) {
    (value[0], value[1], value[2], value[3])
}

fn validate_scene(scene: &SceneFile) -> Result<(), String> {
    if scene.format_version != SCENE_FORMAT_VERSION {
        return Err(format!(
            "unsupported scene format version: {}",
            scene.format_version
        ));
    }
    if scene.entities.is_empty() {
        return Err("scene contains no entities".into());
    }

    let ids: HashSet<_> = scene.entities.iter().map(|node| node.id.as_str()).collect();
    if ids.len() != scene.entities.len() {
        return Err("scene contains duplicate entity IDs".into());
    }
    let Some(root) = scene.entities.iter().find(|node| node.id == scene.root) else {
        return Err("root entity is missing".into());
    };
    if root.parent.is_some() {
        return Err("root entity cannot have a parent".into());
    }
    if !matches!(
        root.kind.as_str(),
        "2d" | "3d"
            | "empty2d"
            | "empty3d"
            | "sprite2d"
            | "camera2d"
            | "mesh3d"
            | "camera3d"
            | "directional_light3d"
            | "point_light3d"
            | "spot_light3d"
    ) {
        return Err(format!("unsupported scene kind: {}", root.kind));
    }

    let parents: HashMap<_, _> = scene
        .entities
        .iter()
        .map(|node| (node.id.as_str(), node.parent.as_deref()))
        .collect();
    for node in &scene.entities {
        if !matches!(
            node.kind.as_str(),
            "empty"
                | "animation_player"
                | "2d"
                | "3d"
                | "empty2d"
                | "collision_rect2d"
                | "empty3d"
                | "sprite2d"
                | "camera2d"
                | "mesh3d"
                | "camera3d"
                | "directional_light3d"
                | "point_light3d"
                | "spot_light3d"
                | "empty_ui"
                | "panel"
                | "text"
                | "button"
                | "image"
        ) {
            return Err(format!("unsupported scene kind: {}", node.kind));
        }
        for component in &node.components {
            if !matches!(
                component.as_str(),
                "transform"
                    | "sprite"
                    | "camera2d"
                    | "mesh3d"
                    | "camera3d"
                    | "directional_light3d"
                    | "point_light3d"
                    | "spot_light3d"
                    | "empty_ui"
                    | "panel"
                    | "text"
                    | "button"
                    | "image"
            ) {
                return Err(format!(
                    "entity {} has an unsupported component: {component}",
                    node.name
                ));
            }
        }
        for binding in &node.systems {
            if binding.script_path.is_empty() {
                continue;
            }
            if !is_valid_system_script_resource(&binding.script_path) {
                return Err(format!(
                    "entity {} has an invalid Rust system script path",
                    node.name
                ));
            }
        }
        if let Some(model) = node.model.as_ref() {
            if scene_model_resource_path(&model.resource_path).is_none() {
                return Err(format!(
                    "entity {} has an invalid 3D model resource path",
                    node.name
                ));
            }
            if node.kind != "mesh3d" && !node.components.iter().any(|value| value == "mesh3d") {
                return Err(format!(
                    "entity {} has a 3D model without a Mesh3D component",
                    node.name
                ));
            }
        }
        if let Some(collision) = node.collision_rect {
            if !collision.size.0.is_finite()
                || !collision.size.1.is_finite()
                || collision.size.0 <= 0.0
                || collision.size.1 <= 0.0
                || !collision.offset.0.is_finite()
                || !collision.offset.1.is_finite()
            {
                return Err(format!(
                    "entity {} has an invalid CollisionRect2D",
                    node.name
                ));
            }
            if node.kind != "collision_rect2d" {
                return Err(format!(
                    "entity {} has collision data without a CollisionRect2D preset",
                    node.name
                ));
            }
        }
        if node.id != scene.root && node.parent.is_none() {
            return Err(format!("entity {} has no parent", node.name));
        }
        if let Some(parent) = node.parent.as_deref() {
            if parent == node.id {
                return Err(format!("entity {} cannot parent itself", node.name));
            }
            if !ids.contains(parent) {
                return Err(format!("entity {} references a missing parent", node.name));
            }
        }

        let mut seen = HashSet::new();
        let mut current = Some(node.id.as_str());
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(format!("scene hierarchy contains a cycle at {}", node.name));
            }
            current = parents.get(id).copied().flatten();
        }
        if !seen.contains(scene.root.as_str()) {
            return Err(format!("entity {} is not connected to the root", node.name));
        }
    }
    Ok(())
}

fn is_valid_system_script_resource(value: &str) -> bool {
    let value = value.trim().replace('\\', "/");
    let Some(relative) = value.strip_prefix("res://src/") else {
        return false;
    };
    relative
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("rs"))
        && relative
            .split('/')
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
}

fn load_active_scene(
    mut commands: Commands,
    project: Res<ProjectRoot>,
    launch: Res<SceneLaunch>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // 先通过 ProjectRoot 验证场景边界，再交给 BSN Asset Loader。这里不能直接
    // 拼接任意命令行路径，否则游戏子进程可读取项目外文件。
    let relative = launch.relative_path.to_string_lossy().replace('\\', "/");
    let Some(path) = project
        .resolve_existing(&relative)
        .filter(|path| path.is_file())
    else {
        error!("Scene not found: res://{relative}");
        return;
    };
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bsn"))
    {
        let content = match fs::read(&path) {
            Ok(content) => content,
            Err(error) => {
                error!("Could not start scene res://{relative}: {error}");
                return;
            }
        };
        let source = match std::str::from_utf8(&content) {
            Ok(source) => source,
            Err(error) => {
                error!("Could not start scene res://{relative}: scene is not valid UTF-8: {error}");
                return;
            }
        };
        if let Err(error) = parse_scene_source(&path, source) {
            error!("Could not start scene res://{relative}: {error}");
            return;
        }
        let asset_path = relative
            .strip_prefix("assets/")
            .unwrap_or(relative.as_str())
            .to_owned();
        let handle: Handle<ScenePatch> = asset_server.load(asset_path);
        commands.spawn(ScenePatchInstance(handle));
        let content_hash = scene_content_hash(&content);
        commands.insert_resource(SceneHotReloadState {
            path,
            display_path: format!("res://{relative}"),
            applied_hash: content_hash,
            observed_hash: content_hash,
            last_check: 0.0,
            last_error: None,
        });
        commands.insert_resource(ActiveScene {
            relative_path: launch.relative_path.clone(),
            root_id: String::new(),
        });
        info!("Queued BSN scene res://{relative}");
        return;
    }
    let scene = match load_scene_file(&path) {
        Ok(scene) => scene,
        Err(error) => {
            error!("Could not start scene res://{relative}: {error}");
            return;
        }
    };

    let mut entities = HashMap::with_capacity(scene.entities.len());
    let mut spatial_nodes = HashSet::new();
    let mut scene_camera_present = false;
    for node in &scene.entities {
        let transform = Transform {
            translation: Vec3::new(node.translation.0, node.translation.1, node.translation.2),
            rotation: Quat::from_xyzw(
                node.rotation.0,
                node.rotation.1,
                node.rotation.2,
                node.rotation.3,
            ),
            scale: Vec3::new(node.scale.0, node.scale.1, node.scale.2),
        };
        let spatial = is_spatial_kind(&node.kind)
            || node.components.iter().any(|component| {
                matches!(
                    component.as_str(),
                    "transform"
                        | "sprite"
                        | "camera2d"
                        | "mesh3d"
                        | "camera3d"
                        | "directional_light3d"
                        | "point_light3d"
                        | "spot_light3d"
                )
            });
        if spatial {
            spatial_nodes.insert(node.id.clone());
        }
        let entity = commands
            .spawn((
                Name::new(node.name.clone()),
                RuntimeSceneNode {
                    id: node.id.clone(),
                    parent_id: node.parent.clone(),
                    order: node.order,
                    kind: node.kind.clone(),
                },
                RuntimeCustomComponents(node.custom_components.clone()),
                RuntimeEntityScript(node.entity_script.clone()),
                RuntimeSystemBindings(node.systems.clone()),
            ))
            .id();
        if let Some(animation_player) = node.animation_player.clone() {
            commands.entity(entity).insert(animation_player);
        }
        if let Some(collision) = node.collision_rect {
            commands.entity(entity).insert(collision);
        }
        if is_ui_kind(&node.kind) {
            let layout = node
                .ui_layout
                .unwrap_or_else(|| default_ui_layout(&node.kind));
            let content = node
                .ui_content
                .clone()
                .unwrap_or_else(|| default_ui_content(&node.kind));
            commands.entity(entity).insert((
                layout,
                runtime_ui_node(layout),
                scene_ui_transform(layout),
            ));
            match node.kind.as_str() {
                "panel" => {
                    commands.entity(entity).insert(BackgroundColor(Color::srgba(
                        content.panel_color.0,
                        content.panel_color.1,
                        content.panel_color.2,
                        content.panel_color.3,
                    )));
                }
                "text" => {
                    commands.entity(entity).insert((
                        Text::new(content.text),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                }
                "button" => {
                    commands.entity(entity).insert((
                        Button,
                        BackgroundColor(Color::srgba(
                            content.panel_color.0,
                            content.panel_color.1,
                            content.panel_color.2,
                            content.panel_color.3,
                        )),
                    ));
                    commands.entity(entity).with_child((
                        RuntimeGeneratedUiContent,
                        Text::new(content.text),
                        TextFont {
                            font_size: FontSize::Px(16.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                }
                "image" => {
                    let image = scene_image_asset_path(&content.image_path)
                        .map_or_else(ImageNode::default, |path| {
                            ImageNode::new(asset_server.load(path))
                        });
                    commands.entity(entity).insert((
                        image,
                        BackgroundColor(Color::srgba(
                            content.panel_color.0,
                            content.panel_color.1,
                            content.panel_color.2,
                            content.panel_color.3,
                        )),
                    ));
                }
                _ => {}
            }
        } else if spatial {
            commands
                .entity(entity)
                .insert((transform, Visibility::default()));
        }
        let has_model = node
            .model
            .as_ref()
            .and_then(|model| runtime_model_components(model, &asset_server))
            .is_some_and(|components| {
                commands.entity(entity).insert(components);
                true
            });
        match node.kind.as_str() {
            "sprite2d" => {
                commands.entity(entity).insert(runtime_sprite_components(
                    node.sprite.as_ref(),
                    &asset_server,
                ));
            }
            "camera2d" => {
                commands.entity(entity).insert(Camera2d);
                scene_camera_present = true;
            }
            "mesh3d" if !has_model => {
                commands.entity(entity).insert((
                    Mesh3d(meshes.add(Cuboid::default())),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(0.28, 0.58, 0.88),
                        ..default()
                    })),
                ));
            }
            "camera3d" => {
                commands.entity(entity).insert(Camera3d::default());
                scene_camera_present = true;
            }
            "directional_light3d" => {
                commands.entity(entity).insert(DirectionalLight::default());
            }
            "point_light3d" => {
                commands.entity(entity).insert(PointLight::default());
            }
            "spot_light3d" => {
                commands.entity(entity).insert(SpotLight::default());
            }
            _ => {}
        }
        for component in &node.components {
            match component.as_str() {
                "transform" => {}
                "sprite" => {
                    commands.entity(entity).insert(runtime_sprite_components(
                        node.sprite.as_ref(),
                        &asset_server,
                    ));
                }
                "camera2d" => {
                    commands.entity(entity).insert(Camera2d);
                    scene_camera_present = true;
                }
                "mesh3d" if !has_model => {
                    commands.entity(entity).insert((
                        Mesh3d(meshes.add(Cuboid::default())),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: Color::srgb(0.28, 0.58, 0.88),
                            ..default()
                        })),
                    ));
                }
                "camera3d" => {
                    commands.entity(entity).insert(Camera3d::default());
                    scene_camera_present = true;
                }
                "directional_light3d" => {
                    commands.entity(entity).insert(DirectionalLight::default());
                }
                "point_light3d" => {
                    commands.entity(entity).insert(PointLight::default());
                }
                "spot_light3d" => {
                    commands.entity(entity).insert(SpotLight::default());
                }
                _ => {}
            }
        }
        entities.insert(node.id.clone(), entity);
    }
    for node in &scene.entities {
        if let Some(parent_id) = node.parent.as_ref()
            && let (Some(&entity), Some(&parent)) =
                (entities.get(&node.id), entities.get(parent_id))
            && ((spatial_nodes.contains(parent_id) && spatial_nodes.contains(&node.id))
                || (is_ui_kind(&node.kind)
                    && scene
                        .entities
                        .iter()
                        .find(|candidate| candidate.id == *parent_id)
                        .is_some_and(|candidate| is_ui_kind(&candidate.kind))))
        {
            commands.entity(entity).insert(ChildOf(parent));
        }
    }

    let root_kind = scene
        .entities
        .iter()
        .find(|node| node.id == scene.root)
        .map(|node| node.kind.as_str());
    if !scene_camera_present {
        match root_kind {
            Some("3d")
            | Some("empty3d")
            | Some("mesh3d")
            | Some("camera3d")
            | Some("directional_light3d")
            | Some("point_light3d")
            | Some("spot_light3d") => {
                commands.spawn((
                    RuntimeGeneratedCamera,
                    Camera3d::default(),
                    Transform::from_xyz(0.0, 4.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
                ));
            }
            _ => {
                commands.spawn((RuntimeGeneratedCamera, Camera2d));
            }
        }
    }

    info!(
        "Started scene res://{} ({} entities)",
        relative,
        scene.entities.len()
    );
    commands.insert_resource(ActiveScene {
        relative_path: launch.relative_path.clone(),
        root_id: scene.root,
    });
}

/// BSN Loader 生成反射组件和层级后，把 Revy 预设补全为真实运行时组件。
///
/// 这里是“磁盘数据 -> 可渲染/可运行 ECS”的关键边界。新增实体类型时必须同步
/// 增加物化逻辑，否则编辑器中可见的实体会在游戏里变成空节点。
fn materialize_bsn_scene_nodes(
    mut commands: Commands,
    nodes: Query<
        (Entity, &SceneNodeData, Has<Transform>),
        (Added<SceneNodeData>, Without<RuntimeSceneNode>),
    >,
    all_nodes: Query<&SceneNodeData>,
    existing_cameras: Query<(), Or<(With<Camera2d>, With<Camera3d>)>>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut active_scene: Option<ResMut<ActiveScene>>,
    mut camera_initialized: Local<bool>,
) {
    for (entity, node, has_transform) in &nodes {
        commands.entity(entity).insert(RuntimeSceneNode {
            id: node.id.clone(),
            parent_id: node.parent.clone(),
            order: node.order,
            kind: node.kind.clone(),
        });
        commands.entity(entity).insert((
            RuntimeCustomComponents(node.custom_components.clone()),
            RuntimeEntityScript(node.entity_script.clone()),
            RuntimeSystemBindings(node.systems.clone()),
        ));
        if let Some(animation_player) = node.animation_player.clone() {
            // 播放器本身是场景数据组件；后续播放系统只读取稳定节点 ID。
            commands.entity(entity).insert(animation_player);
        }
        if let Some(collision) = node.collision_rect {
            commands.entity(entity).insert(collision);
        }

        let transform = Transform {
            translation: Vec3::new(node.translation.0, node.translation.1, node.translation.2),
            rotation: Quat::from_xyzw(
                node.rotation.0,
                node.rotation.1,
                node.rotation.2,
                node.rotation.3,
            ),
            scale: Vec3::new(node.scale.0, node.scale.1, node.scale.2),
        };
        let spatial = is_spatial_kind(&node.kind)
            || node.components.iter().any(|component| {
                matches!(
                    component.as_str(),
                    "transform"
                        | "sprite"
                        | "camera2d"
                        | "mesh3d"
                        | "camera3d"
                        | "directional_light3d"
                        | "point_light3d"
                        | "spot_light3d"
                )
            });

        if is_ui_kind(&node.kind) {
            let layout = node
                .ui_layout
                .unwrap_or_else(|| default_ui_layout(&node.kind));
            let content = node
                .ui_content
                .clone()
                .unwrap_or_else(|| default_ui_content(&node.kind));
            commands.entity(entity).insert((
                layout,
                runtime_ui_node(layout),
                scene_ui_transform(layout),
            ));
            match node.kind.as_str() {
                "panel" => {
                    commands.entity(entity).insert(BackgroundColor(Color::srgba(
                        content.panel_color.0,
                        content.panel_color.1,
                        content.panel_color.2,
                        content.panel_color.3,
                    )));
                }
                "text" => {
                    commands.entity(entity).insert((
                        Text::new(content.text),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                }
                "button" => {
                    commands.entity(entity).insert((
                        Button,
                        BackgroundColor(Color::srgba(
                            content.panel_color.0,
                            content.panel_color.1,
                            content.panel_color.2,
                            content.panel_color.3,
                        )),
                    ));
                    commands.entity(entity).with_child((
                        RuntimeGeneratedUiContent,
                        Text::new(content.text),
                        TextFont {
                            font_size: FontSize::Px(16.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                }
                "image" => {
                    let image = scene_image_asset_path(&content.image_path)
                        .map_or_else(ImageNode::default, |path| {
                            ImageNode::new(asset_server.load(path))
                        });
                    commands.entity(entity).insert((
                        image,
                        BackgroundColor(Color::srgba(
                            content.panel_color.0,
                            content.panel_color.1,
                            content.panel_color.2,
                            content.panel_color.3,
                        )),
                    ));
                }
                _ => {}
            }
        } else if spatial {
            if !has_transform {
                commands.entity(entity).insert(transform);
            }
            commands.entity(entity).insert(Visibility::default());
        }

        let has_model = node
            .model
            .as_ref()
            .and_then(|model| runtime_model_components(model, &asset_server))
            .is_some_and(|components| {
                commands.entity(entity).insert(components);
                true
            });

        let mut apply_component = |component: &str| match component {
            "sprite" | "sprite2d" => {
                commands.entity(entity).insert(runtime_sprite_components(
                    node.sprite.as_ref(),
                    &asset_server,
                ));
            }
            "camera2d" => {
                commands.entity(entity).insert(Camera2d);
            }
            "mesh3d" if !has_model => {
                commands.entity(entity).insert((
                    Mesh3d(meshes.add(Cuboid::default())),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(0.28, 0.58, 0.88),
                        ..default()
                    })),
                ));
            }
            "camera3d" => {
                commands.entity(entity).insert(Camera3d::default());
            }
            "directional_light3d" => {
                commands.entity(entity).insert(DirectionalLight::default());
            }
            "point_light3d" => {
                commands.entity(entity).insert(PointLight::default());
            }
            "spot_light3d" => {
                commands.entity(entity).insert(SpotLight::default());
            }
            _ => {}
        };
        apply_component(node.kind.as_str());
        for component in &node.components {
            apply_component(component);
        }

        if node.parent.is_none()
            && let Some(active_scene) = active_scene.as_deref_mut()
        {
            active_scene.root_id = node.id.clone();
        }
    }

    if *camera_initialized {
        return;
    }
    let Some(root) = all_nodes.iter().find(|node| node.parent.is_none()) else {
        return;
    };
    let authored_camera = !existing_cameras.is_empty()
        || all_nodes.iter().any(|node| {
            matches!(node.kind.as_str(), "camera2d" | "camera3d")
                || node
                    .components
                    .iter()
                    .any(|component| matches!(component.as_str(), "camera2d" | "camera3d"))
        });
    if !authored_camera {
        match root.kind.as_str() {
            "3d"
            | "empty3d"
            | "mesh3d"
            | "camera3d"
            | "directional_light3d"
            | "point_light3d"
            | "spot_light3d" => {
                commands.spawn((
                    RuntimeGeneratedCamera,
                    Camera3d::default(),
                    Transform::from_xyz(0.0, 4.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
                ));
            }
            _ => {
                commands.spawn((RuntimeGeneratedCamera, Camera2d));
            }
        }
    }
    *camera_initialized = true;
    info!(
        "Materialized BSN scene ({} entities)",
        all_nodes.iter().count()
    );
}

/// Inserts project component types that the game has explicitly registered for
/// reflection. The metadata component remains attached as the durable fallback
/// for unregistered types and for tooling that consumes scene-authored values.
fn materialize_reflected_custom_components(world: &mut World) {
    let pending: Vec<_> = {
        let mut query = world
            .query_filtered::<(Entity, &RuntimeCustomComponents), Changed<RuntimeCustomComponents>>(
            );
        query
            .iter(world)
            .map(|(entity, components)| (entity, components.0.clone()))
            .collect()
    };
    if pending.is_empty() {
        return;
    }
    let Some(registry) = world.get_resource::<AppTypeRegistry>().cloned() else {
        return;
    };
    let registry = registry.read();
    for (entity, components) in pending {
        for component in components {
            let Some(registration) = registry.get_with_type_path(&component.type_path) else {
                continue;
            };
            let (Some(reflect_component), Some(reflect_default)) = (
                registration.data::<ReflectComponent>(),
                registration.data::<ReflectDefault>(),
            ) else {
                continue;
            };
            let mut value = reflect_default.default();
            if !apply_custom_component_fields(value.as_partial_reflect_mut(), &component.fields) {
                warn!(
                    "Could not materialize custom component {}: one or more field values are invalid",
                    component.type_path
                );
                continue;
            }
            let Ok(mut entity) = world.get_entity_mut(entity) else {
                continue;
            };
            reflect_component.insert(&mut entity, value.as_partial_reflect(), &registry);
        }
    }
}

fn apply_custom_component_fields(
    value: &mut dyn PartialReflect,
    fields: &[SceneCustomField],
) -> bool {
    match value.reflect_mut() {
        ReflectMut::Struct(value) => fields.iter().all(|field| {
            value
                .field_mut(&field.name)
                .is_some_and(|target| apply_custom_field_value(target, field))
        }),
        ReflectMut::TupleStruct(value) => fields.iter().all(|field| {
            field
                .name
                .parse::<usize>()
                .ok()
                .and_then(|index| value.field_mut(index))
                .is_some_and(|target| apply_custom_field_value(target, field))
        }),
        _ => fields.is_empty(),
    }
}

fn apply_custom_field_value(target: &mut dyn PartialReflect, field: &SceneCustomField) -> bool {
    let value = field.value.trim();
    let short_type = field
        .type_name
        .rsplit("::")
        .next()
        .unwrap_or(&field.type_name);
    macro_rules! apply_parsed {
        ($type:ty) => {{
            value
                .parse::<$type>()
                .ok()
                .is_some_and(|parsed| target.try_apply(&parsed).is_ok())
        }};
    }
    match short_type {
        "bool" => apply_parsed!(bool),
        "i8" => apply_parsed!(i8),
        "i16" => apply_parsed!(i16),
        "i32" => apply_parsed!(i32),
        "i64" => apply_parsed!(i64),
        "i128" => apply_parsed!(i128),
        "isize" => apply_parsed!(isize),
        "u8" => apply_parsed!(u8),
        "u16" => apply_parsed!(u16),
        "u32" => apply_parsed!(u32),
        "u64" => apply_parsed!(u64),
        "u128" => apply_parsed!(u128),
        "usize" => apply_parsed!(usize),
        "f32" => apply_parsed!(f32),
        "f64" => apply_parsed!(f64),
        "String" => target.try_apply(&field.value).is_ok(),
        "Vec2" => parse_custom_scalars::<2>(value)
            .is_some_and(|values| target.try_apply(&Vec2::new(values[0], values[1])).is_ok()),
        "Vec3" => parse_custom_scalars::<3>(value).is_some_and(|values| {
            target
                .try_apply(&Vec3::new(values[0], values[1], values[2]))
                .is_ok()
        }),
        "Vec4" => parse_custom_scalars::<4>(value).is_some_and(|values| {
            target
                .try_apply(&Vec4::new(values[0], values[1], values[2], values[3]))
                .is_ok()
        }),
        "Color" => parse_custom_scalars::<4>(value).is_some_and(|values| {
            target
                .try_apply(&Color::srgba(values[0], values[1], values[2], values[3]))
                .is_ok()
        }),
        // Unsupported fields stay at their reflected default and remain locked
        // in the editor until a field editor is added for their type.
        _ => true,
    }
}

fn parse_custom_scalars<const N: usize>(value: &str) -> Option<[f32; N]> {
    let values = value
        .split(',')
        .map(str::trim)
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    values.try_into().ok()
}

fn hot_reload_bsn_scene(
    mut commands: Commands,
    real_time: Res<Time<Real>>,
    mut state: Option<ResMut<SceneHotReloadState>>,
    mut active_scene: Option<ResMut<ActiveScene>>,
    current_nodes: Query<(Entity, &RuntimeSceneNode)>,
    generated_ui: Query<(Entity, &ChildOf), With<RuntimeGeneratedUiContent>>,
    generated_cameras: Query<Entity, With<RuntimeGeneratedCamera>>,
    existing_models: Query<(), With<WorldAssetRoot>>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(state) = state.as_deref_mut() else {
        return;
    };
    if current_nodes.is_empty() {
        return;
    }
    let now = real_time.elapsed_secs_f64();
    if now - state.last_check < 0.1 {
        return;
    }
    state.last_check = now;

    let content = match fs::read(&state.path) {
        Ok(content) => content,
        Err(error) => {
            report_hot_reload_error(state, format!("could not read scene: {error}"));
            return;
        }
    };
    let hash = scene_content_hash(&content);
    if hash == state.observed_hash {
        return;
    }
    state.observed_hash = hash;

    let source = match std::str::from_utf8(&content) {
        Ok(source) => source,
        Err(error) => {
            report_hot_reload_error(state, format!("scene is not valid UTF-8: {error}"));
            return;
        }
    };
    let scene = match parse_scene_source(&state.path, source) {
        Ok(scene) => scene,
        Err(error) => {
            report_hot_reload_error(state, error);
            return;
        }
    };
    if hash == state.applied_hash {
        state.last_error = None;
        return;
    }

    let existing: HashMap<_, _> = current_nodes
        .iter()
        .map(|(entity, node)| (node.id.clone(), entity))
        .collect();
    let next_ids: HashSet<_> = scene.entities.iter().map(|node| node.id.as_str()).collect();
    let generated_ui_by_parent: HashMap<_, Vec<_>> = generated_ui.iter().fold(
        HashMap::new(),
        |mut by_parent: HashMap<Entity, Vec<Entity>>, (entity, parent)| {
            by_parent.entry(parent.parent()).or_default().push(entity);
            by_parent
        },
    );
    let mut entities = HashMap::with_capacity(scene.entities.len());

    for node in &scene.entities {
        let entity = existing
            .get(&node.id)
            .copied()
            .unwrap_or_else(|| commands.spawn_empty().id());
        if let Some(children) = generated_ui_by_parent.get(&entity) {
            for &child in children {
                commands.entity(child).try_despawn();
            }
        }
        apply_runtime_scene_node(
            &mut commands,
            entity,
            node,
            existing_models.contains(entity),
            &asset_server,
            &mut meshes,
            &mut materials,
        );
        entities.insert(node.id.clone(), entity);
    }

    for node in &scene.entities {
        let entity = entities[&node.id];
        let parent = node.parent.as_ref().and_then(|parent_id| {
            let parent_node = scene
                .entities
                .iter()
                .find(|candidate| candidate.id == *parent_id)?;
            ((scene_node_is_spatial(parent_node) && scene_node_is_spatial(node))
                || (is_ui_kind(&parent_node.kind) && is_ui_kind(&node.kind)))
            .then(|| entities[parent_id])
        });
        if let Some(parent) = parent {
            commands.entity(entity).insert(ChildOf(parent));
        } else {
            commands.entity(entity).remove::<ChildOf>();
        }
    }

    for (id, entity) in existing {
        if !next_ids.contains(id.as_str()) {
            commands.entity(entity).try_despawn();
        }
    }

    for entity in &generated_cameras {
        commands.entity(entity).try_despawn();
    }
    if !scene_has_authored_camera(&scene) {
        spawn_generated_camera(&mut commands, scene_root_kind(&scene));
    }

    if let Some(active_scene) = active_scene.as_deref_mut() {
        active_scene.root_id = scene.root.clone();
    }
    state.applied_hash = hash;
    state.last_error = None;
    info!(
        "Hot reloaded {} ({} entities)",
        state.display_path,
        scene.entities.len()
    );
}

fn report_hot_reload_error(state: &mut SceneHotReloadState, error: String) {
    if state.last_error.as_deref() == Some(error.as_str()) {
        return;
    }
    error!(
        "Scene hot reload failed for {}: {error}; keeping the previous scene",
        state.display_path
    );
    state.last_error = Some(error);
}

fn scene_node_is_spatial(node: &SceneNodeData) -> bool {
    is_spatial_kind(&node.kind)
        || node.components.iter().any(|component| {
            matches!(
                component.as_str(),
                "transform"
                    | "sprite"
                    | "camera2d"
                    | "mesh3d"
                    | "camera3d"
                    | "directional_light3d"
                    | "point_light3d"
                    | "spot_light3d"
            )
        })
}

fn scene_has_authored_camera(scene: &SceneFile) -> bool {
    scene.entities.iter().any(|node| {
        matches!(node.kind.as_str(), "camera2d" | "camera3d")
            || node
                .components
                .iter()
                .any(|component| matches!(component.as_str(), "camera2d" | "camera3d"))
    })
}

fn scene_root_kind(scene: &SceneFile) -> &str {
    scene
        .entities
        .iter()
        .find(|node| node.id == scene.root)
        .map_or("2d", |node| node.kind.as_str())
}

fn spawn_generated_camera(commands: &mut Commands, root_kind: &str) {
    if matches!(
        root_kind,
        "3d" | "empty3d"
            | "mesh3d"
            | "camera3d"
            | "directional_light3d"
            | "point_light3d"
            | "spot_light3d"
    ) {
        commands.spawn((
            RuntimeGeneratedCamera,
            Camera3d::default(),
            Transform::from_xyz(0.0, 4.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
    } else {
        commands.spawn((RuntimeGeneratedCamera, Camera2d));
    }
}

fn apply_runtime_scene_node(
    commands: &mut Commands,
    entity: Entity,
    node: &SceneNodeData,
    had_world_asset: bool,
    asset_server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let has_mesh = node.kind == "mesh3d"
        || node
            .components
            .iter()
            .any(|component| component == "mesh3d");
    let has_world_asset = has_mesh
        && node
            .model
            .as_ref()
            .is_some_and(|model| scene_model_asset_path(&model.resource_path).is_some());
    if had_world_asset && !has_world_asset {
        clear_runtime_world_asset(commands, entity);
    }

    let mut entity_commands = commands.entity(entity);
    entity_commands
        .remove::<(
            Transform,
            Visibility,
            Sprite,
            Anchor,
            SceneSprite2D,
            SceneCollisionRect2D,
            SceneAnimationPlayer,
            RuntimeAnimationPlayback,
            Camera2d,
            Camera3d,
            DirectionalLight,
            PointLight,
            SpotLight,
        )>()
        .remove::<(
            Mesh3d,
            MeshMaterial3d<StandardMaterial>,
            SceneModel3D,
            Node,
            UiTransform,
            SceneUiLayout,
            BackgroundColor,
            Text,
            TextFont,
            TextColor,
            Button,
            ImageNode,
        )>()
        .insert((
            Name::new(node.name.clone()),
            node.clone(),
            RuntimeSceneNode {
                id: node.id.clone(),
                parent_id: node.parent.clone(),
                order: node.order,
                kind: node.kind.clone(),
            },
            RuntimeCustomComponents(node.custom_components.clone()),
            RuntimeEntityScript(node.entity_script.clone()),
            RuntimeSystemBindings(node.systems.clone()),
        ));
    if let Some(animation_player) = node.animation_player.clone() {
        commands.entity(entity).insert(animation_player);
    }
    if let Some(collision) = node.collision_rect {
        commands.entity(entity).insert(collision);
    }

    if is_ui_kind(&node.kind) {
        let layout = node
            .ui_layout
            .unwrap_or_else(|| default_ui_layout(&node.kind));
        let content = node
            .ui_content
            .clone()
            .unwrap_or_else(|| default_ui_content(&node.kind));
        commands.entity(entity).insert((
            layout,
            runtime_ui_node(layout),
            scene_ui_transform(layout),
        ));
        match node.kind.as_str() {
            "panel" => {
                commands.entity(entity).insert(BackgroundColor(Color::srgba(
                    content.panel_color.0,
                    content.panel_color.1,
                    content.panel_color.2,
                    content.panel_color.3,
                )));
            }
            "text" => {
                commands.entity(entity).insert((
                    Text::new(content.text),
                    TextFont {
                        font_size: FontSize::Px(18.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
            }
            "button" => {
                commands.entity(entity).insert((
                    Button,
                    BackgroundColor(Color::srgba(
                        content.panel_color.0,
                        content.panel_color.1,
                        content.panel_color.2,
                        content.panel_color.3,
                    )),
                ));
                commands.entity(entity).with_child((
                    RuntimeGeneratedUiContent,
                    Text::new(content.text),
                    TextFont {
                        font_size: FontSize::Px(16.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
            }
            "image" => {
                let image = scene_image_asset_path(&content.image_path)
                    .map_or_else(ImageNode::default, |path| {
                        ImageNode::new(asset_server.load(path))
                    });
                commands.entity(entity).insert((
                    image,
                    BackgroundColor(Color::srgba(
                        content.panel_color.0,
                        content.panel_color.1,
                        content.panel_color.2,
                        content.panel_color.3,
                    )),
                ));
            }
            _ => {}
        }
    } else if scene_node_is_spatial(node) {
        commands.entity(entity).insert((
            Transform {
                translation: Vec3::new(node.translation.0, node.translation.1, node.translation.2),
                rotation: Quat::from_xyzw(
                    node.rotation.0,
                    node.rotation.1,
                    node.rotation.2,
                    node.rotation.3,
                ),
                scale: Vec3::new(node.scale.0, node.scale.1, node.scale.2),
            },
            Visibility::default(),
        ));
    }

    let has_sprite = node.kind == "sprite2d"
        || node
            .components
            .iter()
            .any(|component| component == "sprite");
    let has_model = has_mesh
        && node
            .model
            .as_ref()
            .and_then(|model| runtime_model_components(model, asset_server))
            .is_some_and(|components| {
                commands.entity(entity).insert(components);
                true
            });
    if has_sprite {
        commands.entity(entity).insert(runtime_sprite_components(
            node.sprite.as_ref(),
            asset_server,
        ));
    }
    if has_mesh && !has_model {
        commands.entity(entity).insert((
            Mesh3d(meshes.add(Cuboid::default())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.28, 0.58, 0.88),
                ..default()
            })),
        ));
    }
    if node.kind == "camera2d" || node.components.iter().any(|value| value == "camera2d") {
        commands.entity(entity).insert(Camera2d);
    }
    if node.kind == "camera3d" || node.components.iter().any(|value| value == "camera3d") {
        commands.entity(entity).insert(Camera3d::default());
    }
    if node.kind == "directional_light3d"
        || node
            .components
            .iter()
            .any(|value| value == "directional_light3d")
    {
        commands.entity(entity).insert(DirectionalLight::default());
    }
    if node.kind == "point_light3d" || node.components.iter().any(|value| value == "point_light3d")
    {
        commands.entity(entity).insert(PointLight::default());
    }
    if node.kind == "spot_light3d" || node.components.iter().any(|value| value == "spot_light3d") {
        commands.entity(entity).insert(SpotLight::default());
    }
}

fn clear_runtime_world_asset(commands: &mut Commands, entity: Entity) {
    commands.queue(move |world: &mut World| {
        let instance_id = world
            .get::<WorldInstance>(entity)
            .map(|instance| **instance);
        if let Some(instance_id) = instance_id {
            world.resource_scope(|world, mut spawner: Mut<WorldInstanceSpawner>| {
                spawner.despawn_instance_sync(world, &instance_id);
            });
        }
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.remove::<WorldAssetRoot>();
            entity_mut.remove::<WorldInstance>();
        }
    });
}

fn is_spatial_kind(kind: &str) -> bool {
    !matches!(
        kind,
        "empty" | "empty_ui" | "panel" | "text" | "button" | "image"
    )
}

fn runtime_sprite_components(
    data: Option<&SceneSprite2D>,
    asset_server: &AssetServer,
) -> (SceneSprite2D, Sprite, Anchor, Visibility) {
    let data = data.cloned().unwrap_or_default();
    let image_path = scene_image_asset_path(&data.image_path);
    let image = image_path
        .as_ref()
        .map(|path| asset_server.load(path.clone()))
        .unwrap_or_default();
    let sprite = Sprite {
        image,
        color: Color::srgba(data.color.0, data.color.1, data.color.2, data.color.3),
        flip_x: data.flip_x,
        flip_y: data.flip_y,
        custom_size: image_path.is_none().then_some(
            scene_sprite_rect(&data)
                .map(|rect| rect.size())
                .unwrap_or(Vec2::splat(64.0)),
        ),
        rect: scene_sprite_rect(&data),
        ..default()
    };
    let anchor = Anchor(Vec2::new(
        data.anchor.0.clamp(0.0, 1.0) - 0.5,
        0.5 - data.anchor.1.clamp(0.0, 1.0),
    ));
    let visibility = if data.visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    (data, sprite, anchor, visibility)
}

fn runtime_model_components(
    data: &SceneModel3D,
    asset_server: &AssetServer,
) -> Option<(SceneModel3D, WorldAssetRoot)> {
    let asset_path = scene_model_asset_path(&data.resource_path)?;
    Some((
        data.clone(),
        WorldAssetRoot(load_model_scene(asset_server, asset_path)),
    ))
}

fn load_model_scene(asset_server: &AssetServer, asset_path: String) -> Handle<WorldAsset> {
    if asset_path
        .split('#')
        .next()
        .is_some_and(|path| path.ends_with(".fbx"))
    {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut FbxLoaderSettings| {
                settings.load_auxiliary_geometry = false;
            })
            .load::<WorldAsset>(asset_path)
    } else {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut bevy::gltf::GltfLoaderSettings| {
                settings.load_cameras = false;
                settings.load_lights = false;
                settings.load_main_model_only = true;
            })
            .load(asset_path)
    }
}

fn sync_runtime_sprite_render(
    asset_server: Res<AssetServer>,
    images: Res<Assets<Image>>,
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
        let next_image = image_path
            .as_ref()
            .map(|path| asset_server.load(path.clone()))
            .unwrap_or_default();
        if sprite.image != next_image {
            sprite.image = next_image;
        }
        let texture_size = images.get(&sprite.image).map(|image| {
            Vec2::new(
                image.texture_descriptor.size.width as f32,
                image.texture_descriptor.size.height as f32,
            )
        });
        let rect = texture_size
            .and_then(|size| scene_sprite_frame_rect(data, size))
            .or_else(|| scene_sprite_rect(data));
        if sprite.rect != rect {
            sprite.rect = rect;
        }
        let custom_size = image_path
            .is_none()
            .then_some(rect.map(|rect| rect.size()).unwrap_or(Vec2::splat(64.0)));
        if sprite.custom_size != custom_size {
            sprite.custom_size = custom_size;
        }
        sprite.color = Color::srgba(data.color.0, data.color.1, data.color.2, data.color.3);
        sprite.flip_x = data.flip_x;
        sprite.flip_y = data.flip_y;
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

fn is_ui_kind(kind: &str) -> bool {
    matches!(kind, "empty_ui" | "panel" | "text" | "button" | "image")
}

fn default_ui_layout(kind: &str) -> SceneUiLayout {
    match kind {
        "empty_ui" => SceneUiLayout::sized(320.0, 180.0),
        "panel" => SceneUiLayout::sized(240.0, 160.0),
        "text" => SceneUiLayout::sized(160.0, 32.0),
        "button" => SceneUiLayout::sized(120.0, 36.0),
        "image" => SceneUiLayout::sized(128.0, 128.0),
        _ => SceneUiLayout::default(),
    }
}

fn default_ui_content(kind: &str) -> SceneUiContent {
    let mut content = SceneUiContent::default();
    match kind {
        "text" => content.text = "Text".into(),
        "button" => {
            content.text = "Button".into();
            content.panel_color = (0.22, 0.42, 0.72, 1.0);
        }
        "image" => content.panel_color = (0.32, 0.34, 0.40, 1.0),
        _ => {}
    }
    content
}

/// Converts persisted UI transform values into Bevy's render-time UI transform.
pub fn scene_ui_transform(layout: SceneUiLayout) -> UiTransform {
    let size = Vec2::new(
        layout.size.0.max(layout.minimum_size.0),
        layout.size.1.max(layout.minimum_size.1),
    );
    let pivot = Vec2::new(layout.pivot_offset.0, layout.pivot_offset.1)
        + Vec2::new(layout.pivot_ratio.0, layout.pivot_ratio.1) * size;
    let centered_pivot = pivot - size * 0.5;
    let scale = Vec2::new(layout.scale.0, layout.scale.1);
    let rotation = Rot2::degrees(layout.rotation);
    let linear = Mat2::from(rotation) * Mat2::from_diagonal(scale);
    let correction = centered_pivot - linear * centered_pivot;
    UiTransform {
        translation: Val2::px(correction.x, correction.y),
        scale,
        rotation,
    }
}

pub(crate) fn runtime_ui_node(layout: SceneUiLayout) -> Node {
    let stretch_x = layout.anchor_max.0 > layout.anchor_min.0;
    let stretch_y = layout.anchor_max.1 > layout.anchor_min.1;
    let horizontal_alignment = match layout.horizontal_alignment {
        UiAlignment::Start => AlignItems::FlexStart,
        UiAlignment::Center => AlignItems::Center,
        UiAlignment::End => AlignItems::FlexEnd,
        UiAlignment::Stretch => AlignItems::Stretch,
    };
    let vertical_alignment = match layout.vertical_alignment {
        UiAlignment::Start => JustifyContent::FlexStart,
        UiAlignment::Center => JustifyContent::Center,
        UiAlignment::End => JustifyContent::FlexEnd,
        UiAlignment::Stretch => JustifyContent::SpaceBetween,
    };
    Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(layout.anchor_min.0 * 100.0),
        top: Val::Percent(layout.anchor_min.1 * 100.0),
        right: if stretch_x {
            Val::Percent((1.0 - layout.anchor_max.0) * 100.0)
        } else {
            Val::Auto
        },
        bottom: if stretch_y {
            Val::Percent((1.0 - layout.anchor_max.1) * 100.0)
        } else {
            Val::Auto
        },
        width: if stretch_x {
            Val::Auto
        } else {
            Val::Px(layout.size.0)
        },
        height: if stretch_y {
            Val::Auto
        } else {
            Val::Px(layout.size.1)
        },
        min_width: Val::Px(layout.minimum_size.0.max(0.0)),
        min_height: Val::Px(layout.minimum_size.1.max(0.0)),
        margin: UiRect::new(
            Val::Px(layout.margin.0 + layout.offset.0),
            Val::Px(layout.margin.2),
            Val::Px(layout.margin.1 + layout.offset.1),
            Val::Px(layout.margin.3),
        ),
        align_items: horizontal_alignment,
        justify_content: vertical_alignment,
        overflow: if layout.clip_contents {
            Overflow::clip()
        } else {
            Overflow::visible()
        },
        ..default()
    }
}

fn sync_run_control(
    launch: Res<SceneLaunch>,
    real_time: Res<Time<Real>>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut paused: ResMut<GamePaused>,
    mut last_check: Local<Option<(f64, Option<SystemTime>)>>,
) {
    let Some(path) = launch.control_path.as_deref() else {
        return;
    };
    let now = real_time.elapsed_secs_f64();
    if last_check
        .as_ref()
        .is_some_and(|(previous, _)| now - previous < 0.1)
    {
        return;
    }
    let modified = fs::metadata(path).and_then(|value| value.modified()).ok();
    if last_check
        .as_ref()
        .is_some_and(|(_, previous)| *previous == modified)
    {
        *last_check = Some((now, modified));
        return;
    }
    *last_check = Some((now, modified));

    let next_paused = fs::read_to_string(path)
        .map(|value| value.trim().eq_ignore_ascii_case("paused"))
        .unwrap_or(false);
    if next_paused == paused.0 {
        return;
    }
    paused.0 = next_paused;
    if next_paused {
        virtual_time.pause();
        info!("Game paused");
    } else {
        virtual_time.unpause();
        info!("Game resumed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        app::TaskPoolPlugin,
        asset::{
            io::{
                memory::{Dir, MemoryAssetReader},
                AssetSourceBuilder, AssetSourceId,
            },
            AssetApp, AssetPlugin,
        },
        scene::ScenePlugin,
    };
    use std::time::Duration;

    #[derive(Component)]
    struct RuntimeOnlyComponent;

    #[derive(Component, Reflect, Default, Debug, PartialEq)]
    #[reflect(Component, Default)]
    struct ReflectedCustomComponent {
        speed: f32,
        active: bool,
        offset: Vec2,
    }

    #[derive(Resource, Default)]
    struct RegisteredSystemRuns(u32);

    fn registered_scene_system(mut runs: ResMut<RegisteredSystemRuns>) {
        runs.0 += 1;
    }

    #[derive(Resource, Default)]
    struct EntityScriptRuns {
        started: Vec<Entity>,
        updated: Vec<Entity>,
    }

    fn entity_script_start(In(entity): In<Entity>, mut runs: ResMut<EntityScriptRuns>) {
        runs.started.push(entity);
    }

    fn entity_script_start_second(In(entity): In<Entity>, mut runs: ResMut<EntityScriptRuns>) {
        runs.started.push(entity);
    }

    fn entity_script_update(In(entity): In<Entity>, mut runs: ResMut<EntityScriptRuns>) {
        runs.updated.push(entity);
    }

    fn test_temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/test-temp")
            .join(format!("{name}-{nonce}.bsn"))
    }

    fn scene() -> SceneFile {
        SceneFile {
            format_version: SCENE_FORMAT_VERSION,
            root: "root".into(),
            entities: vec![
                SceneNodeData {
                    id: "root".into(),
                    parent: None,
                    order: 0,
                    name: "Node2D".into(),
                    kind: "2d".into(),
                    space: None,
                    components: Vec::new(),
                    custom_components: Vec::new(),
                    entity_script: None,
                    systems: Vec::new(),
                    ui_layout: None,
                    ui_content: None,
                    sprite: None,
                    model: None,
                    animation_player: None,
                    collision_rect: None,
                    translation: (0.0, 0.0, 0.0),
                    rotation: (0.0, 0.0, 0.0, 1.0),
                    scale: (1.0, 1.0, 1.0),
                },
                SceneNodeData {
                    id: "child".into(),
                    parent: Some("root".into()),
                    order: 0,
                    name: "Child".into(),
                    kind: "2d".into(),
                    space: None,
                    components: Vec::new(),
                    custom_components: Vec::new(),
                    entity_script: None,
                    systems: Vec::new(),
                    ui_layout: None,
                    ui_content: None,
                    sprite: None,
                    model: None,
                    animation_player: None,
                    collision_rect: None,
                    translation: (1.0, 2.0, 0.0),
                    rotation: (0.0, 0.0, 0.0, 1.0),
                    scale: (1.0, 1.0, 1.0),
                },
            ],
        }
    }

    #[test]
    fn valid_scene_accepts_a_connected_hierarchy() {
        assert_eq!(validate_scene(&scene()), Ok(()));
    }

    #[test]
    fn valid_scene_accepts_animation_player_entity() {
        let mut value = scene();
        value.entities[1].kind = "animation_player".into();
        value.entities[1].space = Some("2d".into());
        value.entities[1].animation_player = Some(SceneAnimationPlayer {
            autoplay: "Idle".into(),
            speed: 1.25,
            clips: vec![SceneAnimationClip {
                name: "Idle".into(),
                length: 0.8,
                looped: true,
                tracks: Vec::new(),
            }],
        });

        let source = scene_file_to_bsn(&value).unwrap();
        let loaded = scene_file_from_bsn(&source).unwrap();

        assert_eq!(loaded, value);
        assert!(source.contains("kind: \"animation_player\""));
        assert!(source.contains("animation_player: Some("));
    }

    #[test]
    fn reflected_custom_component_is_materialized_with_authored_fields() {
        let type_path = std::any::type_name::<ReflectedCustomComponent>();
        let mut app = App::new();
        app.register_type::<ReflectedCustomComponent>()
            .add_systems(Update, materialize_reflected_custom_components);
        let entity = app
            .world_mut()
            .spawn(RuntimeCustomComponents(vec![SceneCustomComponent {
                type_path: type_path.into(),
                source_path: "res://src/custom.rs".into(),
                fields: vec![
                    SceneCustomField {
                        name: "speed".into(),
                        type_name: "f32".into(),
                        value: "8.5".into(),
                    },
                    SceneCustomField {
                        name: "active".into(),
                        type_name: "bool".into(),
                        value: "true".into(),
                    },
                    SceneCustomField {
                        name: "offset".into(),
                        type_name: "Vec2".into(),
                        value: "3.0, 4.0".into(),
                    },
                ],
            }]))
            .id();

        app.update();

        assert_eq!(
            app.world().get::<ReflectedCustomComponent>(entity),
            Some(&ReflectedCustomComponent {
                speed: 8.5,
                active: true,
                offset: Vec2::new(3.0, 4.0),
            })
        );
        assert!(app.world().get::<RuntimeCustomComponents>(entity).is_some());
    }

    #[test]
    fn registered_system_runs_only_for_enabled_matching_scene_binding() {
        let system_path = "sample_game::systems::registered_scene_system";
        let mut app = App::new();
        app.init_resource::<RegisteredSystemRuns>();
        add_arisna_system(
            &mut app,
            SceneSystemSchedule::Update,
            system_path,
            registered_scene_system,
        );

        app.update();
        assert_eq!(app.world().resource::<RegisteredSystemRuns>().0, 0);

        let entity = app
            .world_mut()
            .spawn(RuntimeSystemBindings(vec![SceneSystemBinding {
                script_path: "res://src/systems.rs".into(),
                system_path: system_path.into(),
                schedule: SceneSystemSchedule::Update,
                enabled: true,
                before: Vec::new(),
                after: Vec::new(),
            }]))
            .id();
        app.update();
        assert_eq!(app.world().resource::<RegisteredSystemRuns>().0, 1);

        app.world_mut()
            .get_mut::<RuntimeSystemBindings>(entity)
            .unwrap()
            .0[0]
            .schedule = SceneSystemSchedule::PostUpdate;
        app.update();
        assert_eq!(app.world().resource::<RegisteredSystemRuns>().0, 2);

        app.world_mut()
            .get_mut::<RuntimeSystemBindings>(entity)
            .unwrap()
            .0[0]
            .enabled = false;
        app.update();
        assert_eq!(app.world().resource::<RegisteredSystemRuns>().0, 2);
    }

    #[test]
    fn registered_entity_script_receives_its_scene_entity_and_runs_lifecycles() {
        let source_path = "res://src/player.rs";
        let mut app = App::new();
        app.init_resource::<EntityScriptRuns>();
        add_arisna_entity_script(
            &mut app,
            EntityScriptLifecycle::Start,
            source_path,
            entity_script_start,
        );
        add_arisna_entity_script_fn(
            &mut app,
            EntityScriptLifecycle::Start,
            source_path,
            "sample_game::player::start_second",
            entity_script_start_second,
        );
        add_arisna_entity_script(
            &mut app,
            EntityScriptLifecycle::Update,
            source_path,
            entity_script_update,
        );
        let entity = app
            .world_mut()
            .spawn(RuntimeEntityScript(Some(SceneEntityScript {
                source_path: source_path.into(),
                type_path: "script::src::player".into(),
                enabled: true,
                callbacks: Vec::new(),
            })))
            .id();

        app.update();
        app.update();

        let runs = app.world().resource::<EntityScriptRuns>();
        assert_eq!(runs.started, vec![entity, entity]);
        assert_eq!(runs.updated, vec![entity, entity]);
    }

    #[test]
    fn invalid_scene_rejects_missing_parent() {
        let mut value = scene();
        value.entities[1].parent = Some("missing".into());
        assert!(validate_scene(&value)
            .unwrap_err()
            .contains("missing parent"));
    }

    #[test]
    fn launch_arguments_include_scene_and_control_file() {
        let plugin = SceneRuntimePlugin::from_args([
            OsString::from("--scene"),
            OsString::from("assets/scenes/main.bsn"),
            OsString::from("--control"),
            OsString::from(".arisna/run-control"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(
            plugin.relative_path,
            PathBuf::from("assets/scenes/main.bsn")
        );
        assert_eq!(
            plugin.control_path,
            Some(PathBuf::from(".arisna/run-control"))
        );
    }

    #[test]
    fn bsn_hot_reload_keeps_old_scene_on_error_then_applies_node_diff() {
        let path = test_temp_path("scene-hot-reload");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let initial = scene();
        let initial_source = scene_file_to_bsn(&initial).unwrap();
        fs::write(&path, &initial_source).unwrap();

        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Image>()
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .insert_resource(Time::<Real>::default())
            .insert_resource(GamePaused(true))
            .insert_resource(ActiveScene {
                relative_path: PathBuf::from("assets/scenes/test.bsn"),
                root_id: "root".into(),
            })
            .insert_resource(SceneHotReloadState {
                path: path.clone(),
                display_path: "res://assets/scenes/test.bsn".into(),
                applied_hash: scene_content_hash(initial_source.as_bytes()),
                observed_hash: scene_content_hash(initial_source.as_bytes()),
                last_check: 0.0,
                last_error: None,
            })
            .add_systems(Update, hot_reload_bsn_scene);

        let root = app
            .world_mut()
            .spawn((
                Name::new("Node2D"),
                initial.entities[0].clone(),
                RuntimeSceneNode {
                    id: "root".into(),
                    parent_id: None,
                    order: 0,
                    kind: "2d".into(),
                },
                Transform::default(),
                RuntimeOnlyComponent,
            ))
            .id();
        let child = app
            .world_mut()
            .spawn((
                Name::new("Child"),
                initial.entities[1].clone(),
                RuntimeSceneNode {
                    id: "child".into(),
                    parent_id: Some("root".into()),
                    order: 0,
                    kind: "2d".into(),
                },
                Transform::from_xyz(1.0, 2.0, 0.0),
                ChildOf(root),
            ))
            .id();

        fs::write(&path, "this is not a valid scene").unwrap();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(200));
        app.update();

        assert!(app.world().get_entity(root).is_ok());
        assert!(app.world().get_entity(child).is_ok());
        assert!(app.world().get::<RuntimeOnlyComponent>(root).is_some());
        let failed_state = app.world().resource::<SceneHotReloadState>();
        assert!(failed_state.last_error.is_some());
        assert_ne!(failed_state.applied_hash, failed_state.observed_hash);

        let mut updated = initial;
        updated.entities[0].name = "Renamed Root".into();
        updated.entities[0].translation = (12.0, 24.0, 0.0);
        updated.entities.remove(1);
        updated.entities.push(SceneNodeData {
            id: "replacement".into(),
            parent: Some("root".into()),
            order: 0,
            name: "Replacement".into(),
            kind: "sprite2d".into(),
            space: Some("2d".into()),
            components: Vec::new(),
            custom_components: Vec::new(),
            entity_script: None,
            systems: Vec::new(),
            ui_layout: None,
            ui_content: None,
            sprite: Some(SceneSprite2D {
                image_path: "res://images/replacement.png".into(),
                ..default()
            }),
            model: None,
            animation_player: None,
            collision_rect: None,
            translation: (3.0, 4.0, 0.0),
            rotation: (0.0, 0.0, 0.0, 1.0),
            scale: (1.0, 1.0, 1.0),
        });
        let updated_source = scene_file_to_bsn(&updated).unwrap();
        fs::write(&path, &updated_source).unwrap();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(200));
        app.update();

        assert!(app.world().get_entity(child).is_err());
        assert_eq!(
            app.world().get::<Name>(root).unwrap().as_str(),
            "Renamed Root"
        );
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().translation,
            Vec3::new(12.0, 24.0, 0.0)
        );
        assert!(app.world().get::<RuntimeOnlyComponent>(root).is_some());
        let replacement = app
            .world()
            .iter_entities()
            .find(|entity| {
                entity
                    .get::<RuntimeSceneNode>()
                    .is_some_and(|node| node.id == "replacement")
            })
            .unwrap();
        assert_eq!(
            replacement.get::<ChildOf>().map(ChildOf::parent),
            Some(root)
        );
        assert_eq!(
            replacement.get::<SceneSprite2D>().unwrap().image_path,
            "res://images/replacement.png"
        );
        let state = app.world().resource::<SceneHotReloadState>();
        assert_eq!(
            state.applied_hash,
            scene_content_hash(updated_source.as_bytes())
        );
        assert_eq!(state.observed_hash, state.applied_hash);
        assert!(state.last_error.is_none());
        assert_eq!(app.world().resource::<ActiveScene>().root_id, "root");
        assert!(app.world().resource::<GamePaused>().0);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn bsn_round_trip_preserves_hierarchy_transform_and_ui_data() {
        let mut value = scene();
        value.entities[1].kind = "button".into();
        value.entities[1].ui_layout = Some(SceneUiLayout {
            anchor_min: (0.5, 0.5),
            anchor_max: (0.5, 0.5),
            offset: (-60.0, -18.0),
            size: (120.0, 36.0),
            minimum_size: (80.0, 24.0),
            clip_contents: true,
            rotation: 15.0,
            scale: (1.2, 0.8),
            pivot_offset: (4.0, 6.0),
            pivot_ratio: (0.5, 0.5),
            margin: (1.0, 2.0, 3.0, 4.0),
            horizontal_alignment: UiAlignment::Center,
            vertical_alignment: UiAlignment::End,
        });
        value.entities[1].ui_content = Some(SceneUiContent {
            text: "Play".into(),
            panel_color: (0.2, 0.4, 0.8, 1.0),
            image_path: "res://ui/play.png".into(),
        });

        let source = scene_file_to_bsn(&value).unwrap();
        let loaded = scene_file_from_bsn(&source).unwrap();

        assert_eq!(loaded, value);
        assert!(source.contains("Children ["));
        assert!(source.contains("Transform {"));
        assert!(source.contains("ui_layout: Some("));
        assert!(source.contains("clip_contents: true"));
        assert!(source.contains("rotation: 15"));
    }

    #[test]
    fn ui_transform_uses_top_left_pivot_and_persisted_values() {
        let mut layout = SceneUiLayout::sized(100.0, 40.0);
        assert_eq!(scene_ui_transform(layout), UiTransform::default());

        layout.scale = (2.0, 2.0);
        let top_left = scene_ui_transform(layout);
        assert_eq!(top_left.scale, Vec2::splat(2.0));
        assert_eq!(top_left.translation, Val2::px(50.0, 20.0));

        layout.pivot_ratio = (0.5, 0.5);
        let centered = scene_ui_transform(layout);
        assert_eq!(centered.translation, Val2::ZERO);
    }

    #[test]
    fn bsn_round_trip_preserves_sprite_data() {
        let mut value = scene();
        value.entities[1].kind = "sprite2d".into();
        value.entities[1].sprite = Some(SceneSprite2D {
            image_path: "res://sprites/hero.png".into(),
            color: (0.8, 0.7, 0.6, 0.5),
            flip_x: true,
            flip_y: false,
            hframes: 4,
            vframes: 2,
            frame: 5,
            visible: false,
            region_enabled: true,
            region_rect: (16.0, 24.0, 32.0, 48.0),
            z_index: 7,
            anchor: (0.0, 0.0),
        });

        let source = scene_file_to_bsn(&value).unwrap();
        let loaded = scene_file_from_bsn(&source).unwrap();

        assert_eq!(loaded, value);
        assert!(source.contains("sprite: Some("));
        assert!(source.contains("image_path: \"res://sprites/hero.png\""));
        assert!(source.contains("region_enabled: true"));
        assert!(source.contains("region_rect:"));
        assert!(source.contains("z_index: 7"));
        assert!(source.contains("visible: false"));
    }

    #[test]
    fn sprite_sheet_rect_uses_texture_size_and_clamps_frame() {
        let sprite = SceneSprite2D {
            hframes: 4,
            vframes: 2,
            frame: 99,
            ..default()
        };
        let rect = scene_sprite_frame_rect(&sprite, Vec2::new(400.0, 200.0)).unwrap();
        assert_eq!(rect.min, Vec2::new(300.0, 100.0));
        assert_eq!(rect.max, Vec2::new(400.0, 200.0));
    }

    #[test]
    fn old_sprite_bsn_defaults_to_single_frame() {
        let mut value = scene();
        value.entities[1].kind = "sprite2d".into();
        value.entities[1].sprite = Some(SceneSprite2D::default());
        let source = scene_file_to_bsn(&value).unwrap();
        let legacy_source = source
            .lines()
            .filter(|line| {
                !line.contains("hframes:") && !line.contains("vframes:") && !line.contains("frame:")
            })
            .collect::<Vec<_>>()
            .join("\n");

        let loaded = scene_file_from_bsn(&legacy_source).unwrap();
        let sprite = loaded.entities[1].sprite.as_ref().unwrap();
        assert_eq!(sprite.hframes, 1);
        assert_eq!(sprite.vframes, 1);
        assert_eq!(sprite.frame, 0);
    }

    #[test]
    fn animation_player_defaults_to_realtime_speed() {
        let player = SceneAnimationPlayer::default();
        assert_eq!(player.speed, 1.0);
        assert!(player.autoplay.is_empty());
        assert!(player.clips.is_empty());
    }

    #[test]
    fn bsn_round_trip_preserves_animation_player_tracks() {
        let mut value = scene();
        value.entities[0].animation_player = Some(SceneAnimationPlayer {
            autoplay: "idle".into(),
            speed: 1.25,
            clips: vec![SceneAnimationClip {
                name: "idle".into(),
                length: 0.8,
                looped: true,
                tracks: vec![SceneAnimationTrack {
                    target_node: value.entities[1].id.clone(),
                    property: "transform.translation".into(),
                    kind: SceneAnimationTrackKind::Transform,
                    keys: vec![
                        SceneAnimationKey {
                            time: 0.0,
                            value: "0,0,0".into(),
                        },
                        SceneAnimationKey {
                            time: 0.8,
                            value: "32,0,0".into(),
                        },
                    ],
                }],
            }],
        });

        let source = scene_file_to_bsn(&value).unwrap();
        let loaded = scene_file_from_bsn(&source).unwrap();

        assert_eq!(loaded, value);
        assert!(source.contains("animation_player: Some("));
        assert!(source.contains("target_node:"));
        assert!(source.contains("kind: Transform"));
    }

    #[test]
    fn bsn_round_trip_preserves_collision_rect_data() {
        let mut value = scene();
        value.entities[1].kind = "collision_rect2d".into();
        value.entities[1].space = Some("2d".into());
        value.entities[1].collision_rect = Some(SceneCollisionRect2D {
            size: (320.0, 48.0),
            offset: (12.0, 8.0),
            enabled: false,
        });

        let source = scene_file_to_bsn(&value).unwrap();
        let loaded = scene_file_from_bsn(&source).unwrap();

        assert_eq!(loaded, value);
        assert!(source.contains("collision_rect: Some("));
        assert!(source.contains("size: (320.0, 48.0)"));
        assert!(source.contains("enabled: false"));
    }

    #[test]
    fn bsn_round_trip_preserves_mesh3d_model_resource() {
        let mut value = scene();
        value.entities[1].kind = "mesh3d".into();
        value.entities[1].space = Some("3d".into());
        value.entities[1].model = Some(SceneModel3D {
            resource_path: "res://models/hero.glb".into(),
        });

        let source = scene_file_to_bsn(&value).unwrap();
        let loaded = scene_file_from_bsn(&source).unwrap();

        assert_eq!(loaded, value);
        assert!(source.contains("model: Some("));
        assert!(source.contains("resource_path: \"res://models/hero.glb\""));
    }

    #[test]
    fn bsn_round_trip_preserves_explicit_system_bindings() {
        let mut value = scene();
        value.entities[1].systems = vec![SceneSystemBinding {
            script_path: "res://src/systems/camera_follow.rs".into(),
            system_path: "sample_game::systems::camera_follow".into(),
            schedule: SceneSystemSchedule::PostUpdate,
            enabled: true,
            before: Vec::new(),
            after: Vec::new(),
        }];

        let source = scene_file_to_bsn(&value).unwrap();
        let loaded = scene_file_from_bsn(&source).unwrap();

        assert_eq!(loaded, value);
        assert!(source.contains("systems: ["));
        assert!(source.contains("SceneSystemBinding {"));
        assert!(source.contains("schedule: PostUpdate"));
    }

    #[test]
    fn image_resource_paths_are_canonical_for_editor_and_runtime() {
        assert_eq!(
            scene_image_asset_path("res://assets/ui/icon.png").as_deref(),
            Some("ui/icon.png")
        );
        assert_eq!(
            scene_image_asset_path("project://ui/icon.png").as_deref(),
            Some("ui/icon.png")
        );
        assert_eq!(
            scene_image_resource_path("assets/ui/icon.png").as_deref(),
            Some("res://ui/icon.png")
        );
        assert_eq!(scene_image_asset_path("res://../outside.png"), None);
    }

    #[test]
    fn model_resource_paths_support_fbx_gltf_and_glb() {
        for extension in ["fbx", "gltf", "glb"] {
            let resource = format!("res://models/hero.{extension}");
            assert_eq!(
                scene_model_asset_path(&resource).as_deref(),
                Some(format!("models/hero.{extension}#Scene0").as_str())
            );
            assert_eq!(
                scene_model_resource_path(&resource).as_deref(),
                Some(resource.as_str())
            );
        }
        assert_eq!(scene_model_asset_path("res://models/hero.blend"), None);
        assert_eq!(scene_model_asset_path("res://../hero.glb"), None);
    }

    #[test]
    fn materializes_loaded_bsn_metadata_and_default_camera() {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Image>()
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .insert_resource(ActiveScene {
                relative_path: PathBuf::from("assets/scenes/test.bsn"),
                root_id: String::new(),
            })
            .add_systems(Update, materialize_bsn_scene_nodes);

        let root = app
            .world_mut()
            .spawn((
                Name::new("Root"),
                SceneNodeData {
                    id: "root".into(),
                    parent: None,
                    order: 0,
                    name: "Root".into(),
                    kind: "empty2d".into(),
                    space: Some("2d".into()),
                    components: Vec::new(),
                    custom_components: vec![SceneCustomComponent {
                        type_path: "jjxf_client::world::entities::MovementState".into(),
                        source_path: "res://src/world/entities.rs".into(),
                        fields: vec![SceneCustomField {
                            name: "state".into(),
                            type_name: "i32".into(),
                            value: "7".into(),
                        }],
                    }],
                    entity_script: None,
                    systems: Vec::new(),
                    ui_layout: None,
                    ui_content: None,
                    sprite: None,
                    model: None,
                    animation_player: None,
                    collision_rect: None,
                    translation: (3.0, 4.0, 0.0),
                    rotation: (0.0, 0.0, 0.0, 1.0),
                    scale: (2.0, 2.0, 1.0),
                },
            ))
            .id();
        let sprite_entity = app
            .world_mut()
            .spawn((
                Name::new("Hero"),
                SceneNodeData {
                    id: "hero".into(),
                    parent: Some("root".into()),
                    order: 0,
                    name: "Hero".into(),
                    kind: "sprite2d".into(),
                    space: Some("2d".into()),
                    components: Vec::new(),
                    custom_components: Vec::new(),
                    entity_script: None,
                    systems: Vec::new(),
                    ui_layout: None,
                    ui_content: None,
                    sprite: Some(SceneSprite2D {
                        image_path: "res://sprites/hero.png".into(),
                        color: (0.2, 0.4, 0.6, 0.8),
                        flip_x: true,
                        flip_y: false,
                        ..default()
                    }),
                    model: None,
                    animation_player: None,
                    collision_rect: None,
                    translation: (0.0, 0.0, 0.0),
                    rotation: (0.0, 0.0, 0.0, 1.0),
                    scale: (1.0, 1.0, 1.0),
                },
            ))
            .id();

        app.update();

        assert_eq!(
            app.world().get::<RuntimeSceneNode>(root),
            Some(&RuntimeSceneNode {
                id: "root".into(),
                parent_id: None,
                order: 0,
                kind: "empty2d".into(),
            })
        );
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().translation,
            Vec3::new(3.0, 4.0, 0.0)
        );
        assert!(app.world().get::<Visibility>(root).is_some());
        let sprite = app.world().get::<Sprite>(sprite_entity).unwrap();
        assert!(sprite.flip_x);
        assert!(!sprite.flip_y);
        assert_eq!(sprite.custom_size, None);
        assert_eq!(
            app.world().get::<Anchor>(sprite_entity),
            Some(&Anchor::TOP_LEFT)
        );
        assert_eq!(app.world().resource::<ActiveScene>().root_id, "root");
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<Camera2d>>()
                .iter(app.world())
                .count(),
            1
        );
    }

    #[test]
    fn asset_server_loads_bsn_into_runtime_ecs_hierarchy() {
        let mut scene = scene();
        scene.entities[1].ui_layout = Some(SceneUiLayout::sized(180.0, 40.0));
        scene.entities[1].ui_content = Some(SceneUiContent {
            text: "Loaded from BSN".into(),
            panel_color: (0.1, 0.2, 0.3, 1.0),
            image_path: "res://ui/icon.png".into(),
        });
        scene.entities.push(SceneNodeData {
            id: "image".into(),
            parent: Some("root".into()),
            order: 1,
            name: "Logo".into(),
            kind: "image".into(),
            space: Some("2d".into()),
            components: Vec::new(),
            custom_components: Vec::new(),
            entity_script: None,
            systems: Vec::new(),
            ui_layout: Some(SceneUiLayout::sized(128.0, 128.0)),
            ui_content: Some(SceneUiContent {
                image_path: "res://assets/ui/logo.png".into(),
                ..default()
            }),
            sprite: None,
            model: None,
            animation_player: None,
            collision_rect: None,
            translation: (0.0, 0.0, 0.0),
            rotation: (0.0, 0.0, 0.0, 1.0),
            scale: (1.0, 1.0, 1.0),
        });
        let source = scene_file_to_bsn(&scene).unwrap();
        let dir = Dir::default();
        let reader_dir = dir.clone();
        let mut app = App::new();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || {
                Box::new(MemoryAssetReader {
                    root: reader_dir.clone(),
                })
            }),
        );
        app.add_plugins((
            TaskPoolPlugin::default(),
            AssetPlugin::default(),
            ScenePlugin,
            crate::EnginePlugin::new("."),
        ));
        app.init_asset::<Image>()
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .insert_resource(ActiveScene {
                relative_path: PathBuf::from("assets/scenes/runtime_scene.bsn"),
                root_id: String::new(),
            })
            .add_systems(Update, materialize_bsn_scene_nodes);
        app.finish();
        app.cleanup();

        dir.insert_asset_text(Path::new("runtime_scene.bsn"), &source);
        let asset_server = app.world().resource::<AssetServer>().clone();
        let handle: Handle<ScenePatch> = asset_server.load("runtime_scene.bsn");
        for _ in 0..1_000 {
            app.update();
            if asset_server.is_loaded_with_dependencies(&handle) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            asset_server.is_loaded_with_dependencies(&handle),
            "BSN asset did not finish loading"
        );

        let root = app.world_mut().spawn(ScenePatchInstance(handle)).id();
        for _ in 0..8 {
            app.update();
        }

        let root_node = app.world().get::<RuntimeSceneNode>(root).unwrap();
        assert_eq!(root_node.id, "root");
        assert_eq!(app.world().get::<Name>(root).unwrap().as_str(), "Node2D");
        let children = app.world().get::<Children>(root).unwrap();
        assert_eq!(children.len(), 2);
        let child = children
            .iter()
            .find(|entity| {
                app.world()
                    .get::<RuntimeSceneNode>(*entity)
                    .is_some_and(|node| node.id == "child")
            })
            .unwrap();
        let image_entity = children
            .iter()
            .find(|entity| {
                app.world()
                    .get::<RuntimeSceneNode>(*entity)
                    .is_some_and(|node| node.id == "image")
            })
            .unwrap();
        assert_eq!(
            app.world().get::<RuntimeSceneNode>(child).unwrap().id,
            "child"
        );
        assert_eq!(app.world().get::<ChildOf>(child).unwrap().parent(), root);
        assert_eq!(
            app.world().get::<Transform>(child).unwrap().translation,
            Vec3::new(1.0, 2.0, 0.0)
        );
        let child_data = app.world().get::<SceneNodeData>(child).unwrap();
        assert_eq!(
            child_data.ui_layout,
            Some(SceneUiLayout::sized(180.0, 40.0))
        );
        assert_eq!(
            child_data.ui_content.as_ref().unwrap().text,
            "Loaded from BSN"
        );
        let image = app.world().get::<ImageNode>(image_entity).unwrap();
        assert_eq!(image.image.path().unwrap().to_string(), "ui/logo.png");
        assert_eq!(app.world().resource::<ActiveScene>().root_id, "root");
    }
}
