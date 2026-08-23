//! Revy entity presets built on top of Bevy's ECS components.

use arisna_engine::{
    FbxLoaderSettings, SceneCustomComponent, SceneEntityScript, SceneSystemBinding,
};
pub use arisna_engine::{
    SceneAnimationPlayer, SceneCollisionRect2D, SceneModel3D, SceneSprite2D, SceneUiContent,
    SceneUiLayout, UiAlignment, scene_model_asset_path,
};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot};

use crate::{
    hierarchy::{SceneNodeId, SceneParentId, SceneSiblingOrder},
    selection::EditableObject,
    workspace::SceneSpace,
};

/// The editor-facing type of a scene entity.
///
/// These are presets, not an inheritance hierarchy. Each preset adds the
/// Bevy components that make the entity useful immediately after creation.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EntityKind {
    #[default]
    Empty,
    AnimationPlayer,
    Empty2D,
    CollisionRect2D,
    Empty3D,
    Sprite2D,
    Camera2D,
    Mesh3D,
    Camera3D,
    DirectionalLight3D,
    PointLight3D,
    SpotLight3D,
    EmptyUi,
    Panel,
    Text,
    Button,
    Image,
}

/// Bevy-native components that can be added independently of an entity preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinComponent {
    Transform,
    Sprite,
    Camera2D,
    Mesh3D,
    Camera3D,
    DirectionalLight3D,
    PointLight3D,
    SpotLight3D,
}

impl BuiltinComponent {
    pub const ALL: [Self; 8] = [
        Self::Transform,
        Self::Sprite,
        Self::Camera2D,
        Self::Mesh3D,
        Self::Camera3D,
        Self::DirectionalLight3D,
        Self::PointLight3D,
        Self::SpotLight3D,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Transform => "Transform + Visibility",
            Self::Sprite => "Sprite",
            Self::Camera2D => "Camera2D",
            Self::Mesh3D => "Mesh3D + StandardMaterial",
            Self::Camera3D => "Camera3D",
            Self::DirectionalLight3D => "DirectionalLight3D",
            Self::PointLight3D => "PointLight3D",
            Self::SpotLight3D => "SpotLight3D",
        }
    }

    pub const fn scene_name(self) -> &'static str {
        match self {
            Self::Transform => "transform",
            Self::Sprite => "sprite",
            Self::Camera2D => "camera2d",
            Self::Mesh3D => "mesh3d",
            Self::Camera3D => "camera3d",
            Self::DirectionalLight3D => "directional_light3d",
            Self::PointLight3D => "point_light3d",
            Self::SpotLight3D => "spot_light3d",
        }
    }

    pub fn from_scene_name(value: &str) -> Result<Self, String> {
        match value {
            "transform" => Ok(Self::Transform),
            "sprite" => Ok(Self::Sprite),
            "camera2d" => Ok(Self::Camera2D),
            "mesh3d" => Ok(Self::Mesh3D),
            "camera3d" => Ok(Self::Camera3D),
            "directional_light3d" => Ok(Self::DirectionalLight3D),
            "point_light3d" => Ok(Self::PointLight3D),
            "spot_light3d" => Ok(Self::SpotLight3D),
            _ => Err(format!("unsupported entity component: {value}")),
        }
    }

    pub const fn supports(self, space: SceneSpace) -> bool {
        match self {
            Self::Transform => true,
            Self::Sprite | Self::Camera2D => matches!(space, SceneSpace::TwoD),
            Self::Mesh3D
            | Self::Camera3D
            | Self::DirectionalLight3D
            | Self::PointLight3D
            | Self::SpotLight3D => matches!(space, SceneSpace::ThreeD),
        }
    }
}

/// Extra components authored through the Inspector, excluding preset defaults.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct AddedEntityComponents(pub Vec<BuiltinComponent>);

/// Project-defined Rust components authored on this editor entity.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityCustomComponents(pub Vec<SceneCustomComponent>);

#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityScriptBinding(pub Option<SceneEntityScript>);

/// Systems explicitly bound to this entity in the Systems Inspector tab.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct EntitySystemBindings(pub Vec<SceneSystemBinding>);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NeedsDefaultMesh3D;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NeedsModel3dFocus;

