use super::{
    de::BsnDeserializeError,
    syntax::{parse_bsn, BsnDocument, BsnEntity, BsnParseError},
};
use crate::{
    CachedSceneAsset, ReflectedComponentTemplate, ResolveContext, ResolveSceneError, ResolvedScene,
    Scene, SceneDependencies, ScenePatch,
};
use bevy_asset::{io::Reader, AssetLoader, AssetPath, LoadContext};
use bevy_ecs::{
    hierarchy::ChildOf,
    name::Name,
    reflect::{AppTypeRegistry, ReflectComponent},
    world::{FromWorld, World},
};
use bevy_reflect::{
    serde::TypedReflectDeserializer, PartialReflect, ReflectDeserialize, TypePath, TypeRegistry,
    TypeRegistryArc,
};
use serde::de::DeserializeSeed;
use std::io;
use thiserror::Error;

/// Loads data-only Bevy Scene Notation files as spawnable [`ScenePatch`] assets.
#[derive(Debug, TypePath)]
pub struct BsnAssetLoader {
    type_registry: TypeRegistryArc,
}

impl FromWorld for BsnAssetLoader {
    fn from_world(world: &mut World) -> Self {
        Self {
            type_registry: world.resource::<AppTypeRegistry>().0.clone(),
        }
    }
}

/// An error produced while loading a runtime `.bsn` asset.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BsnLoadError {
    /// Reading the asset failed.
    #[error("could not read BSN asset: {0}")]
    Io(#[from] io::Error),
    /// The file was not valid UTF-8.
    #[error("BSN assets must be UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    /// Tokenizing or parsing failed.
    #[error(transparent)]
    Parse(#[from] BsnParseError),
    /// A reflected component could not be constructed.
    #[error("component `{type_path}` at {line}:{column}: {reason}")]
    Component {
        /// Component type path from the file.
        type_path: String,
        /// One-based line.
        line: usize,
        /// One-based column.
        column: usize,
        /// Detailed reflection error.
        reason: String,
    },
}

impl AssetLoader for BsnAssetLoader {
    type Asset = ScenePatch;
    type Settings = ();
    type Error = BsnLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let source = String::from_utf8(bytes)?;
        let document = parse_bsn(&source)?;
        let runtime_scene = {
            let registry = self.type_registry.read();
            runtime_scene_from_document(document, &registry, self.type_registry.clone())?
        };
        Ok(ScenePatch::load_with(load_context, runtime_scene))
    }

    fn extensions(&self) -> &[&str] {
        &["bsn"]
    }
}

struct RuntimeBsnScene {
    name: Option<Name>,
    cached_scene: Option<AssetPath<'static>>,
    components: Vec<ReflectedComponentTemplate>,
    children: Vec<RuntimeBsnScene>,
}

impl Scene for RuntimeBsnScene {
    fn resolve(
        self,
        context: &mut ResolveContext,
        scene: &mut ResolvedScene,
    ) -> Result<(), ResolveSceneError> {
        if let Some(cached_scene) = self.cached_scene {
            CachedSceneAsset(cached_scene).resolve(context, scene)?;
        }
        if let Some(name) = self.name {
            *scene.get_or_insert_template::<Name>(context) = name;
        }
        for component in self.components {
            scene.push_template_erased(Box::new(component));
        }
        if !self.children.is_empty() {
            let related = scene.get_or_insert_related_resolved_scenes::<ChildOf>();
            for child in self.children {
                let mut resolved_child = ResolvedScene::default();
                child.resolve(context, &mut resolved_child)?;
                related.scenes.push(resolved_child);
            }
        }
        Ok(())
    }

    fn register_dependencies(&self, dependencies: &mut SceneDependencies) {
        if let Some(cached_scene) = &self.cached_scene {
            dependencies.register::<ScenePatch>(cached_scene.clone());
        }
        for child in &self.children {
            child.register_dependencies(dependencies);
        }
    }
}

struct RuntimeComponent {
    type_id: core::any::TypeId,
    type_path: String,
    value: Box<dyn PartialReflect>,
    reflect_component: ReflectComponent,
}

fn runtime_scene_from_document(
    document: BsnDocument,
    registry: &TypeRegistry,
    registry_arc: TypeRegistryArc,
) -> Result<RuntimeBsnScene, BsnLoadError> {
    runtime_scene_from_entity(document.root, registry, registry_arc)
}

