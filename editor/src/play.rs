//! 编辑器控制的游戏构建与运行生命周期。
//!
//! 游戏运行在独立子进程中。编辑器只保存场景、生成隔离构建副本、启动进程、
//! 收集日志并在 Windows 下嵌入游戏窗口，不与运行时共享同一个 ECS World。

use std::{
    collections::{HashSet, hash_map::DefaultHasher},
    fs,
    hash::{Hash, Hasher},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use arisna_engine::{ProjectRoot, SceneSystemBinding, SceneSystemSchedule, load_scene_file};
#[cfg(windows)]
use bevy::ui::UiGlobalTransform;
#[cfg(not(windows))]
use bevy::{input_focus::InputFocus, text::EditableText};
use bevy::{
    prelude::*,
    ui::InteractionDisabled,
    ui_widgets::Activate,
    window::{PrimaryWindow, RawHandleWrapper},
};
#[cfg(windows)]
use raw_window_handle::RawWindowHandle;
use toml_edit::{DocumentMut, Item};

use crate::{
    filesystem::{FileSystemState, RefreshFileSystem},
    output::{OutputLevel, OutputLog, OutputSender},
    rust_components::{RustEntityScriptFunction, discover_entity_scripts},
    scene::{SceneDocument, SceneSaveQuery, persist_last_scene, request_save, save_scene},
    ui::theme,
    undo::SceneHistory,
    workspace::EditorViewMode,
};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GameRunAction {
    #[default]
    PlayProject,
    PlayCurrent,
    Pause,
    Stop,
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct GameRunButton(pub GameRunAction);

#[derive(Component, Clone, Copy, Default)]
pub struct GameRunStatusLabel;

#[derive(Component, Clone, Copy, Default)]
pub struct GameViewportPane;

#[derive(Component, Clone, Copy, Default)]
pub struct GameViewportStatusLabel;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GameRunPhase {
    #[default]
    Stopped,
    Building,
    Launching,
    Running,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaunchTarget {
    Project,
    Current,
}

#[derive(Resource, Debug, Default)]
pub struct GameRunner {
    // 该资源是运行状态机的唯一事实来源。UI 按钮和状态文字只能读取/发送请求，
    // 不应绕过它直接持有或终止 Child 进程。
    phase: GameRunPhase,
    target: Option<LaunchTarget>,
    pending: Option<LaunchTarget>,
    child: Option<Child>,
    executable: Option<PathBuf>,
    scene_relative: Option<PathBuf>,
    control_path: Option<PathBuf>,
    build_fingerprint: Option<u64>,
    build_fingerprint_path: Option<PathBuf>,
    build_source_root: Option<PathBuf>,
    launch_ready_at: Option<Instant>,
}

impl Drop for GameRunner {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(path) = self.control_path.as_deref() {
            let _ = fs::remove_file(path);
        }
    }
}

#[derive(Message, Clone, Copy, Debug)]
struct GameRunRequest(GameRunAction);

pub struct GameRunnerPlugin;

#[cfg(windows)]
#[derive(Resource, Debug, Default)]
struct EmbeddedGameWindow {
    hwnd: isize,
    process_id: u32,
    ready_at: Option<Instant>,
    visible: bool,
    rect: Option<(i32, i32, i32, i32)>,
}

#[cfg(windows)]
#[derive(Resource, Debug, Default)]
struct GameShortcutState([bool; 4]);

#[cfg(windows)]
const PLAYER_LAUNCH_DELAY: Duration = Duration::from_millis(50);
#[cfg(windows)]
const EMBED_SURFACE_READY_DELAY: Duration = Duration::from_millis(1_500);

impl Plugin for GameRunnerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameRunner>()
            .add_message::<GameRunRequest>()
            .add_observer(handle_toolbar_action)
            .add_systems(
                Update,
                (
                    handle_game_shortcuts,
                    handle_run_requests,
                    resume_pending_launch,
                    poll_game_process,
                    sync_game_run_chrome,
                )
                    .chain(),
            );

        #[cfg(windows)]
        app.init_resource::<EmbeddedGameWindow>()
            .init_resource::<GameShortcutState>()
            .add_systems(Last, sync_embedded_game_window);
    }
}

fn handle_toolbar_action(
    activate: On<Activate>,
    buttons: Query<&GameRunButton>,
    mut requests: MessageWriter<GameRunRequest>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        requests.write(GameRunRequest(button.0));
    }
}

#[cfg(not(windows))]
fn handle_game_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    mut requests: MessageWriter<GameRunRequest>,
) {
    if focus
        .as_deref()
        .and_then(InputFocus::get)
        .is_some_and(|entity| editable_text.get(entity).is_ok())
    {
        return;
    }
    let action = if keyboard.just_pressed(KeyCode::F5) {
        Some(GameRunAction::PlayProject)
    } else if keyboard.just_pressed(KeyCode::F6) {
        Some(GameRunAction::PlayCurrent)
    } else if keyboard.just_pressed(KeyCode::F7) {
        Some(GameRunAction::Pause)
    } else if keyboard.just_pressed(KeyCode::F8) {
        Some(GameRunAction::Stop)
    } else {
        None
    };
    if let Some(action) = action {
        requests.write(GameRunRequest(action));
    }
}

#[cfg(windows)]
fn handle_game_shortcuts(
    primary_window: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    mut state: ResMut<GameShortcutState>,
    mut requests: MessageWriter<GameRunRequest>,
) {
    use windows_sys::Win32::UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F5, VK_F6, VK_F7, VK_F8},
        WindowsAndMessaging::{GetForegroundWindow, IsChild},
    };

    let editor_hwnd = primary_window
        .single()
        .ok()
        .and_then(editor_parent_hwnd)
        .map(|hwnd| hwnd as _);
    let editor_is_foreground = editor_hwnd.is_some_and(|editor| {
        let foreground = unsafe { GetForegroundWindow() };
        foreground == editor || unsafe { IsChild(editor, foreground) } != 0
    });
    if !editor_is_foreground {
        state.0.fill(false);
        return;
    }

    let pressed =
        [VK_F5, VK_F6, VK_F7, VK_F8].map(|key| unsafe { GetAsyncKeyState(key as i32) } < 0);
    let action = shortcut_action(pressed, state.0);
    state.0 = pressed;
    if let Some(action) = action {
        requests.write(GameRunRequest(action));
    }
}

fn shortcut_action(pressed: [bool; 4], previous: [bool; 4]) -> Option<GameRunAction> {
    [
        GameRunAction::PlayProject,
        GameRunAction::PlayCurrent,
        GameRunAction::Pause,
        GameRunAction::Stop,
    ]
    .into_iter()
    .enumerate()
    .find_map(|(index, action)| (pressed[index] && !previous[index]).then_some(action))
}

#[allow(clippy::too_many_arguments)]
fn handle_run_requests(
    mut requests: MessageReader<GameRunRequest>,
    project: Res<ProjectRoot>,
    mut runner: ResMut<GameRunner>,
    mut document: ResMut<SceneDocument>,
    objects: Query<SceneSaveQuery>,
    mut filesystem: ResMut<FileSystemState>,
    mut refresh: MessageWriter<RefreshFileSystem>,
    mut history: ResMut<SceneHistory>,
    mut output: ResMut<OutputLog>,
    mut view: ResMut<EditorViewMode>,
) {
    for request in requests.read() {
        match request.0 {
            GameRunAction::Stop => stop_game(&mut runner, &mut output),
            GameRunAction::Pause => toggle_pause(&mut runner, &mut output),
            GameRunAction::PlayProject | GameRunAction::PlayCurrent => {
                *view = EditorViewMode::Game;
                let target = if request.0 == GameRunAction::PlayProject {
                    LaunchTarget::Project
                } else {
                    LaunchTarget::Current
                };
                restart_active_game(&mut runner, &mut output);

                if document.open && document.dirty {
                    if document.path.is_none() {
                        request_save(&project, &mut document);
                        runner.pending = Some(target);
                        output.push(
                            OutputLevel::Info,
                            "Save the current scene to continue running",
                        );
                        continue;
                    }
                    match save_scene(&mut document, &objects) {
                        Ok(()) => {
                            history.request_mark_saved();
                            persist_last_scene(&project, document.path.as_deref());
                            refresh.write(RefreshFileSystem);
                            filesystem.status = "Scene saved before running".into();
                            filesystem.revision = filesystem.revision.wrapping_add(1);
                        }
                        Err(error) => {
                            output.push(OutputLevel::Error, error);
                            continue;
                        }
                    }
                }

                if let Err(error) =
                    launch_target(target, &project, &document, &mut runner, &mut output)
                {
                    output.push(OutputLevel::Error, error);
                }
            }
        }
    }
}

