//! Discovers project-defined Rust ECS components for the Inspector.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use arisna_engine::{EntityScriptLifecycle, ProjectRoot, SceneCustomComponent, SceneCustomField};
use bevy::prelude::*;
use syn::{Fields, FnArg, GenericArgument, Item, PathArguments, Type};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustComponentFieldDefinition {
    pub name: String,
    pub type_name: String,
    pub default_value: String,
    pub editable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustComponentDefinition {
    pub type_path: String,
    pub name: String,
    pub source_path: String,
    pub fields: Vec<RustComponentFieldDefinition>,
    /// The source type exposes Bevy reflection metadata. Concrete runtime
    /// insertion still requires the game to register the type with its App.
    pub reflection_ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustSystemDefinition {
    pub system_path: String,
    pub name: String,
    pub source_path: String,
    pub parameters: Vec<String>,
    /// Whether the generated game entry point can call this function.
    pub valid: bool,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustEntityScriptFunction {
    pub function_path: String,
    pub name: String,
    pub source_path: String,
    pub lifecycle: EntityScriptLifecycle,
    pub valid: bool,
    pub diagnostic: Option<String>,
}

impl RustSystemDefinition {
    pub fn query_summary(&self) -> String {
        parameter_summary(&self.parameters, &["Query", "Single", "ParamSet"])
    }

    pub fn access_summary(&self) -> String {
        parameter_summary(
            &self.parameters,
            &[
                "Commands",
                "Res",
                "ResMut",
                "Local",
                "EventReader",
                "EventWriter",
                "MessageReader",
                "MessageWriter",
                "NonSend",
                "NonSendMut",
            ],
        )
    }
}

impl RustComponentDefinition {
    pub fn instantiate(&self) -> SceneCustomComponent {
        SceneCustomComponent {
            type_path: self.type_path.clone(),
            source_path: self.source_path.clone(),
            fields: self
                .fields
                .iter()
                .map(|field| SceneCustomField {
                    name: field.name.clone(),
                    type_name: field.type_name.clone(),
                    value: field.default_value.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Resource, Debug, Default)]
pub struct RustComponentRegistry {
    pub components: Vec<RustComponentDefinition>,
    pub systems: Vec<RustSystemDefinition>,
    pub entity_scripts: Vec<RustEntityScriptFunction>,
    pub revision: u64,
    fingerprint: u64,
}

impl RustComponentRegistry {
    pub fn get(&self, type_path: &str) -> Option<&RustComponentDefinition> {
        self.components
            .iter()
            .find(|component| component.type_path == type_path)
    }

    pub fn get_system(&self, system_path: &str) -> Option<&RustSystemDefinition> {
        self.systems
            .iter()
            .find(|system| system.system_path == system_path)
    }
}

#[derive(Resource)]
struct RustComponentScanTimer(Timer);

impl Default for RustComponentScanTimer {
    fn default() -> Self {
        Self(Timer::new(Duration::from_secs(2), TimerMode::Repeating))
    }
}

pub struct RustComponentRegistryPlugin;

impl Plugin for RustComponentRegistryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RustComponentRegistry>()
            .init_resource::<RustComponentScanTimer>()
            .add_systems(Startup, scan_project_components)
            .add_systems(Update, refresh_project_components);
    }
}

fn refresh_project_components(
    time: Res<Time>,
    project: Res<ProjectRoot>,
    mut timer: ResMut<RustComponentScanTimer>,
    mut registry: ResMut<RustComponentRegistry>,
) {
    timer.0.tick(time.delta());
    if !timer.0.just_finished() {
        return;
    }
    let fingerprint = source_fingerprint(&project.root);
    if fingerprint != registry.fingerprint {
        apply_scan(&project.root, &mut registry);
    }
}

fn scan_project_components(project: Res<ProjectRoot>, mut registry: ResMut<RustComponentRegistry>) {
    apply_scan(&project.root, &mut registry);
}

fn apply_scan(root: &Path, registry: &mut RustComponentRegistry) {
    let (components, systems, entity_scripts) = discover_rust_ecs(root).unwrap_or_default();
    let fingerprint = source_fingerprint(root);
    if registry.components != components
        || registry.systems != systems
        || registry.entity_scripts != entity_scripts
        || registry.fingerprint != fingerprint
    {
        registry.components = components;
        registry.systems = systems;
        registry.entity_scripts = entity_scripts;
        registry.fingerprint = fingerprint;
        registry.revision = registry.revision.wrapping_add(1);
    }
}

pub fn discover_components(root: &Path) -> Result<Vec<RustComponentDefinition>, String> {
    discover_rust_ecs(root).map(|(components, _, _)| components)
}

pub fn discover_systems(root: &Path) -> Result<Vec<RustSystemDefinition>, String> {
    discover_rust_ecs(root).map(|(_, systems, _)| systems)
}

pub fn discover_entity_scripts(root: &Path) -> Result<Vec<RustEntityScriptFunction>, String> {
    discover_rust_ecs(root).map(|(_, _, scripts)| scripts)
}

fn discover_rust_ecs(
    root: &Path,
) -> Result<
    (
        Vec<RustComponentDefinition>,
        Vec<RustSystemDefinition>,
        Vec<RustEntityScriptFunction>,
    ),
    String,
> {
    let crate_name = package_name(root).unwrap_or_else(|| "game".into());
    let source_root = root.join("src");
    if !source_root.is_dir() {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files).map_err(|error| error.to_string())?;
    files.sort();
    let mut components = Vec::new();
    let mut systems = Vec::new();
    let mut entity_scripts = Vec::new();
    for path in files {
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = syn::parse_file(&source) else {
            continue;
        };
        let module = module_path(&source_root, &path);
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        collect_items(
            &file.items,
            &crate_name,
            &module,
            &format!("res://{relative}"),
            &mut components,
            &mut systems,
            &mut entity_scripts,
        );
    }
    components.sort_by(|left, right| left.type_path.cmp(&right.type_path));
    components.dedup_by(|left, right| left.type_path == right.type_path);
    systems.sort_by(|left, right| left.system_path.cmp(&right.system_path));
    systems.dedup_by(|left, right| left.system_path == right.system_path);
    entity_scripts.sort_by(|left, right| left.function_path.cmp(&right.function_path));
    entity_scripts.dedup_by(|left, right| left.function_path == right.function_path);
    Ok((components, systems, entity_scripts))
}

fn collect_items(
    items: &[Item],
    crate_name: &str,
    module: &[String],
    source_path: &str,
    output: &mut Vec<RustComponentDefinition>,
    systems: &mut Vec<RustSystemDefinition>,
    entity_scripts: &mut Vec<RustEntityScriptFunction>,
) {
    for item in items {
        match item {
            Item::Struct(item) if derives_component(&item.attrs) => {
                let name = item.ident.to_string();
                let mut path = Vec::with_capacity(module.len() + 2);
                path.push(crate_name.to_owned());
                path.extend(module.iter().cloned());
                path.push(name.clone());
                let fields = match &item.fields {
                    Fields::Named(fields) => fields
                        .named
                        .iter()
                        .filter_map(|field| {
                            let name = field.ident.as_ref()?.to_string();
                            let type_name = type_name(&field.ty);
                            let default_value = default_value(&type_name);
                            Some(RustComponentFieldDefinition {
                                name,
                                editable: default_value.is_some(),
                                default_value: default_value.unwrap_or_default(),
                                type_name,
                            })
                        })
                        .collect(),
                    Fields::Unnamed(fields) => fields
                        .unnamed
                        .iter()
                        .enumerate()
                        .map(|(index, field)| {
                            let type_name = type_name(&field.ty);
                            let default_value = default_value(&type_name);
                            RustComponentFieldDefinition {
                                name: index.to_string(),
                                editable: default_value.is_some(),
                                default_value: default_value.unwrap_or_default(),
                                type_name,
                            }
                        })
                        .collect(),
                    Fields::Unit => Vec::new(),
                };
                output.push(RustComponentDefinition {
                    type_path: path.join("::"),
                    name,
                    source_path: source_path.to_owned(),
                    fields,
                    reflection_ready: derives_reflect(&item.attrs)
                        && reflects_component(&item.attrs)
                        && derives_name(&item.attrs, "Default"),
                });
            }
            Item::Mod(item) => {
                if let Some((_, items)) = &item.content {
                    let mut nested = module.to_vec();
                    nested.push(item.ident.to_string());
                    collect_items(
                        items,
                        crate_name,
                        &nested,
                        source_path,
                        output,
                        systems,
                        entity_scripts,
                    );
                }
            }
            Item::Fn(item) => {
                let name = item.sig.ident.to_string();
                let mut path = Vec::with_capacity(module.len() + 2);
                path.push(crate_name.to_owned());
                path.extend(module.iter().cloned());
                path.push(name.clone());
                if let Some(lifecycle) = entity_script_lifecycle(&name) {
                    let has_entity_input = item.sig.inputs.iter().any(is_entity_input);
                    let visible =
                        module.is_empty() || !matches!(item.vis, syn::Visibility::Inherited);
                    let diagnostic = (!has_entity_input)
                        .then(|| format!("{source_path}: `{name}` must accept In<Entity>"))
                        .or_else(|| {
                            (!visible).then(|| {
                                format!("{source_path}: nested `{name}` must be pub(crate)")
                            })
                        });
                    entity_scripts.push(RustEntityScriptFunction {
                        function_path: path.join("::"),
                        name,
                        source_path: source_path.to_owned(),
                        lifecycle,
                        valid: diagnostic.is_none(),
                        diagnostic,
                    });
                } else if item.sig.inputs.iter().any(is_system_parameter) {
                    let diagnostic = system_diagnostic(item, module, source_path, &name);
                    systems.push(RustSystemDefinition {
                        system_path: path.join("::"),
                        name,
                        source_path: source_path.to_owned(),
                        parameters: item
                            .sig
                            .inputs
                            .iter()
                            .filter_map(|argument| match argument {
                                FnArg::Typed(argument) => Some(type_name(&argument.ty)),
                                FnArg::Receiver(_) => None,
                            })
                            .collect(),
                        valid: diagnostic.is_none(),
                        diagnostic,
                    });
                }
            }
            _ => {}
        }
    }
}

fn derives_component(attributes: &[syn::Attribute]) -> bool {
    derives_name(attributes, "Component")
}

fn derives_reflect(attributes: &[syn::Attribute]) -> bool {
    derives_name(attributes, "Reflect")
}

fn derives_name(attributes: &[syn::Attribute], expected: &str) -> bool {
    attributes.iter().any(|attribute| {
        if !attribute.path().is_ident("derive") {
            return false;
        }
        attribute
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|paths| {
                paths.iter().any(|path| {
                    path.segments
                        .last()
                        .is_some_and(|segment| segment.ident == expected)
                })
            })
    })
}

fn reflects_component(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("reflect")
            && attribute
                .meta
                .require_list()
                .is_ok_and(|list| list.tokens.to_string().contains("Component"))
    })
}

fn type_name(ty: &Type) -> String {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .iter()
            .map(|segment| {
                let name = segment.ident.to_string();
                match &segment.arguments {
                    PathArguments::AngleBracketed(arguments) => {
                        let arguments = arguments
                            .args
                            .iter()
                            .filter_map(|argument| match argument {
                                GenericArgument::Type(ty) => Some(type_name(ty)),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        if arguments.is_empty() {
                            name
                        } else {
                            format!("{name}<{}>", arguments.join(", "))
                        }
                    }
                    _ => name,
                }
            })
            .collect::<Vec<_>>()
            .join("::"),
        Type::Reference(reference) => format!(
            "&{}{}",
            if reference.mutability.is_some() {
                "mut "
            } else {
                ""
            },
            type_name(&reference.elem)
        ),
        Type::Tuple(tuple) => format!(
            "({})",
            tuple
                .elems
                .iter()
                .map(type_name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => "Unsupported".into(),
    }
}

fn is_system_parameter(argument: &FnArg) -> bool {
    let FnArg::Typed(argument) = argument else {
        return false;
    };
    let Type::Path(path) = argument.ty.as_ref() else {
        return false;
    };
    path.path.segments.last().is_some_and(|segment| {
        matches!(
            segment.ident.to_string().as_str(),
            "Commands"
                | "Query"
                | "Single"
                | "ParamSet"
                | "Res"
                | "ResMut"
                | "Local"
                | "EventReader"
                | "EventWriter"
                | "MessageReader"
                | "MessageWriter"
                | "NonSend"
                | "NonSendMut"
        )
    })
}

fn system_diagnostic(
    item: &syn::ItemFn,
    module: &[String],
    source_path: &str,
    name: &str,
) -> Option<String> {
    if !module.is_empty() && !is_crate_visible(&item.vis) {
        return Some(format!(
            "{source_path}: nested system `{name}` must be pub(crate)"
        ));
    }
    if item.sig.asyncness.is_some()
        || item.sig.constness.is_some()
        || item.sig.unsafety.is_some()
        || item.sig.abi.is_some()
        || item.sig.variadic.is_some()
        || !item.sig.generics.params.is_empty()
    {
        return Some(format!(
            "{source_path}: system `{name}` must be a non-async, non-generic safe Rust function"
        ));
    }
    if item.sig.inputs.iter().any(is_any_input_parameter) {
        return Some(format!(
            "{source_path}: system `{name}` cannot accept In<T>; use an Entity lifecycle script for In<Entity>"
        ));
    }
    let returns_unit = match &item.sig.output {
        syn::ReturnType::Default => true,
        syn::ReturnType::Type(_, output) => {
            matches!(output.as_ref(), Type::Tuple(tuple) if tuple.elems.is_empty())
        }
    };
    if !returns_unit {
        return Some(format!("{source_path}: system `{name}` must return ()"));
    }
    None
}

fn is_crate_visible(visibility: &syn::Visibility) -> bool {
    match visibility {
        syn::Visibility::Public(_) => true,
        syn::Visibility::Restricted(restricted) => restricted
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "crate"),
        syn::Visibility::Inherited => false,
    }
}

fn is_any_input_parameter(argument: &FnArg) -> bool {
    let FnArg::Typed(argument) = argument else {
        return false;
    };
    let Type::Path(path) = argument.ty.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "In")
}

fn entity_script_lifecycle(name: &str) -> Option<EntityScriptLifecycle> {
    match name {
        "start" | "on_start" => Some(EntityScriptLifecycle::Start),
        "update" | "on_update" => Some(EntityScriptLifecycle::Update),
        "fixed_update" | "on_fixed_update" => Some(EntityScriptLifecycle::FixedUpdate),
        "post_update" | "on_post_update" => Some(EntityScriptLifecycle::PostUpdate),
        _ => None,
    }
}

fn is_entity_input(argument: &FnArg) -> bool {
    let FnArg::Typed(argument) = argument else {
        return false;
    };
    let Type::Path(path) = argument.ty.as_ref() else {
        return false;
    };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if segment.ident != "In" {
        return false;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    arguments.args.iter().any(|argument| {
        matches!(argument, GenericArgument::Type(Type::Path(path)) if path.path.segments.last().is_some_and(|segment| segment.ident == "Entity"))
    })
}

fn parameter_summary(parameters: &[String], kinds: &[&str]) -> String {
    let values = parameters
        .iter()
        .filter(|parameter| {
            let short = parameter
                .split('<')
                .next()
                .unwrap_or(parameter)
                .rsplit("::")
                .next()
                .unwrap_or(parameter);
            kinds.contains(&short)
        })
        .cloned()
        .collect::<Vec<_>>();
    if values.is_empty() {
        "None".into()
    } else {
        values.join(", ")
    }
}

fn default_value(type_name: &str) -> Option<String> {
    let short = type_name.rsplit("::").next().unwrap_or(type_name);
    match short {
        "bool" => Some("false".into()),
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" => Some("0".into()),
        "f32" | "f64" => Some("0.0".into()),
        "String" => Some(String::new()),
        "Vec2" => Some("0.0, 0.0".into()),
        "Vec3" => Some("0.0, 0.0, 0.0".into()),
        "Vec4" => Some("0.0, 0.0, 0.0, 0.0".into()),
        "Color" => Some("1.0, 1.0, 1.0, 1.0".into()),
        _ => None,
    }
}

fn package_name(root: &Path) -> Option<String> {
    let source = fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let document = source.parse::<toml_edit::DocumentMut>().ok()?;
    document
        .get("package")?
        .get("name")?
        .as_str()
        .map(|name| name.replace('-', "_"))
}

fn module_path(source_root: &Path, path: &Path) -> Vec<String> {
    let relative = path.strip_prefix(source_root).unwrap_or(path);
    let mut parts: Vec<String> = relative
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect();
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !matches!(stem, "main" | "lib" | "mod") && !stem.is_empty() {
        parts.push(stem.to_owned());
    }
    parts
}

fn collect_rust_files(folder: &Path, output: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, output)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
    Ok(())
}

fn source_fingerprint(root: &Path) -> u64 {
    let mut files = Vec::new();
    let _ = collect_rust_files(&root.join("src"), &mut files);
    files.sort();
    files.into_iter().fold(0xcbf29ce484222325, |hash, path| {
        let metadata = fs::metadata(&path).ok();
        let modified = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos() as u64);
        let length = metadata.map_or(0, |metadata| metadata.len());
        hash.rotate_left(5) ^ modified ^ length
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_named_and_marker_components() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-temp")
            .join(format!("component-scan-{unique}"));
        fs::create_dir_all(root.join("src/world")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"sample-game\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/world/entities.rs"),
            r#"
                #[derive(Component, Debug)]
                pub struct Movement { pub velocity: Vec2, pub facing: i32 }
                #[derive(bevy::prelude::Component)]
                pub struct LocalPlayer;
                #[derive(Resource)]
                pub struct Settings;
                pub fn move_players(mut players: Query<&mut Movement>, time: Res<Time>) {}
                pub fn on_start(In(entity): In<Entity>, mut commands: Commands) {}
                pub fn update(In(entity): In<Entity>, time: Res<Time>) {}
            "#,
        )
        .unwrap();

        let components = discover_components(&root).unwrap();
        assert_eq!(components.len(), 2);
        assert_eq!(
            components[0].type_path,
            "sample_game::world::entities::LocalPlayer"
        );
        assert_eq!(components[1].fields[0].default_value, "0.0, 0.0");
        let systems = discover_systems(&root).unwrap();
        assert_eq!(systems.len(), 1);
        assert_eq!(
            systems[0].system_path,
            "sample_game::world::entities::move_players"
        );
        assert_eq!(systems[0].query_summary(), "Query<&mut Movement>");
        assert_eq!(systems[0].access_summary(), "Res<Time>");
        let scripts = discover_entity_scripts(&root).unwrap();
        assert_eq!(scripts.len(), 2);
        assert_eq!(scripts[0].lifecycle, EntityScriptLifecycle::Start);
        assert!(scripts.iter().all(|script| script.valid));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lifecycle_names_are_diagnosed_even_without_bevy_system_parameters() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-temp")
            .join(format!("entity-script-diagnostic-{unique}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"sample-game\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn update(entity: Entity) {}\n").unwrap();

        let scripts = discover_entity_scripts(&root).unwrap();
        assert_eq!(scripts.len(), 1);
        assert!(!scripts[0].valid);
        assert!(
            scripts[0]
                .diagnostic
                .as_deref()
                .is_some_and(|message| message.contains("must accept In<Entity>"))
        );
        assert!(discover_systems(&root).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nested_systems_require_crate_visibility_and_supported_signatures() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-temp")
            .join(format!("system-diagnostic-{unique}"));
        fs::create_dir_all(root.join("src/systems")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"sample-game\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/systems/player.rs"),
            r#"
                fn private_system(query: Query<Entity>) {}
                pub(crate) fn valid_system(query: Query<Entity>) {}
                pub(crate) async fn async_system(time: Res<Time>) {}
                pub(crate) fn input_system(entity: In<Entity>, time: Res<Time>) {}
                pub(crate) fn returning_system(time: Res<Time>) -> bool { true }
            "#,
        )
        .unwrap();

        let systems = discover_systems(&root).unwrap();
        assert_eq!(systems.len(), 5);
        assert!(
            systems
                .iter()
                .find(|system| system.name == "valid_system")
                .is_some_and(|system| system.valid)
        );
        for name in [
            "private_system",
            "async_system",
            "input_system",
            "returning_system",
        ] {
            assert!(
                systems
                    .iter()
                    .find(|system| system.name == name)
                    .is_some_and(|system| !system.valid && system.diagnostic.is_some())
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
