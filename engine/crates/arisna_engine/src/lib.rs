//! Revy 对编辑器和游戏公开的运行时门面。
//!
//! 上层只能通过本 crate 共享项目、场景和平台契约；编辑器专用类型不能反向
//! 进入这里，否则生成的游戏项目会被迫依赖编辑器实现。

mod animation;
mod platform;
mod project;
mod scene;

use std::path::PathBuf;

use bevy::prelude::*;

pub use animation::{
    animation_transform_from_ui_layout, apply_sample_to_transform_property,
    apply_sample_to_ui_layout, apply_sample_to_ui_layout_property, format_animation_transform,
    format_sprite_frame, parse_animation_transform, parse_sprite_frame, sample_animation_transform,
    sample_sprite_frame, RuntimeAnimationPlayback,
};
pub use bevy;
pub use bevy_ufbx::{Fbx, FbxLoaderSettings, FbxPlugin};
pub use platform::{configure_process, native_render_plugin, PlatformPlugin};
pub use project::{ProjectRoot, ProjectSettings, ProjectWindowMode, ProjectWindowSettings};
pub use scene::{
    add_arisna_entity_script, add_arisna_entity_script_fn, add_arisna_system,
    add_revy_entity_script, add_revy_entity_script_fn, add_revy_system, load_scene_file,
    scene_file_from_bsn, scene_file_to_bsn, scene_image_asset_path, scene_image_resource_path,
    scene_model_asset_path, scene_model_resource_path, scene_sprite_frame_rect, scene_sprite_rect,
    scene_ui_transform, ActiveScene, EntityScriptLifecycle, GamePaused, RuntimeCustomComponents,
    RuntimeEntityScript, RuntimeSceneNode, RuntimeSystemBindings, SceneAnimationClip,
    SceneAnimationKey, SceneAnimationPlayer, SceneAnimationTrack, SceneAnimationTrackKind,
    SceneCollisionRect2D, SceneCustomComponent, SceneCustomField, SceneEntityScript,
    SceneEntityScriptCallback, SceneFile, SceneModel3D, SceneNodeData, SceneRuntimePlugin,
    SceneSprite2D, SceneSystemBinding, SceneSystemSchedule, SceneUiContent, SceneUiLayout,
    UiAlignment,
};

/// 编辑器和游戏共同使用的引擎插件。
///
/// 负责安装项目根、反射类型和平台能力，不负责编辑器面板或游戏业务系统。
pub struct EnginePlugin {
    project_root: PathBuf,
}

impl EnginePlugin {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
        }
    }
}

impl Plugin for EnginePlugin {
    fn build(&self, app: &mut App) {
        // 编辑器内运行时，游戏可执行文件位于 target 的生成项目中；这个环境变量
        // 把资源解析重新指向用户的真实项目根。普通独立启动则使用构造参数。
        let project_root = std::env::var_os("REVY_PROJECT_ROOT")
            .or_else(|| std::env::var_os("ARISNA_PROJECT_ROOT"))
            .map(PathBuf::from)
            .unwrap_or_else(|| self.project_root.clone());
        // 场景中允许出现的共享类型必须在这里注册。新增持久化类型后，审核者应
        // 同时检查编辑器保存、运行时物化和 round-trip 测试。
        app.insert_resource(ProjectRoot::new(project_root))
            .register_type::<Name>()
            .register_type::<Transform>()
            .register_type::<SceneNodeData>()
            .register_type::<SceneCustomComponent>()
            .register_type::<SceneCustomField>()
            .register_type::<RuntimeCustomComponents>()
            .register_type::<RuntimeEntityScript>()
            .register_type::<SceneEntityScript>()
            .register_type::<SceneEntityScriptCallback>()
            .register_type::<EntityScriptLifecycle>()
            .register_type::<RuntimeSystemBindings>()
            .register_type::<SceneModel3D>()
            .register_type::<SceneAnimationPlayer>()
            .register_type::<SceneAnimationClip>()
            .register_type::<SceneAnimationTrack>()
            .register_type::<SceneAnimationTrackKind>()
            .register_type::<SceneAnimationKey>()
            .register_type::<SceneCollisionRect2D>()
            .register_type::<SceneSprite2D>()
            .register_type::<SceneSystemBinding>()
            .register_type::<SceneSystemSchedule>()
            .register_type::<SceneUiContent>()
            .register_type::<SceneUiLayout>()
            .register_type::<UiAlignment>()
            .add_plugins(platform::PlatformPlugin);
    }
}