fn resume_pending_launch(
    project: Res<ProjectRoot>,
    document: Res<SceneDocument>,
    mut runner: ResMut<GameRunner>,
    mut output: ResMut<OutputLog>,
) {
    let Some(target) = runner.pending else {
        return;
    };
    if document.save_dialog_open() {
        return;
    }
    runner.pending = None;
    if document.path.is_none() || document.dirty {
        output.push(
            OutputLevel::Warning,
            "Run cancelled because the scene was not saved",
        );
        return;
    }
    if let Err(error) = launch_target(target, &project, &document, &mut runner, &mut output) {
        output.push(OutputLevel::Error, error);
    }
}

fn launch_target(
    target: LaunchTarget,
    project: &ProjectRoot,
    document: &SceneDocument,
    runner: &mut GameRunner,
    output: &mut OutputLog,
) -> Result<(), String> {
    let scene_relative = match target {
        LaunchTarget::Current => current_scene_relative(project, document)?,
        LaunchTarget::Project => match read_main_scene(&project.root)? {
            Some(path) => path,
            None => {
                let current = current_scene_relative(project, document)?;
                write_main_scene(&project.root, &current)?;
                output.push(
                    OutputLevel::Info,
                    format!(
                        "Set res://{} as the project main scene",
                        display_relative(&current)
                    ),
                );
                current
            }
        },
    };

    let scene_text = display_relative(&scene_relative);
    let Some(scene_path) = project
        .resolve_existing(&scene_text)
        .filter(|path| path.is_file())
    else {
        return Err(format!("Scene not found: res://{scene_text}"));
    };
    load_scene_file(&scene_path)
        .map_err(|error| format!("Cannot run res://{scene_text}: {error}"))?;

    start_build(project, target, scene_relative, runner, output)
}

fn current_scene_relative(
    project: &ProjectRoot,
    document: &SceneDocument,
) -> Result<PathBuf, String> {
    let path = document
        .path
        .as_deref()
        .ok_or_else(|| "Create or open a scene before running the current scene".to_string())?;
    let root = fs::canonicalize(&project.root).unwrap_or_else(|_| project.root.clone());
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.strip_prefix(&root)
        .map(Path::to_path_buf)
        .map_err(|_| "The scene must be saved inside the current project".to_string())
}

fn start_build(
    project: &ProjectRoot,
    target: LaunchTarget,
    scene_relative: PathBuf,
    runner: &mut GameRunner,
    output: &mut OutputLog,
) -> Result<(), String> {
    let source_manifest = project.root.join("Cargo.toml");
    if !source_manifest.is_file() {
        return Err(format!(
            "Project manifest not found: {}",
            source_manifest.display()
        ));
    }
    let executable_name = project_executable_name(&source_manifest)?;
    // 编辑器运行不会篡改用户项目入口：生成副本和全部 Cargo 输出都隔离在
    // Revy 的 target 中，真实项目只保留用户主动保存的源文件和场景。
    let workspace_root = crate::paths::workspace_root();
    let build_root = workspace_root.join("target");
    fs::create_dir_all(&build_root)
        .map_err(|error| format!("Could not create the game build folder: {error}"))?;
    let generated =
        prepare_generated_project_for_scene(&project.root, &build_root, Some(&scene_relative))?;
    let manifest = generated.project_root.join("Cargo.toml");
    let child_dirs = prepare_child_process_directories(&build_root)?;
    let executable = build_root.join("debug").join(format!(
        "{}{}",
        executable_name,
        std::env::consts::EXE_SUFFIX
    ));
    let control_path = generated.project_root.join(".arisna/run-control");
    if let Some(parent) = control_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create game control folder: {error}"))?;
    }
    fs::write(&control_path, "running\n")
        .map_err(|error| format!("Could not initialize game controls: {error}"))?;

    let build_fingerprint = project_build_fingerprint(&project.root)?;
    let build_fingerprint_path = generated.project_root.join(".arisna/build-fingerprint");
    runner.target = Some(target);
    runner.executable = Some(executable.clone());
    runner.scene_relative = Some(scene_relative.clone());
    runner.control_path = Some(control_path);
    runner.build_source_root = Some(project.root.clone());

    if executable.is_file()
        && read_build_fingerprint(&build_fingerprint_path) == Some(build_fingerprint)
    {
        output.push(
            OutputLevel::Info,
            format!(
                "Using cached game build for res://{}",
                display_relative(&scene_relative)
            ),
        );
        runner.phase = GameRunPhase::Launching;
        runner.launch_ready_at = Some(Instant::now() + PLAYER_LAUNCH_DELAY);
        return Ok(());
    }

    let mut command = Command::new("cargo");
    let process_manifest = child_process_path(&manifest);
    let process_project_root = child_process_path(&project.root);
    let process_build_root = child_process_path(&build_root);
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(&process_manifest)
        .current_dir(&process_project_root)
        .env("CARGO_TARGET_DIR", &process_build_root)
        .env("CARGO_HOME", child_process_path(&child_dirs.cargo_home))
        .env("TEMP", child_process_path(&child_dirs.temp))
        .env("TMP", child_process_path(&child_dirs.temp))
        .env("LOCALAPPDATA", child_process_path(&child_dirs.data))
        .env("APPDATA", child_process_path(&child_dirs.data))
        .env("CARGO_NET_OFFLINE", "false")
        .env("CARGO_INCREMENTAL", "1")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_TERM_COLOR", "never")
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_hidden_process(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start Cargo: {error}"))?;
    attach_process_output(&mut child, output.sender());

    output.push(
        OutputLevel::Info,
        format!(
            "Building project for res://{}",
            display_relative(&scene_relative)
        ),
    );
    runner.phase = GameRunPhase::Building;
    runner.child = Some(child);
    runner.build_fingerprint = Some(build_fingerprint);
    runner.build_fingerprint_path = Some(build_fingerprint_path);
    Ok(())
}

fn poll_game_process(
    mut runner: ResMut<GameRunner>,
    output: Res<OutputLog>,
    primary_window: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    viewport: Query<(&ComputedNode, &UiGlobalTransform), With<GameViewportPane>>,
) {
    if runner.phase == GameRunPhase::Launching {
        if runner
            .launch_ready_at
            .is_some_and(|ready_at| Instant::now() < ready_at)
        {
            return;
        }
        if let Err(error) = launch_ready_player(&mut runner, &output, &primary_window, &viewport) {
            output.sender().push(OutputLevel::Error, error);
            reset_runner(&mut runner);
        }
        return;
    }
    let Some(child) = runner.child.as_mut() else {
        return;
    };
    let status = match child.try_wait() {
        Ok(Some(status)) => status,
        Ok(None) => return,
        Err(error) => {
            output.sender().push(
                OutputLevel::Error,
                format!("Could not inspect game process: {error}"),
            );
            reset_runner(&mut runner);
            return;
        }
    };
    runner.child.take();

    match runner.phase {
        GameRunPhase::Building if status.success() => {
            refresh_build_fingerprint(&mut runner);
            persist_build_fingerprint(&runner);
            runner.phase = GameRunPhase::Launching;
            runner.launch_ready_at = Some(Instant::now() + PLAYER_LAUNCH_DELAY);
        }
        GameRunPhase::Building => {
            output.sender().push(
                OutputLevel::Error,
                format!("Project build failed with {status}"),
            );
            reset_runner(&mut runner);
        }
        GameRunPhase::Running | GameRunPhase::Paused => {
            let level = if status.success() {
                OutputLevel::Info
            } else {
                OutputLevel::Error
            };
            output
                .sender()
                .push(level, format!("Game process exited with {status}"));
            reset_runner(&mut runner);
        }
        GameRunPhase::Stopped | GameRunPhase::Launching => reset_runner(&mut runner),
    }
}

