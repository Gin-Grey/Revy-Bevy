#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Revy Rust 游戏项目入口模板。
//!
//! 用户业务 Plugin、反射组件、Entity Script 和 System 都在 `app.run()` 前注册。
//! 编辑器运行时只修改 target 内的生成副本，不会向源项目注入临时代码。

use std::path::Path;

use revy_engine::{
    EnginePlugin, FbxPlugin, ProjectSettings, ProjectWindowSettings, SceneRuntimePlugin,
    bevy::{asset::AssetPlugin, prelude::*},
    configure_process, native_render_plugin,
};

// Inspector 绑定的 Entity Script 是编译后的 Rust System，不是解释脚本。
// 如需手工注册生命周期函数，应放在 `app.run()` 之前：
// add_revy_entity_script(
//     &mut app,
//     EntityScriptLifecycle::Update,
//     "res://src/player.rs",
//     player::update,
// );

fn main() {
    configure_process();
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project_settings = ProjectSettings::load(project_root).unwrap_or_else(|error| {
        eprintln!("Revy project settings error: {error}");
        ProjectSettings {
            name: "{{PROJECT_NAME}}".into(),
            window: ProjectWindowSettings {
                title: "{{PROJECT_NAME}}".into(),
                ..default()
            },
            ..default()
        }
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
        Ok(Some(runtime)) => {
            app.add_plugins(runtime);
        }
        Ok(None) => {}
        Err(error) => eprintln!("Revy launch error: {error}"),
    }
    app.run();
}
