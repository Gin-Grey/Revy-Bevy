use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use toml_edit::{DocumentMut, Item};

use crate::paths;

pub const SUPPORTED_FORMAT_VERSION: i64 = 1;
const CONFIG_FILE_NAME: &str = "project_manager.toml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectMetadata {
    pub name: String,
    pub format_version: i64,
    pub icon: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct RecentProject {
    pub path: PathBuf,
    pub name: String,
    pub last_opened: u64,
    pub validation_error: Option<String>,
    pub icon: Option<PathBuf>,
}

impl RecentProject {
    pub fn is_missing(&self) -> bool {
        !self.path.is_dir()
    }

    pub fn is_valid(&self) -> bool {
        self.validation_error.is_none()
    }
}

#[derive(Clone, Debug, Default)]
pub struct RecentProjects {
    pub projects: Vec<RecentProject>,
    pub last_project: Option<PathBuf>,
}

impl RecentProjects {
    pub fn load() -> Self {
        let path = config_path();
        let source = fs::read_to_string(&path)
            // Read the old location once for a seamless Arisna -> Revy migration.
            .or_else(|_| fs::read_to_string(legacy_config_path()));
        let Ok(source) = source else {
            return Self::with_starter_project();
        };
        Self::from_config_source(&source).unwrap_or_else(Self::with_starter_project)
    }

