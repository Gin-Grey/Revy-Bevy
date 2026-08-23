use std::path::{Path, PathBuf};

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

pub fn default_project_root() -> PathBuf {
    if cfg!(debug_assertions) {
        workspace_root().join("project")
    } else {
        executable_directory().join("game_project")
    }
}

pub fn editor_asset_root() -> PathBuf {
    if cfg!(debug_assertions) {
        workspace_root().join("assets")
    } else {
        executable_directory().join("assets")
    }
}

pub fn project_template_root() -> PathBuf {
    if cfg!(debug_assertions) {
        workspace_root().join("build/templates/rust_game")
    } else {
        executable_directory().join("templates/rust_game")
    }
}

pub fn engine_sdk_root() -> PathBuf {
    if cfg!(debug_assertions) {
        workspace_root().join("engine/crates/arisna_engine")
    } else {
        executable_directory().join("sdk/source/engine/crates/arisna_engine")
    }
}

pub fn cargo_vendor_root() -> PathBuf {
    if cfg!(debug_assertions) {
        workspace_root().join("build/vendor/cargo")
    } else {
        executable_directory().join("sdk/source/vendor/cargo")
    }
}

pub fn executable_directory() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}