impl EntityKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::AnimationPlayer => "AnimationPlayer",
            Self::Empty2D => "Empty2D",
            Self::CollisionRect2D => "CollisionRect2D",
            Self::Empty3D => "Empty3D",
            Self::Sprite2D => "Sprite2D",
            Self::Camera2D => "Camera2D",
            Self::Mesh3D => "Mesh3D",
            Self::Camera3D => "Camera3D",
            Self::DirectionalLight3D => "DirectionalLight3D",
            Self::PointLight3D => "PointLight3D",
            Self::SpotLight3D => "SpotLight3D",
            Self::EmptyUi => "EmptyUI",
            Self::Panel => "Panel",
            Self::Text => "Text",
            Self::Button => "Button",
            Self::Image => "Image",
        }
    }

    /// Shared icon used everywhere an entity type appears in the editor.
    pub const fn icon_path(self) -> &'static str {
        match self {
            Self::Empty | Self::EmptyUi => "editor/icons/node-circle.png",
            Self::AnimationPlayer => "editor/icons/play.png",
            Self::Empty2D => "editor/icons/axis-2d.png",
            Self::CollisionRect2D => "editor/icons/box.png",
            Self::Empty3D | Self::Mesh3D => "editor/icons/box.png",
            Self::Sprite2D | Self::Image => "editor/icons/image.png",
            Self::Camera2D | Self::Camera3D => "editor/icons/camera.png",
            Self::DirectionalLight3D => "editor/icons/sun.png",
            Self::PointLight3D => "editor/icons/lightbulb.png",
            Self::SpotLight3D => "editor/icons/flashlight.png",
            Self::Panel => "editor/icons/panel-bottom-open.png",
            Self::Text => "editor/icons/type.png",
            Self::Button => "editor/icons/square.png",
        }
    }

    pub fn icon_color(self) -> Color {
        match self {
            Self::Empty => Color::srgb(0.82, 0.83, 0.86),
            Self::AnimationPlayer => Color::srgb(0.32, 0.82, 0.72),
            Self::Empty2D | Self::Sprite2D | Self::Camera2D => Color::srgb(0.25, 0.52, 1.0),
            Self::CollisionRect2D => Color::srgb(0.95, 0.30, 0.32),
            Self::Empty3D | Self::Camera3D => Color::srgb(1.0, 0.32, 0.38),
            Self::Mesh3D => Color::srgb(0.95, 0.60, 0.24),
            Self::DirectionalLight3D | Self::PointLight3D | Self::SpotLight3D => {
                Color::srgb(1.0, 0.78, 0.23)
            }
            Self::EmptyUi | Self::Panel | Self::Text | Self::Button | Self::Image => {
                Color::srgb(0.42, 0.80, 0.58)
            }
        }
    }

    pub const fn scene_kind(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::AnimationPlayer => "animation_player",
            Self::Empty2D => "empty2d",
            Self::CollisionRect2D => "collision_rect2d",
            Self::Empty3D => "empty3d",
            Self::Sprite2D => "sprite2d",
            Self::Camera2D => "camera2d",
            Self::Mesh3D => "mesh3d",
            Self::Camera3D => "camera3d",
            Self::DirectionalLight3D => "directional_light3d",
            Self::PointLight3D => "point_light3d",
            Self::SpotLight3D => "spot_light3d",
            Self::EmptyUi => "empty_ui",
            Self::Panel => "panel",
            Self::Text => "text",
            Self::Button => "button",
            Self::Image => "image",
        }
    }

    pub fn from_scene_kind(value: &str) -> Result<Self, String> {
        match value {
            // v2 scene files used these two values for their root entities.
            "2d" | "empty2d" => Ok(Self::Empty2D),
            "collision_rect2d" => Ok(Self::CollisionRect2D),
            "3d" | "empty3d" => Ok(Self::Empty3D),
            "empty" => Ok(Self::Empty),
            "animation_player" => Ok(Self::AnimationPlayer),
            "sprite2d" => Ok(Self::Sprite2D),
            "camera2d" => Ok(Self::Camera2D),
            "mesh3d" => Ok(Self::Mesh3D),
            "camera3d" => Ok(Self::Camera3D),
            "directional_light3d" => Ok(Self::DirectionalLight3D),
            "point_light3d" => Ok(Self::PointLight3D),
            "spot_light3d" => Ok(Self::SpotLight3D),
            "empty_ui" => Ok(Self::EmptyUi),
            "panel" => Ok(Self::Panel),
            "text" => Ok(Self::Text),
            "button" => Ok(Self::Button),
            "image" => Ok(Self::Image),
            _ => Err(format!("unsupported entity kind: {value}")),
        }
    }

    pub const fn default_name(self) -> &'static str {
        self.label()
    }

    pub const fn is_spatial(self) -> bool {
        !matches!(
            self,
            Self::Empty
                | Self::AnimationPlayer
                | Self::EmptyUi
                | Self::Panel
                | Self::Text
                | Self::Button
                | Self::Image
        )
    }

    pub const fn is_ui(self) -> bool {
        matches!(
            self,
            Self::EmptyUi | Self::Panel | Self::Text | Self::Button | Self::Image
        )
    }

    pub const fn default_space(self) -> Option<SceneSpace> {
        match self {
            Self::Empty3D
            | Self::Mesh3D
            | Self::Camera3D
            | Self::DirectionalLight3D
            | Self::PointLight3D
            | Self::SpotLight3D => Some(SceneSpace::ThreeD),
            Self::Empty2D
            | Self::CollisionRect2D
            | Self::Sprite2D
            | Self::Camera2D
            | Self::EmptyUi
            | Self::Panel
            | Self::Text
            | Self::Button
            | Self::Image => Some(SceneSpace::TwoD),
            Self::Empty | Self::AnimationPlayer => None,
        }
    }

    pub fn default_ui_layout(self) -> Option<SceneUiLayout> {
        match self {
            Self::EmptyUi => Some(SceneUiLayout::sized(320.0, 180.0)),
            Self::Panel => Some(SceneUiLayout::sized(240.0, 160.0)),
            Self::Text => Some(SceneUiLayout::sized(160.0, 32.0)),
            Self::Button => Some(SceneUiLayout::sized(120.0, 36.0)),
            Self::Image => Some(SceneUiLayout::sized(128.0, 128.0)),
            _ => None,
        }
    }

    pub fn default_ui_content(self) -> Option<SceneUiContent> {
        let mut content = SceneUiContent::default();
        match self {
            Self::Text => content.text = "Text".into(),
            Self::Button => {
                content.text = "Button".into();
                content.panel_color = (0.22, 0.42, 0.72, 1.0);
            }
            Self::Image => content.panel_color = (0.32, 0.34, 0.40, 1.0),
            Self::EmptyUi | Self::Panel => {}
            _ => return None,
        }
        Some(content)
    }
}