    fn from_config_source(source: &str) -> Option<Self> {
        let Ok(document) = source.parse::<DocumentMut>() else {
            return None;
        };

        let last_project = document
            .get("last_project")
            .and_then(Item::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);
        let mut projects = Vec::new();
        if let Some(entries) = document.get("projects").and_then(Item::as_array_of_tables) {
            for entry in entries {
                let Some(path) = entry.get("path").and_then(Item::as_str).map(PathBuf::from) else {
                    continue;
                };
                if projects
                    .iter()
                    .any(|candidate: &RecentProject| same_path(&candidate.path, &path))
                {
                    continue;
                }
                let last_opened = entry
                    .get("last_opened")
                    .and_then(Item::as_integer)
                    .and_then(|value| u64::try_from(value).ok())
                    .unwrap_or(0);
                projects.push(project_from_disk(path, last_opened));
            }
        }

        projects.sort_by(|left, right| {
            right
                .last_opened
                .cmp(&left.last_opened)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        Some(Self {
            projects,
            last_project,
        })
    }

    pub fn with_starter_project() -> Self {
        let starter = paths::default_project_root();
        if starter.join("project.toml").is_file() {
            let modified = project_modified_time(&starter);
            Self {
                projects: vec![project_from_disk(starter.clone(), modified)],
                last_project: Some(starter),
            }
        } else {
            Self::default()
        }
    }

    pub fn add_or_update(&mut self, path: &Path, opened_at: u64) -> Result<PathBuf, String> {
        let canonical = canonical_project_path(path)?;
        let metadata = validate_project(&canonical)?;
        self.projects
            .retain(|project| !same_path(&project.path, &canonical));
        self.projects.push(RecentProject {
            path: canonical.clone(),
            name: metadata.name,
            last_opened: opened_at,
            validation_error: None,
            icon: metadata.icon,
        });
        self.projects.sort_by(|left, right| {
            right
                .last_opened
                .cmp(&left.last_opened)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        self.last_project = Some(canonical.clone());
        Ok(canonical)
    }

    pub fn remove(&mut self, path: &Path) {
        self.projects
            .retain(|project| !same_path(&project.path, path));
        if self
            .last_project
            .as_ref()
            .is_some_and(|last| same_path(last, path))
        {
            self.last_project = self.projects.first().map(|project| project.path.clone());
        }
    }

    pub fn remove_missing(&mut self) -> usize {
        let before = self.projects.len();
        self.projects.retain(|project| !project.is_missing());
        if self.last_project.as_ref().is_some_and(|last| {
            !self
                .projects
                .iter()
                .any(|project| same_path(&project.path, last))
        }) {
            self.last_project = self.projects.first().map(|project| project.path.clone());
        }
        before - self.projects.len()
    }

    pub fn refresh(&mut self) {
        for project in &mut self.projects {
            *project = project_from_disk(project.path.clone(), project.last_opened);
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create settings directory: {error}"))?;
        }

        let last_project = self
            .last_project
            .as_deref()
            .map(path_string)
            .unwrap_or_default();
        let mut output = format!(
            "format_version = 1\nlast_project = \"{}\"\n",
            escape_toml_string(&last_project)
        );
        for project in &self.projects {
            output.push_str("\n[[projects]]\npath = \"");
            output.push_str(&escape_toml_string(&path_string(&project.path)));
            output.push_str("\"\nlast_opened = ");
            output.push_str(
                &i64::try_from(project.last_opened)
                    .unwrap_or(i64::MAX)
                    .to_string(),
            );
            output.push('\n');
        }
        fs::write(&path, output).map_err(|error| format!("Could not save project list: {error}"))
    }
}

pub fn validate_project(root: &Path) -> Result<ProjectMetadata, String> {
    if !root.exists() {
        return Err("Project folder does not exist".into());
    }
    if !root.is_dir() {
        return Err("Project path is not a folder".into());
    }
    let manifest_path = root.join("project.toml");
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("Cannot read project.toml: {error}"))?;
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| format!("Invalid project.toml: {error}"))?;
    let project = document
        .get("project")
        .and_then(Item::as_table_like)
        .ok_or_else(|| "project.toml is missing [project]".to_string())?;
    let name = project
        .get("name")
        .and_then(Item::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "project.name must be a non-empty string".to_string())?
        .to_string();
    let format_version = project
        .get("format_version")
        .and_then(Item::as_integer)
        .ok_or_else(|| "project.format_version must be an integer".to_string())?;
    if format_version != SUPPORTED_FORMAT_VERSION {
        return Err(format!(
            "Unsupported project format {format_version}; expected {SUPPORTED_FORMAT_VERSION}"
        ));
    }

    let cargo_path = root.join("Cargo.toml");
    let cargo_source = fs::read_to_string(&cargo_path)
        .map_err(|error| format!("Cannot read Cargo.toml: {error}"))?;
    let cargo = cargo_source
        .parse::<DocumentMut>()
        .map_err(|error| format!("Invalid Cargo.toml: {error}"))?;
    let uses_revy = cargo
        .get("dependencies")
        .and_then(Item::as_table_like)
        .is_some_and(|dependencies| {
            dependencies.contains_key("revy_engine") || dependencies.contains_key("arisna_engine")
        });
    if !uses_revy {
        return Err(
            "Cargo.toml is missing the revy_engine dependency (legacy arisna_engine is also accepted)"
                .into(),
        );
    }
    if !root.join("src/main.rs").is_file() {
        return Err("Project is missing src/main.rs".into());
    }

    let configured_icon = project
        .get("icon")
        .and_then(Item::as_str)
        .map(PathBuf::from);
    let icon = configured_icon
        .into_iter()
        .chain([PathBuf::from("icon.png"), PathBuf::from("assets/icon.png")])
        .find_map(|relative| safe_project_file(root, &relative));

    Ok(ProjectMetadata {
        name,
        format_version,
        icon,
    })
}

pub fn create_project(name: &str, parent: &Path) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name cannot be empty".into());
    }
    let folder_name = project_folder_name(name);
    if folder_name.is_empty() {
        return Err("Project name does not contain a valid folder name".into());
    }
    let target = absolute_path(&parent.join(folder_name));
    ensure_empty_target(&target)?;

    let template_root = paths::project_template_root();
    let engine_root = paths::engine_sdk_root();
    let vendor_root = paths::cargo_vendor_root();
    for required in [
        template_root.join("Cargo.toml.template"),
        template_root.join("project.toml.template"),
        template_root.join("src/main.rs"),
        template_root.join(".cargo/config.toml.template"),
    ] {
        if !required.is_file() {
            return Err(format!(
                "Project template is incomplete: {}",
                required.display()
            ));
        }
    }
    if !engine_root.is_dir() {
        return Err(format!(
            "Engine SDK was not found: {}",
            engine_root.display()
        ));
    }
    if !vendor_root.is_dir() {
        return Err(format!(
            "Bundled Cargo packages were not found: {}",
            vendor_root.display()
        ));
    }