fn launch_ready_player(
    runner: &mut GameRunner,
    output: &OutputLog,
    primary_window: &Query<&RawHandleWrapper, With<PrimaryWindow>>,
    viewport: &Query<(&ComputedNode, &UiGlobalTransform), With<GameViewportPane>>,
) -> Result<(), String> {
    let embed_parent = primary_window.single().ok().and_then(editor_parent_hwnd);
    #[cfg(windows)]
    if embed_parent.is_none() {
        return Err("Could not obtain the editor window handle for the Game workspace".into());
    }
    #[cfg(windows)]
    let embed_rect = Some(
        game_viewport_rect(viewport)
            .ok_or_else(|| "Could not obtain the Game workspace bounds".to_string())?,
    );
    #[cfg(not(windows))]
    let embed_rect = None;
    start_player(runner, output.sender(), embed_parent, embed_rect)
}

fn start_player(
    runner: &mut GameRunner,
    sender: OutputSender,
    embed_parent: Option<usize>,
    embed_rect: Option<(i32, i32, i32, i32)>,
) -> Result<(), String> {
    let executable = runner
        .executable
        .as_deref()
        .ok_or_else(|| "The game executable path was lost".to_string())?;
    if !executable.is_file() {
        return Err(format!(
            "Game executable not found: {}",
            executable.display()
        ));
    }
    let scene = runner
        .scene_relative
        .as_deref()
        .ok_or_else(|| "The selected scene path was lost".to_string())?;
    let control = runner
        .control_path
        .as_deref()
        .ok_or_else(|| "The game control path was lost".to_string())?;

    let process_executable = child_process_path(executable);
    let process_control = child_process_path(control);
    let process_project_root = child_process_path(
        control
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new(".")),
    );
    let build_root = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "The game build folder path was lost".to_string())?;
    let child_dirs = prepare_child_process_directories(build_root)?;
    let mut command = Command::new(process_executable);
    command
        .arg("--scene")
        .arg(scene)
        .arg("--control")
        .arg(process_control)
        .current_dir(process_project_root)
        .env("CARGO_HOME", child_process_path(&child_dirs.cargo_home))
        .env("TEMP", child_process_path(&child_dirs.temp))
        .env("TMP", child_process_path(&child_dirs.temp))
        .env("LOCALAPPDATA", child_process_path(&child_dirs.data))
        .env("APPDATA", child_process_path(&child_dirs.data))
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(project_root) = runner.build_source_root.as_deref() {
        let project_root = child_process_path(project_root);
        command
            .env("REVY_PROJECT_ROOT", &project_root)
            .env("ARISNA_PROJECT_ROOT", &project_root);
    }
    if let Some(parent) = embed_parent {
        command
            .env("REVY_EMBEDDED", "1")
            .env("REVY_EMBED_PARENT_HWND", parent.to_string())
            .env("ARISNA_EMBEDDED", "1")
            .env("ARISNA_EMBED_PARENT_HWND", parent.to_string());
        if let Some((x, y, width, height)) = embed_rect {
            command
                .env("REVY_EMBED_X", x.to_string())
                .env("REVY_EMBED_Y", y.to_string())
                .env("REVY_EMBED_WIDTH", width.to_string())
                .env("REVY_EMBED_HEIGHT", height.to_string())
                .env("ARISNA_EMBED_X", x.to_string())
                .env("ARISNA_EMBED_Y", y.to_string())
                .env("ARISNA_EMBED_WIDTH", width.to_string())
                .env("ARISNA_EMBED_HEIGHT", height.to_string());
        }
    }
    configure_hidden_process(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not launch the game: {error}"))?;
    attach_process_output(&mut child, sender.clone());
    sender.push(
        OutputLevel::Info,
        format!("Started game process {}", child.id()),
    );
    runner.child = Some(child);
    runner.phase = GameRunPhase::Running;
    Ok(())
}

#[cfg(windows)]
fn editor_parent_hwnd(handle: &RawHandleWrapper) -> Option<usize> {
    let RawWindowHandle::Win32(handle) = handle.get_window_handle() else {
        return None;
    };
    Some(handle.hwnd.get() as usize)
}

#[cfg(not(windows))]
fn editor_parent_hwnd(_: &RawHandleWrapper) -> Option<usize> {
    None
}

fn toggle_pause(runner: &mut GameRunner, output: &mut OutputLog) {
    let next = match runner.phase {
        GameRunPhase::Running => GameRunPhase::Paused,
        GameRunPhase::Paused => GameRunPhase::Running,
        _ => return,
    };
    let Some(path) = runner.control_path.as_deref() else {
        return;
    };
    let value = if next == GameRunPhase::Paused {
        "paused\n"
    } else {
        "running\n"
    };
    match fs::write(path, value) {
        Ok(()) => {
            runner.phase = next;
            output.push(
                OutputLevel::Info,
                if next == GameRunPhase::Paused {
                    "Game paused"
                } else {
                    "Game resumed"
                },
            );
        }
        Err(error) => output.push(
            OutputLevel::Error,
            format!("Could not update game pause state: {error}"),
        ),
    }
}

fn restart_active_game(runner: &mut GameRunner, output: &mut OutputLog) {
    if runner.phase != GameRunPhase::Stopped || runner.pending.is_some() {
        output.push(OutputLevel::Info, "Restarting the game");
        stop_game(runner, output);
    }
}

fn stop_game(runner: &mut GameRunner, output: &mut OutputLog) {
    if runner.phase == GameRunPhase::Stopped && runner.pending.is_none() {
        return;
    }
    runner.pending = None;
    if let Some(child) = runner.child.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    output.push(OutputLevel::Info, "Game stopped");
    reset_runner(runner);
}

fn sync_game_run_chrome(
    runner: Res<GameRunner>,
    mut buttons: Query<(
        Entity,
        &GameRunButton,
        Has<InteractionDisabled>,
        &mut BorderColor,
    )>,
    mut labels: Query<&mut Text, With<GameRunStatusLabel>>,
    mut viewport_labels: Query<
        &mut Text,
        (With<GameViewportStatusLabel>, Without<GameRunStatusLabel>),
    >,
    mut commands: Commands,
) {
    if !runner.is_changed() {
        return;
    }
    for (entity, button, disabled, mut border) in &mut buttons {
        let should_disable = match button.0 {
            GameRunAction::PlayProject | GameRunAction::PlayCurrent => false,
            GameRunAction::Pause => {
                !matches!(runner.phase, GameRunPhase::Running | GameRunPhase::Paused)
            }
            GameRunAction::Stop => {
                runner.phase == GameRunPhase::Stopped && runner.pending.is_none()
            }
        };
        if should_disable && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !should_disable && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }

        let highlighted = match button.0 {
            GameRunAction::PlayProject => {
                runner.target == Some(LaunchTarget::Project)
                    && runner.phase != GameRunPhase::Stopped
            }
            GameRunAction::PlayCurrent => {
                runner.target == Some(LaunchTarget::Current)
                    && runner.phase != GameRunPhase::Stopped
            }
            GameRunAction::Pause => runner.phase == GameRunPhase::Paused,
            GameRunAction::Stop => false,
        };
        *border = BorderColor::all(if highlighted {
            if runner.phase == GameRunPhase::Paused {
                theme::warning()
            } else {
                theme::play()
            }
        } else {
            Color::NONE
        });
    }

    let scene_name = runner
        .scene_relative
        .as_deref()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    let status = if runner.pending.is_some() {
        "Waiting for save".to_string()
    } else {
        match runner.phase {
            GameRunPhase::Stopped => "Ready".to_string(),
            GameRunPhase::Building => run_status("Building", scene_name),
            GameRunPhase::Launching => run_status("Launching", scene_name),
            GameRunPhase::Running => run_status("Running", scene_name),
            GameRunPhase::Paused => run_status("Paused", scene_name),
        }
    };
    for mut label in &mut labels {
        label.0.clone_from(&status);
    }

    let viewport_status = if runner.pending.is_some() {
        "Save the scene to continue".to_string()
    } else {
        match runner.phase {
            GameRunPhase::Stopped => "Run a scene to start the game".to_string(),
            GameRunPhase::Building => run_status("Building", scene_name),
            GameRunPhase::Launching => run_status("Launching", scene_name),
            GameRunPhase::Running => run_status("Starting", scene_name),
            GameRunPhase::Paused => run_status("Paused", scene_name),
        }
    };
    for mut label in &mut viewport_labels {
        label.0.clone_from(&viewport_status);
    }
}

