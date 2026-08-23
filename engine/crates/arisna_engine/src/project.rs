//! 游戏项目配置与路径安全边界。
//!
//! 本模块是 `project.toml` 和 `res://` 磁盘访问的共享契约。编辑器和运行时
//! 都应通过这里解析项目路径，避免各自实现不一致的越界检查。

use std::fs;
use std::path::{Component, Path, PathBuf};

use bevy::{
    prelude::*,
    window::{MonitorSelection, PresentMode, VideoModeSelection, WindowMode, WindowResolution},
};
use toml_edit::DocumentMut;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProjectWindowMode {
    #[default]
    Windowed,
    Borderless,
    Fullscreen,
}

impl ProjectWindowMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windowed => "windowed",
            Self::Borderless => "borderless",
            Self::Fullscreen => "fullscreen",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "windowed" => Some(Self::Windowed),
            "borderless" | "borderless_fullscreen" => Some(Self::Borderless),
            "fullscreen" => Some(Self::Fullscreen),
            _ => None,
        }
    }

    fn to_bevy(self) -> WindowMode {
        match self {
            Self::Windowed => WindowMode::Windowed,
            Self::Borderless => WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
            Self::Fullscreen => {
                WindowMode::Fullscreen(MonitorSelection::Primary, VideoModeSelection::Current)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWindowSettings {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub mode: ProjectWindowMode,
    pub vsync: bool,
}

impl Default for ProjectWindowSettings {
    fn default() -> Self {
        Self {
            title: "Revy Game".into(),
            width: 1280,
            height: 720,
            mode: ProjectWindowMode::Windowed,
            vsync: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSettings {
    pub name: String,
    pub main_scene: String,
    pub window: ProjectWindowSettings,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            name: "Revy Game".into(),
            main_scene: String::new(),
            window: ProjectWindowSettings::default(),
        }
    }
}

impl ProjectSettings {
    /// 从项目根目录读取基础设置。
    ///
    /// 解析失败由调用方决定是阻止启动还是使用默认值；本函数不会静默改写文件。
    pub fn load(project_root: &Path) -> Result<Self, String> {
        let manifest_path = project_root.join("project.toml");
        let source = fs::read_to_string(&manifest_path)
            .map_err(|error| format!("Could not read {}: {error}", manifest_path.display()))?;
        let document = source
            .parse::<DocumentMut>()
            .map_err(|error| format!("Invalid {}: {error}", manifest_path.display()))?;
        let project = document
            .get("project")
            .and_then(|item| item.as_table_like())
            .ok_or_else(|| "project.toml is missing [project]".to_string())?;

        let name = project
            .get("name")
            .and_then(|item| item.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Revy Game")
            .to_owned();
        let main_scene = project
            .get("main_scene")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .trim()
            .replace('\\', "/");

        let window_table = document
            .get("display")
            .and_then(|display| display.get("window"))
            .and_then(|window| window.as_table_like());
        let title = window_table
            .and_then(|window| window.get("title"))
            .and_then(|item| item.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&name)
            .to_owned();
        let width = read_window_dimension(window_table, "width", 1280)?;
        let height = read_window_dimension(window_table, "height", 720)?;
        let mode = window_table
            .and_then(|window| window.get("mode"))
            .and_then(|item| item.as_str())
            .map(|value| {
                ProjectWindowMode::parse(value)
                    .ok_or_else(|| format!("Unsupported display.window.mode: {value}"))
            })
            .transpose()?
            .unwrap_or_default();
        let vsync = window_table
            .and_then(|window| window.get("vsync"))
            .and_then(|item| item.as_bool())
            .unwrap_or(true);

        Ok(Self {
            name,
            main_scene,
            window: ProjectWindowSettings {
                title,
                width,
                height,
                mode,
                vsync,
            },
        })
    }

    pub fn game_window(&self, visible: bool) -> Window {
        Window {
            title: self.window.title.clone(),
            resolution: WindowResolution::new(self.window.width, self.window.height),
            mode: self.window.mode.to_bevy(),
            present_mode: if self.window.vsync {
                PresentMode::AutoVsync
            } else {
                PresentMode::AutoNoVsync
            },
            visible,
            ..default()
        }
    }
}

fn read_window_dimension(
    window: Option<&dyn toml_edit::TableLike>,
    key: &str,
    fallback: u32,
) -> Result<u32, String> {
    let Some(value) = window.and_then(|window| window.get(key)) else {
        return Ok(fallback);
    };
    let value = value
        .as_integer()
        .ok_or_else(|| format!("display.window.{key} must be an integer"))?;
    let value =
        u32::try_from(value).map_err(|_| format!("display.window.{key} must be positive"))?;
    if !(240..=16384).contains(&value) {
        return Err(format!(
            "display.window.{key} must be between 240 and 16384"
        ));
    }
    Ok(value)
}

/// 引擎挂载为 `res://` 的游戏项目根目录。
///
/// 它是项目文件访问的安全边界，不等同于进程当前工作目录。
#[derive(Resource, Debug, Clone)]
pub struct ProjectRoot {
    pub root: PathBuf,
}

impl ProjectRoot {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(root)
        };
        Self { root }
    }

    pub fn resolve(&self, relative: &str) -> Option<PathBuf> {
        // 这里只接受普通相对路径段；绝对路径、盘符和 `..` 都必须被拒绝。
        let relative = Path::new(relative);
        if relative
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
        {
            Some(self.root.join(relative))
        } else {
            None
        }
    }

    pub fn resolve_existing(&self, relative: &str) -> Option<PathBuf> {
        // 读取现有路径时再次规范化并验证前缀，防止链接或路径别名越出项目根。
        let resolved = fs::canonicalize(self.resolve(relative)?).ok()?;
        let root = fs::canonicalize(&self.root).ok()?;
        resolved.starts_with(root).then_some(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn project_root_rejects_paths_that_can_escape() {
        let project = ProjectRoot::new(PathBuf::from("project"));

        assert_eq!(
            project.resolve("assets/player.png"),
            Some(project.root.join("assets/player.png"))
        );
        assert_eq!(project.resolve("../outside.txt"), None);
        assert_eq!(project.resolve("/absolute/path"), None);
        #[cfg(target_os = "windows")]
        assert_eq!(project.resolve("C:\\outside.txt"), None);
    }

    #[test]
    fn project_settings_load_window_configuration() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("revy-project-settings-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("project.toml"),
            r#"[project]
name = "Settings Test"
main_scene = "assets/scenes/main.bsn"

[display.window]
title = "Configured Window"
width = 1600
height = 900
mode = "borderless"
vsync = false
"#,
        )
        .unwrap();

        let settings = ProjectSettings::load(&root).unwrap();
        assert_eq!(settings.name, "Settings Test");
        assert_eq!(settings.main_scene, "assets/scenes/main.bsn");
        assert_eq!(settings.window.title, "Configured Window");
        assert_eq!(settings.window.width, 1600);
        assert_eq!(settings.window.height, 900);
        assert_eq!(settings.window.mode, ProjectWindowMode::Borderless);
        assert!(!settings.window.vsync);

        fs::remove_dir_all(root).unwrap();
    }
}
