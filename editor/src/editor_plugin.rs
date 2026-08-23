//! 编辑器功能组合根。
//!
//! 本文件只负责注册资源源和组合插件；具体业务状态应留在各自模块中，
//! 不要在这里实现 Scene、Inspector 或 FileSystem 的业务逻辑。

use arisna_engine::{FbxPlugin, ProjectRoot, native_render_plugin};
use bevy::{
    asset::{AssetApp, io::AssetSourceBuilder},
    prelude::*,
    window::{WindowResizeConstraints, WindowResolution},
};

use crate::{
    animation_timeline::AnimationTimelinePlugin,
    editor_menu::EditorMenuPlugin,
    entity_picker::EntityPickerPlugin,
    filesystem::FileSystemPlugin,
    hierarchy::HierarchyPlugin,
    inspector::InspectorPlugin,
    output::OutputPlugin,
    panels::PanelsPlugin,
    paths,
    play::GameRunnerPlugin,
    project_settings::ProjectSettingsPlugin,
    rust_components::RustComponentRegistryPlugin,
    scene::SceneDocumentPlugin,
    selection::{Selection, SelectionSet},
    ui::EditorUiPlugin,
    undo::SceneHistoryPlugin,
    viewport::ViewportPlugin,
    workspace::WorkspacePlugin,
};

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        if let Some(project) = app.world().get_resource::<ProjectRoot>() {
            // 编辑器图标使用默认 AssetSource，游戏项目资产使用 project://。
            // 两者分离后，FileSystem 才不会把编辑器内部资源暴露为 res://。
            let project_assets = project.root.join("assets").to_string_lossy().into_owned();
            app.register_asset_source(
                "project",
                AssetSourceBuilder::platform_default(&project_assets, None),
            );
        }
        // 插件之间通过 Resource、Message 和 Component 协作。新增功能优先
        // 注册为独立 Plugin，不要让 EditorPlugin 变成全局状态容器。
        app.add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Revy".into(),
                        // Respect the OS DPI scale so Winit and DXGI agree during resize.
                        resolution: WindowResolution::new(1600, 900),
                        resize_constraints: WindowResizeConstraints {
                            min_width: 900.0,
                            min_height: 600.0,
                            ..default()
                        },
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: editor_asset_root(),
                    ..default()
                })
                .set(native_render_plugin()),
            FbxPlugin,
            ViewportPlugin,
            EditorUiPlugin,
            HierarchyPlugin,
            EntityPickerPlugin,
            InspectorPlugin,
            WorkspacePlugin,
            PanelsPlugin,
            FileSystemPlugin,
            SceneDocumentPlugin,
            SceneHistoryPlugin,
            OutputPlugin,
            GameRunnerPlugin,
            ProjectSettingsPlugin,
        ))
        .add_plugins((
            EditorMenuPlugin,
            RustComponentRegistryPlugin,
            AnimationTimelinePlugin,
        ))
        .init_resource::<Selection>()
        .init_resource::<SelectionSet>();
    }
}

/// Debug builds load directly from the project so file watching works even
/// when the executable is launched outside Cargo. Packaged builds expect the
/// `assets` directory beside the distribution's working directory.
fn editor_asset_root() -> String {
    paths::editor_asset_root().to_string_lossy().into_owned()
}