fn run_status(phase: &str, scene_name: Option<&str>) -> String {
    scene_name.map_or_else(|| phase.to_string(), |scene| format!("{phase}: {scene}"))
}

#[cfg(windows)]
fn sync_embedded_game_window(
    runner: Res<GameRunner>,
    view: Res<EditorViewMode>,
    primary_window: Query<(&Window, &RawHandleWrapper), With<PrimaryWindow>>,
    viewport: Query<(&ComputedNode, &UiGlobalTransform), With<GameViewportPane>>,
    mut embedded: ResMut<EmbeddedGameWindow>,
    output: Res<OutputLog>,
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetParent, IsWindow, SW_HIDE, SW_SHOW, ShowWindow,
    };

    let Ok((_, handle)) = primary_window.single() else {
        return;
    };
    let Some(parent_hwnd) = editor_parent_hwnd(handle).map(|hwnd| hwnd as _) else {
        return;
    };

    let player_pid = runner
        .child
        .as_ref()
        .filter(|_| matches!(runner.phase, GameRunPhase::Running | GameRunPhase::Paused))
        .map(Child::id);
    let Some(player_pid) = player_pid else {
        if embedded.hwnd != 0 {
            unsafe {
                ShowWindow(embedded.hwnd as _, SW_HIDE);
            }
        }
        *embedded = EmbeddedGameWindow::default();
        return;
    };

    if embedded.hwnd != 0
        && (embedded.process_id != player_pid
            || unsafe { IsWindow(embedded.hwnd as _) } == 0
            || unsafe { GetParent(embedded.hwnd as _) } != parent_hwnd)
    {
        *embedded = EmbeddedGameWindow::default();
    }

    if embedded.hwnd == 0 {
        let Some(hwnd) = find_process_child_window(parent_hwnd, player_pid) else {
            return;
        };
        embedded.hwnd = hwnd as isize;
        embedded.process_id = player_pid;
        embedded.ready_at = Some(Instant::now() + EMBED_SURFACE_READY_DELAY);
        embedded.visible = false;
        embedded.rect = game_viewport_rect(&viewport);
        output.sender().push(
            OutputLevel::Info,
            format!("Attached game process {player_pid} to the Game workspace"),
        );
    }

    let should_show = *view == EditorViewMode::Game;
    if !should_show {
        if embedded.visible {
            unsafe {
                ShowWindow(embedded.hwnd as _, SW_HIDE);
            }
            embedded.visible = false;
        }
        return;
    }

    if embedded
        .ready_at
        .is_some_and(|ready_at| Instant::now() < ready_at)
    {
        return;
    }
    embedded.ready_at = None;

    let Ok((node, transform)) = viewport.single() else {
        return;
    };
    let size = node.size().round().as_ivec2();
    if size.x < 2 || size.y < 2 {
        return;
    }
    let center = transform.affine().translation.round().as_ivec2();
    let position = center - size / 2;
    let rect = (position.x, position.y, size.x, size.y);
    if embedded.rect != Some(rect) {
        resize_embedded_window(embedded.hwnd as _, rect);
        embedded.rect = Some(rect);
    }
    if !embedded.visible {
        unsafe {
            ShowWindow(embedded.hwnd as _, SW_SHOW);
        }
        embedded.visible = true;
    }
}

#[cfg(windows)]
fn game_viewport_rect(
    viewport: &Query<(&ComputedNode, &UiGlobalTransform), With<GameViewportPane>>,
) -> Option<(i32, i32, i32, i32)> {
    let (node, transform) = viewport.single().ok()?;
    let size = node.size().round().as_ivec2();
    if size.x < 2 || size.y < 2 {
        return None;
    }
    let center = transform.affine().translation.round().as_ivec2();
    let position = center - size / 2;
    Some((position.x, position.y, size.x, size.y))
}

#[cfg(windows)]
fn find_process_child_window(
    parent: windows_sys::Win32::Foundation::HWND,
    process_id: u32,
) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{EnumChildWindows, GetWindowThreadProcessId},
    };

    struct Search {
        process_id: u32,
        hwnd: HWND,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> i32 {
        let search = unsafe { &mut *(lparam as *mut Search) };
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut owner);
        }
        if owner == search.process_id {
            search.hwnd = hwnd;
            0
        } else {
            1
        }
    }

    let mut search = Search {
        process_id,
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumChildWindows(parent, Some(visit), &mut search as *mut Search as LPARAM);
    }
    (!search.hwnd.is_null()).then_some(search.hwnd)
}

#[cfg(windows)]
fn resize_embedded_window(
    hwnd: windows_sys::Win32::Foundation::HWND,
    (x, y, width, height): (i32, i32, i32, i32),
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, SetWindowPos,
    };

    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

fn reset_runner(runner: &mut GameRunner) {
    runner.phase = GameRunPhase::Stopped;
    runner.target = None;
    runner.pending = None;
    runner.child = None;
    runner.executable = None;
    runner.scene_relative = None;
    runner.build_fingerprint = None;
    runner.build_fingerprint_path = None;
    runner.build_source_root = None;
    runner.launch_ready_at = None;
    if let Some(path) = runner.control_path.take() {
        let _ = fs::remove_file(path);
    }
}

#[derive(Debug)]
struct GeneratedProject {
    project_root: PathBuf,
}

#[cfg(test)]
fn prepare_generated_project(
    source_root: &Path,
    build_root: &Path,
) -> Result<GeneratedProject, String> {
    prepare_generated_project_for_scene(source_root, build_root, None)
}

fn prepare_generated_project_for_scene(
    source_root: &Path,
    build_root: &Path,
    scene_relative: Option<&Path>,
) -> Result<GeneratedProject, String> {
    // 使用真实项目根路径的哈希隔离多个游戏项目；相同项目重复运行可复用缓存。
    let source_root = fs::canonicalize(source_root).unwrap_or_else(|_| source_root.to_path_buf());
    let build_root = fs::canonicalize(build_root).unwrap_or_else(|_| build_root.to_path_buf());
    let mut hasher = DefaultHasher::new();
    source_root
        .to_string_lossy()
        .to_lowercase()
        .hash(&mut hasher);
    let generated_root = build_root
        .join("revy-generated")
        .join(format!("{:016x}", hasher.finish()));
    let project_root = generated_root.join("project");
    if !project_root.starts_with(&build_root) {
        return Err("Generated project escaped the Revy target directory".into());
    }
    fs::create_dir_all(&project_root)
        .map_err(|error| format!("Could not create generated project folder: {error}"))?;

    for directory in ["src", "vendor", ".cargo"] {
        sync_generated_directory(&source_root.join(directory), &project_root.join(directory))?;
    }
    for file in ["Cargo.toml", "Cargo.lock", "build.rs", "project.toml"] {
        sync_generated_file(&source_root.join(file), &project_root.join(file))?;
    }

    let manifest = project_root.join("Cargo.toml");
    rewrite_generated_manifest_paths(&manifest, &source_root)?;
    isolate_generated_manifest_workspace(&manifest)?;

    let assets = source_root.join("assets");
    if assets.is_dir() {
        replace_directory_link(&project_root.join("assets"), &assets)?;
    }
    let protobuf = source_root
        .parent()
        .map(|parent| parent.join("protobuf"))
        .filter(|path| path.is_dir());
    if let Some(protobuf) = protobuf {
        replace_directory_link(&generated_root.join("protobuf"), &protobuf)?;
    }

    let scripts = discover_entity_scripts(&source_root)?;
    let systems = crate::rust_components::discover_systems(&source_root)?;
    let bindings = scene_relative
        .map(|relative| {
            let path = source_root.join(relative);
            load_scene_file(&path).map(|scene| {
                scene
                    .entities
                    .into_iter()
                    .flat_map(|node| node.systems)
                    .collect::<Vec<_>>()
            })
        })
        .transpose()?;
    let entry = generated_binary_source_path(&manifest)?;
    let engine_crate = project_engine_crate_name(&manifest)?;
    // 只修改 target 内的生成入口。Entity Script/System 仍是编译后的 Rust，
    // 注入内容仅负责把 Inspector 保存的稳定路径绑定到真实函数。
    inject_runtime_registrations(
        &entry,
        engine_crate,
        &scripts,
        &systems,
        bindings.as_deref(),
    )?;
    Ok(GeneratedProject { project_root })
}