/// Adds the common editor metadata and the Bevy defaults for an entity preset.
/// `space` is inherited from the parent for the logical `Empty` preset.
#[allow(clippy::too_many_arguments)]
pub fn spawn_entity(
    commands: &mut Commands,
    kind: EntityKind,
    name: String,
    id: SceneNodeId,
    parent: Option<SceneNodeId>,
    order: u32,
    space: SceneSpace,
) -> Entity {
    let entity = commands
        .spawn((
            EditableObject { name },
            kind,
            AddedEntityComponents::default(),
            EntityCustomComponents::default(),
            EntityScriptBinding::default(),
            EntitySystemBindings::default(),
            space,
            id,
            SceneParentId(parent),
            SceneSiblingOrder(order),
        ))
        .id();

    if kind.is_spatial() {
        commands
            .entity(entity)
            .insert((Transform::default(), Visibility::Visible));
    }
    if let Some(layout) = kind.default_ui_layout() {
        commands.entity(entity).insert(layout);
    }
    if let Some(content) = kind.default_ui_content() {
        commands.entity(entity).insert(content);
    }
    if kind == EntityKind::AnimationPlayer {
        commands
            .entity(entity)
            .insert(SceneAnimationPlayer::default());
    }

    match kind {
        EntityKind::Sprite2D => {
            // A sized sprite gives the editor a useful selection box before a texture is set.
            commands.entity(entity).insert((
                SceneSprite2D::default(),
                Sprite::sized(Vec2::splat(64.0)),
                Anchor::TOP_LEFT,
            ));
        }
        EntityKind::CollisionRect2D => {
            let collision = SceneCollisionRect2D::default();
            commands.entity(entity).insert((
                collision,
                SceneSprite2D {
                    image_path: String::new(),
                    color: (0.92, 0.20, 0.24, 0.25),
                    flip_x: false,
                    flip_y: false,
                    hframes: 1,
                    vframes: 1,
                    frame: 0,
                    visible: true,
                    region_enabled: false,
                    region_rect: (0.0, 0.0, collision.size.0, collision.size.1),
                    z_index: 100,
                    anchor: (0.0, 0.0),
                },
                Sprite::from_color(
                    Color::srgba(0.92, 0.20, 0.24, 0.25),
                    Vec2::new(collision.size.0, collision.size.1),
                ),
                Anchor::TOP_LEFT,
            ));
        }
        EntityKind::Camera2D => {
            commands.entity(entity).insert((
                Camera2d,
                Camera {
                    is_active: false,
                    ..default()
                },
            ));
        }
        EntityKind::Mesh3D => {
            commands.entity(entity).insert(NeedsDefaultMesh3D);
        }
        EntityKind::Camera3D => {
            commands.entity(entity).insert((
                Camera3d::default(),
                Camera {
                    is_active: false,
                    ..default()
                },
            ));
        }
        EntityKind::DirectionalLight3D => {
            commands.entity(entity).insert(DirectionalLight::default());
        }
        EntityKind::PointLight3D => {
            commands.entity(entity).insert(PointLight::default());
        }
        EntityKind::SpotLight3D => {
            commands.entity(entity).insert(SpotLight::default());
        }
        EntityKind::Empty
        | EntityKind::AnimationPlayer
        | EntityKind::Empty2D
        | EntityKind::Empty3D
        | EntityKind::EmptyUi
        | EntityKind::Panel
        | EntityKind::Text
        | EntityKind::Button
        | EntityKind::Image => {}
    }

    entity
}

