#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! 默认游戏项目入口，也是生成项目和运行时集成的最小参考。
//!
//! 游戏业务 Plugin、反射组件、Entity Script 和 System 都应在 `app.run()` 前
//! 注册；编辑器运行只会改写 target 内的生成副本，不会改写本文件。

use std::path::Path;

use revy_engine::{
    EnginePlugin, FbxPlugin, ProjectSettings, SceneRuntimePlugin,
    bevy::{asset::AssetPlugin, prelude::*},
    configure_process, native_render_plugin,
};

fn main() {
    configure_process();
    // 独立运行时使用当前项目根；编辑器内运行可由 EnginePlugin 的
    // REVY_PROJECT_ROOT 覆盖到真实源项目；旧 ARISNA_* 名称仍兼容。
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project_settings = ProjectSettings::load(project_root).unwrap_or_else(|error| {
        eprintln!("Revy project settings error: {error}");
        ProjectSettings::default()
    });
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(AssetPlugin {
                file_path: project_root.join("assets").to_string_lossy().into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(project_settings.game_window(
                    std::env::var_os("REVY_EMBEDDED").is_none()
                        && std::env::var_os("ARISNA_EMBEDDED").is_none(),
                )),
                ..default()
            })
            .set(native_render_plugin()),
        EnginePlugin::new(project_root),
        FbxPlugin,
    ));
    match SceneRuntimePlugin::from_args(std::env::args_os().skip(1)) {
        // 编辑器通过 --scene/--control 指定当前运行场景及暂停控制文件。
        Ok(Some(runtime)) => {
            app.add_plugins(runtime);
        }
        Ok(None) => {}
        Err(error) => eprintln!("Revy launch error: {error}"),
    }
    app.run();
}