fn sync_generated_directory(source: &Path, destination: &Path) -> Result<(), String> {
    remove_generated_path(destination)?;
    if !source.is_dir() {
        return Ok(());
    }
    copy_directory(source, destination)
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("Could not create {}: {error}", destination.display()))?;
    let mut entries = fs::read_dir(source)
        .map_err(|error| format!("Could not inspect {}: {error}", source.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Could not inspect {}: {error}", source.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", source_path.display()))?
            .is_dir()
        {
            copy_directory(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).map_err(|error| {
                format!(
                    "Could not copy {} to {}: {error}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn sync_generated_file(source: &Path, destination: &Path) -> Result<(), String> {
    if source.is_file() {
        fs::copy(source, destination).map_err(|error| {
            format!(
                "Could not copy {} to {}: {error}",
                source.display(),
                destination.display()
            )
        })?;
    } else if destination.exists() {
        fs::remove_file(destination)
            .map_err(|error| format!("Could not remove {}: {error}", destination.display()))?;
    }
    Ok(())
}

fn remove_generated_path(path: &Path) -> Result<(), String> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        fs::remove_dir(path)
            .or_else(|_| fs::remove_file(path))
            .map_err(|error| format!("Could not remove link {}: {error}", path.display()))
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|error| format!("Could not refresh {}: {error}", path.display()))
    } else {
        fs::remove_file(path)
            .map_err(|error| format!("Could not refresh {}: {error}", path.display()))
    }
}

fn replace_directory_link(link: &Path, target: &Path) -> Result<(), String> {
    remove_generated_path(link)?;
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    }
    create_directory_link(link, target).map_err(|error| {
        format!(
            "Could not link {} to {}: {error}",
            link.display(),
            target.display()
        )
    })
}

#[cfg(windows)]
fn create_directory_link(link: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::symlink_dir;

    if symlink_dir(target, link).is_ok() {
        return Ok(());
    }
    let script = "& { param([string]$Link, [string]$Target) New-Item -ItemType Junction -Path $Link -Target $Target -Force | Out-Null }";
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .arg(link.to_string_lossy().trim_start_matches(r"\\?\"))
        .arg(target.to_string_lossy().trim_start_matches(r"\\?\"));
    configure_hidden_process(&mut command);
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "PowerShell junction creation exited with {status}"
        )))
    }
}

#[cfg(unix)]
fn create_directory_link(link: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

fn rewrite_generated_manifest_paths(manifest: &Path, source_root: &Path) -> Result<(), String> {
    let source = fs::read_to_string(manifest)
        .map_err(|error| format!("Could not read {}: {error}", manifest.display()))?;
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines = Vec::new();
    let mut section = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.trim_matches('[').trim_matches(']').to_owned();
        }
        // Keep binary/library source paths relative to the generated copy;
        // only dependency path values need to point back to the source tree.
        if !section.contains("dependencies") && !section.starts_with("patch.") {
            lines.push(line.to_owned());
            continue;
        }
        let Some(path_start) = line.find("path = \"") else {
            lines.push(line.to_owned());
            continue;
        };
        let value_start = path_start + "path = \"".len();
        let Some(value_end) = line[value_start..]
            .find('"')
            .map(|index| value_start + index)
        else {
            lines.push(line.to_owned());
            continue;
        };
        let path = Path::new(&line[value_start..value_end]);
        if path.is_absolute() {
            lines.push(line.to_owned());
            continue;
        }
        let mut rewritten = String::with_capacity(line.len() + 64);
        rewritten.push_str(&line[..value_start]);
        rewritten.push_str(
            &child_process_path(&source_root.join(path))
                .to_string_lossy()
                .replace('\\', "/"),
        );
        rewritten.push_str(&line[value_end..]);
        lines.push(rewritten);
    }
    let mut rendered = lines.join(newline);
    if source.ends_with('\n') {
        rendered.push_str(newline);
    }
    fs::write(manifest, rendered)
        .map_err(|error| format!("Could not update {}: {error}", manifest.display()))
}

fn isolate_generated_manifest_workspace(manifest: &Path) -> Result<(), String> {
    let mut source = fs::read_to_string(manifest)
        .map_err(|error| format!("Could not read {}: {error}", manifest.display()))?;
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| format!("Could not parse {}: {error}", manifest.display()))?;
    if document.get("workspace").is_none() {
        if !source.ends_with('\n') {
            source.push('\n');
        }
        source.push_str("\n[workspace]\n");
        fs::write(manifest, source)
            .map_err(|error| format!("Could not isolate {}: {error}", manifest.display()))?;
    }
    Ok(())
}

fn generated_binary_source_path(manifest: &Path) -> Result<PathBuf, String> {
    let source = fs::read_to_string(manifest)
        .map_err(|error| format!("Could not read {}: {error}", manifest.display()))?;
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| format!("Invalid Cargo.toml: {error}"))?;
    let relative = document
        .get("bin")
        .and_then(|bins| bins.as_array_of_tables())
        .and_then(|bins| bins.iter().next())
        .and_then(|bin| bin.get("path"))
        .and_then(|path| path.as_str())
        .unwrap_or("src/main.rs");
    let path = manifest
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(relative);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("Game entry source not found: {}", path.display()))
    }
}

fn project_engine_crate_name(manifest: &Path) -> Result<&'static str, String> {
    let source = fs::read_to_string(manifest)
        .map_err(|error| format!("Could not read {}: {error}", manifest.display()))?;
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| format!("Invalid Cargo.toml: {error}"))?;
    let dependencies = document
        .get("dependencies")
        .and_then(Item::as_table_like)
        .ok_or_else(|| "Cargo.toml is missing [dependencies]".to_string())?;
    if dependencies.contains_key("revy_engine") {
        Ok("revy_engine")
    } else if dependencies.contains_key("arisna_engine") {
        Ok("arisna_engine")
    } else {
        Err("Cargo.toml is missing revy_engine (legacy arisna_engine is also accepted)".into())
    }
}