/// Inserts one Inspector-authored Bevy component with useful editor defaults.
pub fn insert_builtin_component(
    commands: &mut Commands,
    entity: Entity,
    component: BuiltinComponent,
) {
    match component {
        BuiltinComponent::Transform => {
            commands
                .entity(entity)
                .insert((Transform::default(), Visibility::Visible));
        }
        BuiltinComponent::Sprite => {
            commands.entity(entity).insert((
                SceneSprite2D::default(),
                Sprite::sized(Vec2::splat(64.0)),
                Anchor::TOP_LEFT,
            ));
        }
        BuiltinComponent::Camera2D => {
            commands.entity(entity).insert((
                Camera2d,
                Camera {
                    is_active: false,
                    ..default()
                },
            ));
        }
        BuiltinComponent::Mesh3D => {
            commands.entity(entity).insert(NeedsDefaultMesh3D);
        }
        BuiltinComponent::Camera3D => {
            commands.entity(entity).insert((
                Camera3d::default(),
                Camera {
                    is_active: false,
                    ..default()
                },
            ));
        }
        BuiltinComponent::DirectionalLight3D => {
            commands.entity(entity).insert(DirectionalLight::default());
        }
        BuiltinComponent::PointLight3D => {
            commands.entity(entity).insert(PointLight::default());
        }
        BuiltinComponent::SpotLight3D => {
            commands.entity(entity).insert(SpotLight::default());
        }
    }
}