    fs::create_dir_all(&target)
        .map_err(|error| format!("Could not create project folder: {error}"))?;
    copy_directory(&template_root, &target)?;

    let package_name = rust_package_name(name);
    let project_name_toml = escape_toml_string(name);
    let project_name_rust = escape_rust_string(name);
    let engine_path_toml = escape_toml_string(&path_string(&engine_root));
    let vendor_path_toml = escape_toml_string(&path_string(&vendor_root));
    materialize_template(
        &target.join("Cargo.toml.template"),
        &target.join("Cargo.toml"),
        &[
            ("{{PACKAGE_NAME}}", package_name.as_str()),
            ("{{ENGINE_PATH}}", engine_path_toml.as_str()),
        ],
    )?;
    materialize_template(
        &target.join("project.toml.template"),
        &target.join("project.toml"),
        &[("{{PROJECT_NAME}}", project_name_toml.as_str())],
    )?;
    replace_in_file(
        &target.join("src/main.rs"),
        "{{PROJECT_NAME}}",
        &project_name_rust,
    )?;
    materialize_template(
        &target.join(".cargo/config.toml.template"),
        &target.join(".cargo/config.toml"),
        &[("{{CARGO_VENDOR_PATH}}", vendor_path_toml.as_str())],
    )?;
    fs::create_dir_all(target.join("assets/scenes"))
        .map_err(|error| format!("Could not create assets/scenes: {error}"))?;

    validate_project(&target)?;
    canonical_project_path(&target)
}

pub fn create_target_path(name: &str, parent: &Path) -> PathBuf {
    absolute_path(&parent.join(project_folder_name(name.trim())))
}

pub fn create_validation(name: &str, parent: &Path) -> Result<PathBuf, String> {
    if name.trim().is_empty() {
        return Err("Enter a project name".into());
    }
    if parent.as_os_str().is_empty() {
        return Err("Choose a location".into());
    }
    let target = create_target_path(name, parent);
    ensure_empty_target(&target)?;
    Ok(target)
}

pub fn now_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn relative_time(timestamp: u64) -> String {
    if timestamp == 0 {
        return "Never opened".into();
    }
    let elapsed = now_epoch_seconds().saturating_sub(timestamp);
    match elapsed {
        0..=59 => "Just now".into(),
        60..=3_599 => format!("{} min ago", elapsed / 60),
        3_600..=86_399 => format!("{} hr ago", elapsed / 3_600),
        86_400..=2_591_999 => format!("{} days ago", elapsed / 86_400),
        2_592_000..=31_535_999 => format!("{} months ago", elapsed / 2_592_000),
        _ => format!("{} years ago", elapsed / 31_536_000),
    }
}

pub fn config_path() -> PathBuf {
    // Keep editor-owned state beside the workspace/package instead of writing
    // new application data to the system drive.
    if cfg!(debug_assertions) {
        paths::workspace_root()
            .join("target/editor-data/Revy")
            .join(CONFIG_FILE_NAME)
    } else {
        paths::executable_directory()
            .join("data/Revy")
            .join(CONFIG_FILE_NAME)
    }
}

fn legacy_config_path() -> PathBuf {
    if cfg!(target_os = "windows") {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join("Arisna")
            .join(CONFIG_FILE_NAME)
    } else if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(root).join("arisna").join(CONFIG_FILE_NAME)
    } else {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join(".config/arisna")
            .join(CONFIG_FILE_NAME)
    }
}

fn project_from_disk(path: PathBuf, last_opened: u64) -> RecentProject {
    match validate_project(&path) {
        Ok(metadata) => RecentProject {
            path,
            name: metadata.name,
            last_opened,
            validation_error: None,
            icon: metadata.icon,
        },
        Err(error) => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Missing project".into());
            RecentProject {
                path,
                name,
                last_opened,
                validation_error: Some(error),
                icon: None,
            }
        }
    }
}

fn project_modified_time(root: &Path) -> u64 {
    fs::metadata(root.join("project.toml"))
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn canonical_project_path(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| format!("Cannot open project folder: {error}"))
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn safe_project_file(root: &Path, relative: &Path) -> Option<PathBuf> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    let path = root.join(relative);
    path.is_file().then_some(path)
}