fn inject_runtime_registrations(
    entry: &Path,
    engine_crate: &str,
    scripts: &[RustEntityScriptFunction],
    systems: &[crate::rust_components::RustSystemDefinition],
    bindings: Option<&[SceneSystemBinding]>,
) -> Result<(), String> {
    let valid = scripts
        .iter()
        .filter(|script| script.valid)
        .collect::<Vec<_>>();
    let mut selected_systems = Vec::new();
    if let Some(bindings) = bindings {
        for binding in bindings {
            if binding.system_path.trim().is_empty() {
                continue;
            }
            let Some(definition) = systems
                .iter()
                .find(|definition| definition.system_path == binding.system_path)
            else {
                return Err(format!(
                    "System `{}` referenced by res://{} was not found in the project source",
                    binding.system_path, binding.script_path
                ));
            };
            if !definition.valid {
                return Err(definition.diagnostic.clone().unwrap_or_else(|| {
                    format!("System `{}` cannot be registered", definition.system_path)
                }));
            }
            if !binding.script_path.trim().is_empty()
                && binding.script_path != definition.source_path
            {
                return Err(format!(
                    "System `{}` is not declared by {}",
                    binding.system_path, binding.script_path
                ));
            }
            if !selected_systems
                .iter()
                .any(|path: &String| path == &definition.system_path)
            {
                selected_systems.push(definition.system_path.clone());
            }
        }
        for binding in bindings {
            if !binding.system_path.trim().is_empty() || binding.script_path.trim().is_empty() {
                continue;
            }
            for definition in systems
                .iter()
                .filter(|definition| definition.source_path == binding.script_path)
            {
                if !definition.valid {
                    return Err(definition.diagnostic.clone().unwrap_or_else(|| {
                        format!("System `{}` cannot be registered", definition.system_path)
                    }));
                }
                if !selected_systems
                    .iter()
                    .any(|path: &String| path == &definition.system_path)
                {
                    selected_systems.push(definition.system_path.clone());
                }
            }
        }
    }
    if valid.is_empty() && selected_systems.is_empty() {
        return Ok(());
    }
    let source = fs::read_to_string(entry)
        .map_err(|error| format!("Could not read {}: {error}", entry.display()))?;
    let insertion = source.rfind("app.run()").ok_or_else(|| {
        format!(
            "Could not inject Entity scripts: {} has no app.run()",
            entry.display()
        )
    })?;
    let mut registrations =
        String::from("    // Generated by Revy. Source project files remain unchanged.\n");
    for script in valid {
        let callable = script
            .function_path
            .split_once("::")
            .map(|(_, path)| format!("crate::{path}"))
            .unwrap_or_else(|| format!("crate::{}", script.name));
        registrations.push_str(&format!(
            "    {engine_crate}::add_revy_entity_script_fn(\n        &mut app,\n        {engine_crate}::EntityScriptLifecycle::{},\n        {:?},\n        {:?},\n        {},\n    );\n",
            script.lifecycle.label(),
            script.source_path,
            script.function_path,
            callable,
        ));
    }
    for system_path in selected_systems {
        let definition = systems
            .iter()
            .find(|definition| definition.system_path == system_path)
            .expect("selected system was validated above");
        let callable = definition
            .system_path
            .split_once("::")
            .map(|(_, path)| format!("crate::{path}"))
            .unwrap_or_else(|| format!("crate::{}", definition.name));
        let schedule = bindings
            .and_then(|bindings| {
                bindings
                    .iter()
                    .find(|binding| binding.system_path == definition.system_path)
            })
            .map(|binding| binding.schedule)
            .unwrap_or(SceneSystemSchedule::Update);
        registrations.push_str(&format!(
            "    {engine_crate}::add_revy_system(\n        &mut app,\n        {engine_crate}::SceneSystemSchedule::{},\n        {:?},\n        {},\n    );\n",
            schedule.label(), definition.system_path, callable
        ));
    }
    let mut generated = String::with_capacity(source.len() + registrations.len());
    generated.push_str(&source[..insertion]);
    generated.push_str(&registrations);
    generated.push_str(&source[insertion..]);
    fs::write(entry, generated)
        .map_err(|error| format!("Could not update {}: {error}", entry.display()))
}

#[derive(Debug)]
struct ChildProcessDirectories {
    cargo_home: PathBuf,
    temp: PathBuf,
    data: PathBuf,
}

fn prepare_child_process_directories(build_root: &Path) -> Result<ChildProcessDirectories, String> {
    // 子进程的 Cargo、TEMP 和应用数据目录全部重定向到 target，防止构建缓存
    // 和运行时临时文件散落到 C 盘用户目录。
    let directories = ChildProcessDirectories {
        cargo_home: build_root.join("cargo-home"),
        temp: build_root.join("editor-temp"),
        data: build_root.join("editor-data"),
    };
    for path in [
        &directories.cargo_home,
        &directories.temp,
        &directories.data,
    ] {
        fs::create_dir_all(path)
            .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    }
    Ok(directories)
}

fn project_build_fingerprint(project_root: &Path) -> Result<u64, String> {
    let mut hasher = DefaultHasher::new();
    let mut visited = HashSet::new();
    hash_crate_build_inputs(project_root, project_root, &mut hasher, &mut visited)?;

    // A rebuilt editor means the bundled engine SDK may have changed.
    if let Ok(executable) = std::env::current_exe()
        && let Ok(metadata) = fs::metadata(executable)
    {
        metadata.len().hash(&mut hasher);
        metadata.modified().ok().hash(&mut hasher);
    }
    Ok(hasher.finish())
}

fn hash_crate_build_inputs(
    crate_root: &Path,
    fingerprint_root: &Path,
    hasher: &mut DefaultHasher,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let crate_root = fs::canonicalize(crate_root).unwrap_or_else(|_| crate_root.to_path_buf());
    if !visited.insert(crate_root.clone()) {
        return Ok(());
    }
    for relative in ["Cargo.toml", "Cargo.lock", "build.rs", ".cargo/config.toml"] {
        let path = crate_root.join(relative);
        if path.is_file() {
            hash_file(&path, fingerprint_root, hasher)?;
        }
    }
    hash_source_directory(&crate_root.join("src"), fingerprint_root, hasher)?;

    let manifest_path = crate_root.join("Cargo.toml");
    if manifest_path.is_file() {
        let source = fs::read_to_string(&manifest_path)
            .map_err(|error| format!("Could not read {}: {error}", manifest_path.display()))?;
        let document = source
            .parse::<DocumentMut>()
            .map_err(|error| format!("Could not parse {}: {error}", manifest_path.display()))?;
        let mut dependency_roots = Vec::new();
        collect_local_dependency_roots(document.as_item(), &crate_root, &mut dependency_roots);
        dependency_roots.sort();
        dependency_roots.dedup();
        for dependency_root in dependency_roots {
            hash_crate_build_inputs(&dependency_root, fingerprint_root, hasher, visited)?;
        }
    }
    Ok(())
}

fn collect_local_dependency_roots(
    item: &toml_edit::Item,
    manifest_root: &Path,
    output: &mut Vec<PathBuf>,
) {
    match item {
        toml_edit::Item::Table(table) => {
            if let Some(path) = table.get("path").and_then(toml_edit::Item::as_str) {
                push_local_dependency_root(manifest_root, path, output);
            }
            for (_, item) in table.iter() {
                collect_local_dependency_roots(item, manifest_root, output);
            }
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables.iter() {
                if let Some(path) = table.get("path").and_then(toml_edit::Item::as_str) {
                    push_local_dependency_root(manifest_root, path, output);
                }
                for (_, item) in table.iter() {
                    collect_local_dependency_roots(item, manifest_root, output);
                }
            }
        }
        toml_edit::Item::Value(value) => {
            collect_local_dependency_value(value, manifest_root, output)
        }
        toml_edit::Item::None => {}
    }
}

fn collect_local_dependency_value(
    value: &toml_edit::Value,
    manifest_root: &Path,
    output: &mut Vec<PathBuf>,
) {
    match value {
        toml_edit::Value::InlineTable(table) => {
            if let Some(path) = table.get("path").and_then(toml_edit::Value::as_str) {
                push_local_dependency_root(manifest_root, path, output);
            }
            for (_, value) in table.iter() {
                collect_local_dependency_value(value, manifest_root, output);
            }
        }
        toml_edit::Value::Array(values) => {
            for value in values.iter() {
                collect_local_dependency_value(value, manifest_root, output);
            }
        }
        _ => {}
    }
}

fn push_local_dependency_root(manifest_root: &Path, value: &str, output: &mut Vec<PathBuf>) {
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        manifest_root.join(path)
    };
    if path.is_dir() && path.join("Cargo.toml").is_file() {
        output.push(path);
    }
}