/// Removes one Inspector-authored component bundle.
///
/// The Inspector only exposes this for entries in [`AddedEntityComponents`],
/// so preset-owned defaults stay intact and are recreated consistently when a
/// scene is loaded or an edit is undone.
pub fn remove_builtin_component(
    commands: &mut Commands,
    entity: Entity,
    component: BuiltinComponent,
) {
    match component {
        BuiltinComponent::Transform => {
            commands
                .entity(entity)
                .remove::<Transform>()
                .remove::<Visibility>();
        }
        BuiltinComponent::Sprite => {
            commands
                .entity(entity)
                .remove::<Sprite>()
                .remove::<SceneSprite2D>()
                .remove::<Anchor>();
        }
        BuiltinComponent::Camera2D => {
            commands
                .entity(entity)
                .remove::<Camera2d>()
                .remove::<Camera>();
        }
        BuiltinComponent::Mesh3D => {
            commands
                .entity(entity)
                .remove::<NeedsDefaultMesh3D>()
                .remove::<NeedsModel3dFocus>()
                .remove::<Mesh3d>()
                .remove::<MeshMaterial3d<StandardMaterial>>();
        }
        BuiltinComponent::Camera3D => {
            commands
                .entity(entity)
                .remove::<Camera3d>()
                .remove::<Camera>();
        }
        BuiltinComponent::DirectionalLight3D => {
            commands.entity(entity).remove::<DirectionalLight>();
        }
        BuiltinComponent::PointLight3D => {
            commands.entity(entity).remove::<PointLight>();
        }
        BuiltinComponent::SpotLight3D => {
            commands.entity(entity).remove::<SpotLight>();
        }
    }
}

/// Allocates resources for presets that need asset handles after commands flush.
pub fn ensure_entity_defaults(
    mut commands: Commands,
    entities: Query<(
        Entity,
        &EntityKind,
        Option<&Mesh3d>,
        Has<NeedsDefaultMesh3D>,
    )>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let (Some(meshes), Some(materials)) = (meshes.as_deref_mut(), materials.as_deref_mut()) else {
        return;
    };
    for (entity, kind, mesh, needs_mesh) in &entities {
        if (*kind != EntityKind::Mesh3D && !needs_mesh) || mesh.is_some() {
            continue;
        }
        commands.entity(entity).insert((
            Mesh3d(meshes.add(Cuboid::default())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.28, 0.58, 0.88),
                ..default()
            })),
        ));
        commands.entity(entity).remove::<NeedsDefaultMesh3D>();
    }
}

pub fn sync_model3d_assets(
    mut commands: Commands,
    asset_server: Option<Res<AssetServer>>,
    models: Query<(Entity, &EntityKind, &SceneModel3D), Changed<SceneModel3D>>,
) {
    let Some(asset_server) = asset_server else {
        return;
    };
    for (entity, kind, model) in &models {
        let Some(asset_path) = scene_model_asset_path(&model.resource_path) else {
            commands
                .entity(entity)
                .remove::<WorldAssetRoot>()
                .remove::<NeedsModel3dFocus>();
            if *kind == EntityKind::Mesh3D {
                commands.entity(entity).insert(NeedsDefaultMesh3D);
            }
            continue;
        };
        commands
            .entity(entity)
            .remove::<NeedsDefaultMesh3D>()
            .remove::<Mesh3d>()
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((
                WorldAssetRoot(load_model_scene(
                    &asset_server,
                    format!("project://{asset_path}"),
                )),
                NeedsModel3dFocus,
            ));
    }
}

fn load_model_scene(asset_server: &AssetServer, asset_path: String) -> Handle<WorldAsset> {
    if asset_path
        .split('#')
        .next()
        .is_some_and(|path| path.ends_with(".fbx"))
    {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut FbxLoaderSettings| {
                settings.load_auxiliary_geometry = false;
            })
            .load::<WorldAsset>(asset_path)
    } else {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut bevy::gltf::GltfLoaderSettings| {
                settings.load_cameras = false;
                settings.load_lights = false;
                settings.load_main_model_only = true;
            })
            .load(asset_path)
    }
}

/// Removes a preset's placeholder cube even when it was queued in the same
/// frame as a model assignment. The default mesh and model asset are spawned
/// through deferred commands, so a one-shot `Changed<SceneModel3D>` query can
/// otherwise observe the model before the placeholder exists.
pub fn clear_model3d_placeholders(
    mut commands: Commands,
    models: Query<(
        Entity,
        &SceneModel3D,
        Option<&Mesh3d>,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
) {
    for (entity, model, mesh, material) in &models {
        if scene_model_asset_path(&model.resource_path).is_none() {
            continue;
        }
        let mut entity_commands = commands.entity(entity);
        if mesh.is_some() {
            entity_commands.remove::<Mesh3d>();
        }
        if material.is_some() {
            entity_commands.remove::<MeshMaterial3d<StandardMaterial>>();
        }
    }
}