fn ensure_empty_target(target: &Path) -> Result<(), String> {
    if !target.exists() {
        return Ok(());
    }
    if !target.is_dir() {
        return Err("The target path is not a folder".into());
    }
    let mut entries =
        fs::read_dir(target).map_err(|error| format!("Cannot inspect target folder: {error}"))?;
    if entries.next().is_some() {
        return Err(format!("Target folder is not empty: {}", target.display()));
    }
    Ok(())
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|error| error.to_string())?;
    let entries = fs::read_dir(source)
        .map_err(|error| format!("Cannot read template {}: {error}", source.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let destination = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &destination)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), destination)
                .map_err(|error| format!("Could not copy project template: {error}"))?;
        }
    }
    Ok(())
}

fn materialize_template(
    template: &Path,
    destination: &Path,
    replacements: &[(&str, &str)],
) -> Result<(), String> {
    let mut source = fs::read_to_string(template)
        .map_err(|error| format!("Cannot read {}: {error}", template.display()))?;
    for (from, to) in replacements {
        source = source.replace(from, to);
    }
    fs::write(destination, source)
        .map_err(|error| format!("Cannot write {}: {error}", destination.display()))?;
    fs::remove_file(template)
        .map_err(|error| format!("Cannot finish {}: {error}", destination.display()))
}

fn replace_in_file(path: &Path, from: &str, to: &str) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    fs::write(path, source.replace(from, to))
        .map_err(|error| format!("Cannot write {}: {error}", path.display()))
}

fn project_folder_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) || character.is_control()
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_string()
}

fn rust_package_name(name: &str) -> String {
    let mut package = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['_', '-'])
        .to_string();
    if package.is_empty() {
        package = "revy_game".into();
    }
    if package.starts_with(|character: char| character.is_ascii_digit()) {
        package.insert_str(0, "game_");
    }
    package
}

pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    if cfg!(target_os = "windows") {
        path_string(left).eq_ignore_ascii_case(&path_string(right))
    } else {
        left == right
    }
}

fn path_string(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_string()
    } else {
        normalized
    }
}

fn escape_toml_string(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            '\u{08}' => "\\b".chars().collect(),
            '\u{0c}' => "\\f".chars().collect(),
            control if control.is_control() => {
                format!("\\u{:04X}", control as u32).chars().collect()
            }
            other => vec![other],
        })
        .collect()
}

