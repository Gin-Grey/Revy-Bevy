#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Revy 桌面编辑器入口。
//!
//! 审核时先看这里：未传项目路径时只启动项目管理器；选定项目后，
//! 才会创建编辑器 App，并依次挂载共享引擎层和编辑器功能层。

mod animation_timeline;
mod editor_menu;
mod editor_plugin;
mod entities;
mod entity_picker;
mod filesystem;
mod hierarchy;
mod inspector;
mod output;
mod panels;
mod paths;
mod play;
mod project_manager;
mod project_settings;
mod rust_components;
mod scene;
mod selection;
mod ui;
mod undo;
mod viewport;
mod workspace;

use arisna_engine::{EnginePlugin, configure_process};
use bevy::prelude::*;
use editor_plugin::EditorPlugin;

fn main() {
    configure_process();

    // 项目管理器和完整编辑器使用不同生命周期，避免在“尚未选定项目”时
    // 提前初始化渲染、文件监听和项目级资源。
    let Some(project_root) = filesystem::project_path_from_args(std::env::args_os().skip(1)) else {
        project_manager::run();
        return;
    };

    let project_root = if project_root.is_absolute() {
        project_root
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(project_root)
    };
    project_manager::remember_project(&project_root);

    // EnginePlugin 定义编辑器/游戏共享契约；EditorPlugin 只增加编辑工具。
    // 保持这个方向可以防止游戏运行时反向依赖编辑器代码。
    App::new()
        .add_plugins(EnginePlugin::new(project_root))
        .add_plugins(EditorPlugin)
        .run();
}