fn hash_source_directory(
    directory: &Path,
    project_root: &Path,
    hasher: &mut DefaultHasher,
) -> Result<(), String> {
    if !directory.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(directory)
        .map_err(|error| format!("Could not inspect {}: {error}", directory.display()))?
        .collect::<Result<_, _>>()
        .map_err(|error| format!("Could not inspect {}: {error}", directory.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            hash_source_directory(&path, project_root, hasher)?;
        } else if path.is_file() {
            hash_file(&path, project_root, hasher)?;
        }
    }
    Ok(())
}

fn hash_file(path: &Path, project_root: &Path, hasher: &mut DefaultHasher) -> Result<(), String> {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .hash(hasher);
    fs::read(path)
        .map_err(|error| format!("Could not read build input {}: {error}", path.display()))?
        .hash(hasher);
    Ok(())
}

fn read_build_fingerprint(path: &Path) -> Option<u64> {
    u64::from_str_radix(fs::read_to_string(path).ok()?.trim(), 16).ok()
}

fn persist_build_fingerprint(runner: &GameRunner) {
    let (Some(path), Some(fingerprint)) = (
        runner.build_fingerprint_path.as_deref(),
        runner.build_fingerprint,
    ) else {
        return;
    };
    let _ = fs::write(path, format!("{fingerprint:016x}\n"));
}

fn refresh_build_fingerprint(runner: &mut GameRunner) {
    let Some(project_root) = runner.build_source_root.as_deref() else {
        return;
    };
    if let Ok(fingerprint) = project_build_fingerprint(project_root) {
        runner.build_fingerprint = Some(fingerprint);
    }
}

fn attach_process_output(child: &mut Child, sender: OutputSender) {
    if let Some(stdout) = child.stdout.take() {
        pump_process_stream(stdout, OutputLevel::Info, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        pump_process_stream(stderr, OutputLevel::Info, sender);
    }
}

fn pump_process_stream(
    stream: impl Read + Send + 'static,
    default_level: OutputLevel,
    sender: OutputSender,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) => break,
                Ok(_) => {
                    let message = String::from_utf8_lossy(&bytes).trim().to_string();
                    if message.is_empty() {
                        continue;
                    }
                    let lowercase = message.to_ascii_lowercase();
                    let level = if lowercase.contains("error") || lowercase.contains("failed") {
                        OutputLevel::Error
                    } else if lowercase.contains("warning") {
                        OutputLevel::Warning
                    } else {
                        default_level
                    };
                    sender.push(level, message);
                }
                Err(error) => {
                    sender.push(OutputLevel::Error, format!("Output stream failed: {error}"));
                    break;
                }
            }
        }
    });
}

fn read_main_scene(root: &Path) -> Result<Option<PathBuf>, String> {
    let manifest_path = root.join("project.toml");
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("Could not read project.toml: {error}"))?;
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| format!("Invalid project.toml: {error}"))?;
    Ok(document
        .get("project")
        .and_then(|project| project.get("main_scene"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from))
}

fn write_main_scene(root: &Path, scene: &Path) -> Result<(), String> {
    let manifest_path = root.join("project.toml");
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("Could not read project.toml: {error}"))?;
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| format!("Invalid project.toml: {error}"))?;
    if document
        .get("project")
        .and_then(|item| item.as_table())
        .is_none()
    {
        return Err("project.toml is missing [project]".into());
    }

    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let ended_with_newline = source.ends_with('\n');
    let mut lines: Vec<String> = source.lines().map(str::to_owned).collect();
    let section_start = lines
        .iter()
        .position(|line| line.trim() == "[project]")
        .ok_or_else(|| "project.toml is missing [project]".to_string())?;
    let section_end = lines
        .iter()
        .enumerate()
        .skip(section_start + 1)
        .find(|(_, line)| {
            let line = line.trim();
            line.starts_with('[') && line.ends_with(']')
        })
        .map(|(index, _)| index)
        .unwrap_or(lines.len());
    let escaped = display_relative(scene)
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let replacement = format!("main_scene = \"{escaped}\"");
    let existing = (section_start + 1..section_end).find(|&index| {
        lines[index]
            .split_once('=')
            .is_some_and(|(key, _)| key.trim() == "main_scene")
    });
    if let Some(index) = existing {
        lines[index] = replacement;
    } else {
        lines.insert(section_end, replacement);
    }
    let mut rendered = lines.join(newline);
    if ended_with_newline {
        rendered.push_str(newline);
    }
    fs::write(&manifest_path, rendered)
        .map_err(|error| format!("Could not update project.toml: {error}"))
}

fn project_executable_name(manifest: &Path) -> Result<String, String> {
    let source = fs::read_to_string(manifest)
        .map_err(|error| format!("Could not read Cargo.toml: {error}"))?;
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| format!("Invalid Cargo.toml: {error}"))?;
    if let Some(name) = document
        .get("bin")
        .and_then(|bins| bins.as_array_of_tables())
        .and_then(|bins| bins.iter().next())
        .and_then(|bin| bin.get("name"))
        .and_then(|name| name.as_str())
    {
        return Ok(name.to_owned());
    }
    document
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml is missing package.name".to_string())
}

fn display_relative(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(windows)]
fn child_process_path(path: &Path) -> PathBuf {
    let path = path.to_string_lossy();
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.into_owned().into()
    }
}