fn escape_rust_string(value: &str) -> String {
    value.chars().flat_map(char::escape_default).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "revy-project-manager-{name}-{}-{}",
            std::process::id(),
            now_epoch_seconds()
        ))
    }

    fn write_minimal_project(root: &Path, name: &str) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("project.toml"),
            format!("[project]\nname = \"{name}\"\nformat_version = 1\n"),
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\narisna_engine = { path = \"engine\" }\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    }

    #[test]
    fn validates_supported_project_manifest() {
        let root = test_root("validate");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("project.toml"),
            "[project]\nname = \"Demo\"\nformat_version = 1\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\narisna_engine = { path = \"engine\" }\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

        let metadata = validate_project(&root).unwrap();
        assert_eq!(metadata.name, "Demo");
        assert_eq!(metadata.format_version, 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_and_future_project_manifests() {
        let root = test_root("invalid");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("project.toml"),
            "[project]\nname = \"Demo\"\nformat_version = 99\n",
        )
        .unwrap();

        let error = validate_project(&root).unwrap_err();
        assert!(error.contains("Unsupported project format"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_non_revy_rust_project() {
        let root = test_root("not-revy");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("project.toml"),
            "[project]\nname = \"Demo\"\nformat_version = 1\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

        let error = validate_project(&root).unwrap_err();
        assert!(error.contains("revy_engine"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creates_safe_folder_and_package_names() {
        assert_eq!(project_folder_name(" Demo: Game? "), "Demo_ Game_");
        assert_eq!(rust_package_name("42 Hello World"), "game_42_hello_world");
        assert_eq!(rust_package_name("\u{6d4b}\u{8bd5}"), "revy_game");
    }

    #[test]
    fn refuses_non_empty_creation_target() {
        let root = test_root("non-empty");
        let target = root.join("Demo");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), "user data").unwrap();

        let error = create_validation("Demo", &root).unwrap_err();
        assert!(error.contains("not empty"));
        assert!(target.join("keep.txt").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creates_complete_project_from_template() {
        let root = test_root("create");
        fs::create_dir_all(&root).unwrap();

        let project = create_project("My Test Game", &root).unwrap();
        let cargo = fs::read_to_string(project.join("Cargo.toml")).unwrap();
        let manifest = fs::read_to_string(project.join("project.toml")).unwrap();
        let main = fs::read_to_string(project.join("src/main.rs")).unwrap();
        let cargo_config = fs::read_to_string(project.join(".cargo/config.toml")).unwrap();

        assert!(cargo.contains("name = \"my_test_game\""));
        assert!(cargo.contains("revy_engine"));
        assert!(manifest.contains("name = \"My Test Game\""));
        assert!(manifest.contains("format_version = 1"));
        assert!(main.contains("My Test Game"));
        assert!(cargo_config.contains("offline = true"));
        assert!(project.join("assets/scenes").is_dir());
        assert!(!project.join("Cargo.toml.template").exists());
        assert!(!project.join("project.toml.template").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn escapes_project_name_in_generated_toml_and_rust() {
        let root = test_root("escape");
        fs::create_dir_all(&root).unwrap();

        let project = create_project("Quoted \"Game\"\\Next", &root).unwrap();
        let metadata = validate_project(&project).unwrap();
        let main = fs::read_to_string(project.join("src/main.rs")).unwrap();

        assert_eq!(metadata.name, "Quoted \"Game\"\\Next");
        assert!(main.contains(r#"title: "Quoted \"Game\"\\Next".into()"#));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn valid_empty_recent_list_stays_empty() {
        let recents =
            RecentProjects::from_config_source("format_version = 1\nlast_project = \"\"\n")
                .unwrap();

        assert!(recents.projects.is_empty());
        assert!(recents.last_project.is_none());
    }

    #[test]
    fn windows_verbatim_and_regular_paths_match() {
        if cfg!(target_os = "windows") {
            assert!(same_path(
                Path::new(r"\\?\E:\Games\Demo"),
                Path::new(r"E:\Games\Demo")
            ));
            assert_eq!(
                path_string(Path::new(r"\\?\UNC\server\share\Demo")),
                "//server/share/Demo"
            );
        }
    }

    #[test]
    fn reopening_project_updates_one_cached_record() {
        let root = test_root("reopen");
        write_minimal_project(&root, "Demo");
        let mut recents = RecentProjects::default();

        let canonical = recents.add_or_update(&root, 10).unwrap();
        recents.add_or_update(&root, 20).unwrap();

        assert_eq!(recents.projects.len(), 1);
        assert_eq!(recents.projects[0].last_opened, 20);
        assert!(same_path(&recents.projects[0].path, &canonical));
        assert!(
            recents
                .last_project
                .as_ref()
                .is_some_and(|path| same_path(path, &canonical))
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_cached_project_keeps_files_on_disk() {
        let root = test_root("remove-record");
        write_minimal_project(&root, "Keep Me");
        let marker = root.join("assets/keep.txt");
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(&marker, "user data").unwrap();
        let mut recents = RecentProjects::default();
        recents.add_or_update(&root, 10).unwrap();

        recents.remove(&root);

        assert!(recents.projects.is_empty());
        assert!(recents.last_project.is_none());
        assert_eq!(fs::read_to_string(&marker).unwrap(), "user data");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remove_missing_keeps_existing_projects() {
        let root = test_root("remove-missing");
        write_minimal_project(&root, "Existing");
        let missing = root.with_file_name(format!(
            "{}-missing",
            root.file_name().unwrap().to_string_lossy()
        ));
        let existing = project_from_disk(root.clone(), 20);
        let missing_record = project_from_disk(missing.clone(), 10);
        let mut recents = RecentProjects {
            projects: vec![existing, missing_record],
            last_project: Some(missing),
        };

        assert_eq!(recents.remove_missing(), 1);
        assert_eq!(recents.projects.len(), 1);
        assert!(same_path(&recents.projects[0].path, &root));
        assert!(
            recents
                .last_project
                .as_ref()
                .is_some_and(|path| same_path(path, &root))
        );

        fs::remove_dir_all(root).unwrap();
    }
}