fn runtime_scene_from_entity(
    entity: BsnEntity,
    registry: &TypeRegistry,
    registry_arc: TypeRegistryArc,
) -> Result<RuntimeBsnScene, BsnLoadError> {
    let mut components: Vec<RuntimeComponent> = Vec::new();
    for component in entity.components {
        let registration = registry
            .get_with_type_path(&component.type_path)
            .or_else(|| registry.get_with_short_type_path(&component.type_path))
            .ok_or_else(|| {
                component_error(
                    &component,
                    "type is not registered or its short path is ambiguous",
                )
            })?;
        let reflect_component = registration
            .data::<ReflectComponent>()
            .cloned()
            .ok_or_else(|| {
                component_error(
                    &component,
                    "type is registered but does not expose `#[reflect(Component)]`",
                )
            })?;
        let value_source = component.body.as_value();
        let deserialize_source = match (&component.body, registration.data::<ReflectDeserialize>())
        {
            (super::syntax::BsnComponentBody::Tuple(values), Some(_)) if values.len() == 1 => {
                &values[0]
            }
            _ => &value_source,
        };
        let value = TypedReflectDeserializer::new(registration, registry)
            .deserialize(deserialize_source)
            .map_err(|error: BsnDeserializeError| component_error(&component, error.to_string()))?;

        if let Some(existing) = components
            .iter_mut()
            .find(|existing| existing.type_id == registration.type_id())
        {
            existing.value.try_apply(&*value).map_err(|error| {
                component_error(
                    &component,
                    format!("could not merge repeated component patch: {error}"),
                )
            })?;
        } else {
            components.push(RuntimeComponent {
                type_id: registration.type_id(),
                type_path: registration.type_info().type_path().to_string(),
                value,
                reflect_component,
            });
        }
    }

    let components = components
        .into_iter()
        .map(|component| {
            let _ = component.type_path;
            ReflectedComponentTemplate::new(
                component.value,
                component.reflect_component,
                registry_arc.clone(),
            )
        })
        .collect();
    let children = entity
        .children
        .into_iter()
        .map(|child| runtime_scene_from_entity(child, registry, registry_arc.clone()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(RuntimeBsnScene {
        name: entity.name.map(Name::new),
        cached_scene: entity.cached_scene.map(AssetPath::from),
        components,
        children,
    })
}

fn component_error(
    component: &crate::bsn::BsnComponent,
    reason: impl Into<String>,
) -> BsnLoadError {
    BsnLoadError::Component {
        type_path: component.type_path.clone(),
        line: component.span.line,
        column: component.span.column,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScenePlugin;
    use bevy_app::{App, TaskPoolPlugin};
    use bevy_asset::{AssetPlugin, AssetServer, Assets};
    use bevy_ecs::{component::Component, reflect::AppTypeRegistry};
    use bevy_reflect::{prelude::ReflectDefault, Reflect};

    #[derive(Component, Reflect, Default, Debug, PartialEq)]
    #[reflect(Component, Default)]
    struct RuntimeBsnTest {
        value: i32,
        enabled: bool,
    }

    #[test]
    fn reflected_runtime_component_spawns_through_scene_patch() {
        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            AssetPlugin::default(),
            ScenePlugin,
        ));
        app.register_type::<RuntimeBsnTest>();

        let document = parse_bsn(
            "Name(\"Visible Root\") RuntimeBsnTest { value: 42, enabled: true } Children [(#Child)]",
        )
        .unwrap();
        let registry_arc = app.world().resource::<AppTypeRegistry>().0.clone();
        let runtime = {
            let registry = registry_arc.read();
            runtime_scene_from_document(document, &registry, registry_arc.clone()).unwrap()
        };
        let asset_server = app.world().resource::<AssetServer>().clone();
        let mut patch = ScenePatch::load(&asset_server, runtime);
        app.world_mut().resource_scope(
            |world, patches: bevy_ecs::change_detection::Mut<Assets<ScenePatch>>| {
                patch.resolve(&asset_server, &patches).unwrap();
                let entity = patch.spawn(world).unwrap().id();
                assert_eq!(
                    world.get::<RuntimeBsnTest>(entity),
                    Some(&RuntimeBsnTest {
                        value: 42,
                        enabled: true,
                    })
                );
                assert_eq!(world.get::<Name>(entity).unwrap().as_str(), "Visible Root");
                assert_eq!(
                    world
                        .get::<bevy_ecs::hierarchy::Children>(entity)
                        .unwrap()
                        .len(),
                    1
                );
            },
        );
    }

    #[test]
    fn missing_component_registration_is_reported_at_source_location() {
        let registry = TypeRegistry::default();
        let registry_arc = TypeRegistryArc::default();
        let document = parse_bsn("\n  MissingComponent").unwrap();
        let error = match runtime_scene_from_document(document, &registry, registry_arc) {
            Ok(_) => panic!("missing component registration should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("2:3"));
        assert!(error.to_string().contains("not registered"));
    }
}