#[cfg(not(windows))]
fn child_process_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(windows)]
fn configure_hidden_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_hidden_process(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = crate::paths::workspace_root()
            .join("target/test-temp")
            .join(format!("arisna-play-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn main_scene_round_trip_uses_project_relative_paths() {
        let root = temp_dir("main-scene");
        fs::write(
            root.join("project.toml"),
            "[project]\nname = \"Test\"\nformat_version = 1\nmain_scene = \"\"\n",
        )
        .unwrap();
        let scene = Path::new("assets/scenes/main.bsn");
        write_main_scene(&root, scene).unwrap();
        assert_eq!(read_main_scene(&root).unwrap(), Some(scene.to_path_buf()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn current_scene_uses_the_open_document_path() {
        let root = temp_dir("current-scene");
        let scene = root.join("assets/scenes/current.bsn");
        fs::create_dir_all(scene.parent().unwrap()).unwrap();
        fs::write(&scene, "").unwrap();
        let project = ProjectRoot::new(&root);
        let mut document = SceneDocument::default();
        document.open = true;
        document.path = Some(scene);

        assert_eq!(
            current_scene_relative(&project, &document).unwrap(),
            PathBuf::from("assets/scenes/current.bsn")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn active_game_is_stopped_before_restart() {
        let mut runner = GameRunner::default();
        runner.phase = GameRunPhase::Running;
        runner.pending = Some(LaunchTarget::Current);
        let mut output = OutputLog::default();

        restart_active_game(&mut runner, &mut output);

        assert_eq!(runner.phase, GameRunPhase::Stopped);
        assert_eq!(runner.pending, None);
    }

    #[test]
    fn run_status_includes_the_scene_filename() {
        assert_eq!(
            run_status("Running", Some("lobby.bsn")),
            "Running: lobby.bsn"
        );
        assert_eq!(run_status("Running", None), "Running");
    }

    #[test]
    fn child_process_directories_stay_inside_target() {
        let root = temp_dir("child-dirs");
        let target = root.join("target");
        let directories = prepare_child_process_directories(&target).unwrap();

        assert_eq!(directories.cargo_home, target.join("cargo-home"));
        assert_eq!(directories.temp, target.join("editor-temp"));
        assert_eq!(directories.data, target.join("editor-data"));
        assert!(directories.cargo_home.is_dir());
        assert!(directories.temp.is_dir());
        assert!(directories.data.is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generated_project_isolated_under_target_and_links_runtime_assets() {
        let root = temp_dir("generated-project-source");
        let target = temp_dir("generated-project-target");
        fs::create_dir_all(root.join("src/scripts")).unwrap();
        fs::create_dir_all(root.join("assets/scenes")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nrevy_engine = { package = \"arisna_engine\", path = \"../engine\" }\n",
        )
        .unwrap();
        fs::write(
            root.join("src/main.rs"),
            "mod scripts;\nfn main() { let mut app = bevy::prelude::App::new(); app.run(); }\n",
        )
        .unwrap();
        fs::write(root.join("src/scripts/mod.rs"), "pub mod player;\n").unwrap();
        fs::write(
            root.join("src/scripts/player.rs"),
            "use bevy::prelude::*;\npub fn update(In(_entity): In<Entity>) {}\n",
        )
        .unwrap();
        fs::write(root.join("assets/scenes/main.bsn"), "scene\n").unwrap();

        let generated = prepare_generated_project(&root, &target).unwrap();
        let generated_again = prepare_generated_project(&root, &target).unwrap();
        assert_eq!(generated.project_root, generated_again.project_root);
        assert!(
            fs::canonicalize(&generated.project_root)
                .unwrap()
                .starts_with(fs::canonicalize(&target).unwrap())
        );
        assert!(
            generated
                .project_root
                .join("assets/scenes/main.bsn")
                .is_file()
        );
        let generated_main =
            fs::read_to_string(generated.project_root.join("src/main.rs")).unwrap();
        assert!(generated_main.contains("revy_engine::add_revy_entity_script_fn"));
        assert!(generated_main.contains("crate::scripts::player::update"));
        assert!(
            !fs::read_to_string(root.join("src/main.rs"))
                .unwrap()
                .contains("add_revy_entity_script_fn")
        );
        assert!(
            fs::read_to_string(generated.project_root.join("Cargo.toml"))
                .unwrap()
                .contains("[workspace]")
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn runtime_registration_includes_only_scene_bound_systems() {
        let root = temp_dir("bound-system-registration");
        let entry = root.join("main.rs");
        fs::write(
            &entry,
            "fn main() { let mut app = bevy::prelude::App::new(); app.run(); }\n",
        )
        .unwrap();
        let systems = vec![
            crate::rust_components::RustSystemDefinition {
                system_path: "demo::systems::move_player".into(),
                name: "move_player".into(),
                source_path: "res://src/systems.rs".into(),
                parameters: vec!["Query<&mut Transform>".into()],
                valid: true,
                diagnostic: None,
            },
            crate::rust_components::RustSystemDefinition {
                system_path: "demo::systems::unused".into(),
                name: "unused".into(),
                source_path: "res://src/systems.rs".into(),
                parameters: vec!["Res<Time>".into()],
                valid: true,
                diagnostic: None,
            },
        ];
        let bindings = vec![SceneSystemBinding {
            script_path: "res://src/systems.rs".into(),
            system_path: "demo::systems::move_player".into(),
            schedule: SceneSystemSchedule::FixedUpdate,
            enabled: true,
            before: vec![],
            after: vec![],
        }];

        inject_runtime_registrations(&entry, "revy_engine", &[], &systems, Some(&bindings))
            .unwrap();

        let generated = fs::read_to_string(&entry).unwrap();
        assert!(generated.contains("revy_engine::add_revy_system"));
        assert!(generated.contains("SceneSystemSchedule::FixedUpdate"));
        assert!(generated.contains("crate::systems::move_player"));
        assert!(!generated.contains("crate::systems::unused"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_registration_rejects_unknown_scene_system_path() {
        let root = temp_dir("missing-system-registration");
        let entry = root.join("main.rs");
        fs::write(&entry, "fn main() { app.run(); }\n").unwrap();
        let bindings = vec![SceneSystemBinding {
            script_path: "res://src/systems.rs".into(),
            system_path: "demo::systems::missing".into(),
            ..default()
        }];

        let error = inject_runtime_registrations(&entry, "revy_engine", &[], &[], Some(&bindings))
            .unwrap_err();

        assert!(error.contains("was not found in the project source"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generated_manifest_rewrites_inline_relative_path_dependencies() {
        let root = temp_dir("manifest-path-source");
        let manifest = root.join("Cargo.toml");
        fs::write(
            &manifest,
            "[dependencies]\nlocal = { path = \"../local\", features = [\"x\"] }\n",
        )
        .unwrap();
        rewrite_generated_manifest_paths(&manifest, &root).unwrap();
        let source = fs::read_to_string(&manifest).unwrap();
        assert!(source.contains(&root.join("../local").to_string_lossy().replace('\\', "/")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shortcut_action_only_fires_on_the_press_edge() {
        assert_eq!(
            shortcut_action([false, false, false, true], [false; 4]),
            Some(GameRunAction::Stop)
        );
        assert_eq!(
            shortcut_action([false, false, false, true], [false, false, false, true]),
            None
        );
    }

    #[test]
    fn executable_name_defaults_to_package_name() {
        let root = temp_dir("binary-name");
        let manifest = root.join("Cargo.toml");
        fs::write(
            &manifest,
            "[package]\nname = \"my_game\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert_eq!(project_executable_name(&manifest).unwrap(), "my_game");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn executable_name_prefers_explicit_binary_target() {
        let root = temp_dir("binary-target");
        let manifest = root.join("Cargo.toml");
        fs::write(
            &manifest,
            "[package]\nname = \"my_game\"\nversion = \"0.1.0\"\n[[bin]]\nname = \"player\"\npath = \"src/main.rs\"\n",
        )
        .unwrap();
        assert_eq!(project_executable_name(&manifest).unwrap(), "player");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn child_process_paths_do_not_use_windows_verbatim_prefixes() {
        assert_eq!(
            child_process_path(Path::new(r"\\?\E:\Games\Demo\Cargo.toml")),
            PathBuf::from(r"E:\Games\Demo\Cargo.toml")
        );
        assert_eq!(
            child_process_path(Path::new(r"\\?\UNC\server\share\Demo")),
            PathBuf::from(r"\\server\share\Demo")
        );
    }

    #[test]
    fn build_fingerprint_changes_only_for_build_inputs() {
        let root = temp_dir("fingerprint");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("assets/scene.ron"), "scene v1\n").unwrap();

        let initial = project_build_fingerprint(&root).unwrap();
        fs::write(root.join("assets/scene.ron"), "scene v2\n").unwrap();
        assert_eq!(project_build_fingerprint(&root).unwrap(), initial);

        fs::write(
            root.join("src/main.rs"),
            "fn main() { println!(\"changed\"); }\n",
        )
        .unwrap();
        assert_ne!(project_build_fingerprint(&root).unwrap(), initial);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn build_fingerprint_tracks_local_path_dependency_rust_but_not_assets() {
        let root = temp_dir("path-dependency-fingerprint");
        let dependency = root.join("local-engine");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(dependency.join("src")).unwrap();
        fs::create_dir_all(dependency.join("assets")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n[dependencies]\nlocal_engine = { path = \"local-engine\" }\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            dependency.join("Cargo.toml"),
            "[package]\nname = \"local_engine\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            dependency.join("src/lib.rs"),
            "pub fn value() -> i32 { 1 }\n",
        )
        .unwrap();
        fs::write(dependency.join("assets/scene.bsn"), "scene v1\n").unwrap();

        let initial = project_build_fingerprint(&root).unwrap();
        fs::write(dependency.join("assets/scene.bsn"), "scene v2\n").unwrap();
        assert_eq!(project_build_fingerprint(&root).unwrap(), initial);

        fs::write(
            dependency.join("src/lib.rs"),
            "pub fn value() -> i32 { 2 }\n",
        )
        .unwrap();
        assert_ne!(project_build_fingerprint(&root).unwrap(), initial);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn successful_build_refreshes_fingerprint_after_lockfile_creation() {
        let root = temp_dir("refresh-fingerprint");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

        let before = project_build_fingerprint(&root).unwrap();
        let mut runner = GameRunner::default();
        runner.build_fingerprint = Some(before);
        runner.build_fingerprint_path = Some(root.join(".arisna/build-fingerprint"));
        runner.build_source_root = Some(root.clone());
        fs::write(root.join("Cargo.lock"), "# generated by Cargo\n").unwrap();

        refresh_build_fingerprint(&mut runner);

        assert_eq!(
            runner.build_fingerprint,
            Some(project_build_fingerprint(&root).unwrap())
        );
        assert_ne!(runner.build_fingerprint, Some(before));
        fs::remove_dir_all(root).unwrap();
    }
}
