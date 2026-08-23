//! 编辑器和游戏运行时共用的动画取样器。
//!
//! 动画轨道只保存稳定场景节点 ID 和可移植字符串值。本模块负责把字符串值
//! 还原为 Bevy Transform，并保证编辑器预览与游戏运行时使用完全相同的插值。

use bevy::prelude::*;

use crate::scene::{
    runtime_ui_node, scene_ui_transform, GamePaused, RuntimeSceneNode, SceneAnimationKey,
    SceneAnimationPlayer, SceneAnimationTrackKind, SceneSprite2D, SceneUiLayout,
};

/// 游戏运行时中某个 AnimationPlayer 的播放游标。
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct RuntimeAnimationPlayback {
    pub clip_name: String,
    pub time: f32,
    pub playing: bool,
}

/// 把 Transform 编码为 `.bsn` 动画关键帧使用的稳定文本格式。
pub fn format_animation_transform(transform: &Transform) -> String {
    format!(
        "translation:{:.4},{:.4},{:.4};rotation:{:.6},{:.6},{:.6},{:.6};scale:{:.4},{:.4},{:.4}",
        transform.translation.x,
        transform.translation.y,
        transform.translation.z,
        transform.rotation.x,
        transform.rotation.y,
        transform.rotation.z,
        transform.rotation.w,
        transform.scale.x,
        transform.scale.y,
        transform.scale.z,
    )
}

/// 解析动画 Transform。未知字段会被忽略，但三个必需字段必须完整存在。
pub fn parse_animation_transform(value: &str) -> Result<Transform, String> {
    let mut translation = None;
    let mut rotation = None;
    let mut scale = None;

    for field in value.split(';') {
        let Some((name, values)) = field.split_once(':') else {
            continue;
        };
        match name.trim() {
            "translation" => translation = Some(parse_vec3(values, "translation")?),
            "rotation" => rotation = Some(parse_quat(values)?),
            "scale" => scale = Some(parse_vec3(values, "scale")?),
            _ => {}
        }
    }

    Ok(Transform {
        translation: translation
            .ok_or_else(|| "animation transform has no translation".to_string())?,
        rotation: rotation.ok_or_else(|| "animation transform has no rotation".to_string())?,
        scale: scale.ok_or_else(|| "animation transform has no scale".to_string())?,
    })
}

/// 在指定时间对 Transform 关键帧取样。
///
/// 无需调用方预先排序：早于首帧或晚于末帧时分别保持首尾关键帧，损坏的
/// 关键帧会被跳过，避免一个手工编辑错误使整条动画停止。
pub fn sample_animation_transform(keys: &[SceneAnimationKey], time: f32) -> Option<Transform> {
    let time = time.max(0.0);
    let mut before: Option<(f32, Transform)> = None;
    let mut after: Option<(f32, Transform)> = None;

    for key in keys {
        let Ok(transform) = parse_animation_transform(&key.value) else {
            continue;
        };
        if key.time <= time
            && before
                .as_ref()
                .is_none_or(|(candidate, _)| key.time >= *candidate)
        {
            before = Some((key.time, transform));
        } else if key.time > time
            && after
                .as_ref()
                .is_none_or(|(candidate, _)| key.time < *candidate)
        {
            after = Some((key.time, transform));
        }
    }

    match (before, after) {
        (Some((left_time, left)), Some((right_time, right))) => {
            let duration = right_time - left_time;
            let factor = if duration <= f32::EPSILON {
                0.0
            } else {
                ((time - left_time) / duration).clamp(0.0, 1.0)
            };
            Some(Transform {
                translation: left.translation.lerp(right.translation, factor),
                rotation: left.rotation.slerp(right.rotation, factor),
                scale: left.scale.lerp(right.scale, factor),
            })
        }
        (Some((_, transform)), None) | (None, Some((_, transform))) => Some(transform),
        (None, None) => None,
    }
}

/// Encodes a Sprite2D frame as stable `.bsn` animation text.
pub fn format_sprite_frame(frame: u32) -> String {
    frame.to_string()
}

/// Parses a zero-based Sprite2D frame index.
pub fn parse_sprite_frame(value: &str) -> Result<u32, String> {
    value
        .trim()
        .parse()
        .map_err(|error| format!("invalid sprite frame: {error}"))
}

/// Samples SpriteFrame tracks with step interpolation.
pub fn sample_sprite_frame(keys: &[SceneAnimationKey], time: f32) -> Option<u32> {
    let time = time.max(0.0);
    let mut before: Option<(f32, u32)> = None;
    let mut first_after: Option<(f32, u32)> = None;
    for key in keys {
        let Ok(frame) = parse_sprite_frame(&key.value) else {
            continue;
        };
        if key.time <= time
            && before
                .as_ref()
                .is_none_or(|(candidate, _)| key.time >= *candidate)
        {
            before = Some((key.time, frame));
        } else if key.time > time
            && first_after
                .as_ref()
                .is_none_or(|(candidate, _)| key.time < *candidate)
        {
            first_after = Some((key.time, frame));
        }
    }
    before.or(first_after).map(|(_, frame)| frame)
}

/// AnimationPlayer 新增或热重载后，根据 autoplay 初始化运行时游标。
pub(crate) fn initialize_runtime_animation_players(
    mut commands: Commands,
    players: Query<(Entity, &SceneAnimationPlayer), Changed<SceneAnimationPlayer>>,
) {
    for (entity, player) in &players {
        let autoplay = player.autoplay.trim();
        if autoplay.is_empty() {
            commands.entity(entity).remove::<RuntimeAnimationPlayback>();
            continue;
        }
        let Some(clip) = player.clips.iter().find(|clip| clip.name == autoplay) else {
            warn!(
                "AnimationPlayer autoplay clip `{autoplay}` does not exist; playback was skipped"
            );
            commands.entity(entity).remove::<RuntimeAnimationPlayback>();
            continue;
        };
        commands.entity(entity).insert(RuntimeAnimationPlayback {
            clip_name: clip.name.clone(),
            time: 0.0,
            playing: true,
        });
    }
}

/// 按游戏时间推进所有运行时播放器；暂停游戏时动画时间也保持不变。
pub(crate) fn advance_runtime_animation_players(
    time: Res<Time>,
    paused: Res<GamePaused>,
    mut players: Query<(&SceneAnimationPlayer, &mut RuntimeAnimationPlayback)>,
) {
    if paused.0 {
        return;
    }
    for (player, mut playback) in &mut players {
        if !playback.playing {
            continue;
        }
        let Some(clip) = player
            .clips
            .iter()
            .find(|clip| clip.name == playback.clip_name)
        else {
            playback.playing = false;
            continue;
        };
        let length = clip.length.max(0.0);
        if length <= f32::EPSILON {
            playback.time = 0.0;
            playback.playing = false;
            continue;
        }
        playback.time += time.delta_secs() * player.speed.max(0.0);
        if playback.time >= length {
            if clip.looped {
                playback.time %= length;
            } else {
                playback.time = length;
                playback.playing = false;
            }
        }
    }
}

/// 对当前播放时间取样并写入真实 Bevy 组件。
pub(crate) fn apply_runtime_animations(
    players: Query<(&SceneAnimationPlayer, &RuntimeAnimationPlayback)>,
    mut targets: Query<(
        &RuntimeSceneNode,
        Option<&mut Transform>,
        Option<&mut SceneUiLayout>,
        Option<&mut Node>,
        Option<&mut UiTransform>,
        Option<&mut SceneSprite2D>,
    )>,
) {
    for (player, playback) in &players {
        let Some(clip) = player
            .clips
            .iter()
            .find(|clip| clip.name == playback.clip_name)
        else {
            continue;
        };
        for track in &clip.tracks {
            let Some((_, transform, ui_layout, node, ui_transform, sprite)) = targets
                .iter_mut()
                .find(|(target, ..)| target.id == track.target_node)
            else {
                continue;
            };
            if matches!(track.kind, SceneAnimationTrackKind::SpriteFrame) {
                let (Some(frame), Some(mut sprite)) =
                    (sample_sprite_frame(&track.keys, playback.time), sprite)
                else {
                    continue;
                };
                sprite.frame = frame.min(sprite.frame_count().saturating_sub(1));
                continue;
            }
            if !matches!(track.kind, SceneAnimationTrackKind::Transform) {
                continue;
            }
            let Some(sampled) = sample_animation_transform(&track.keys, playback.time) else {
                continue;
            };
            if let Some(mut transform) = transform {
                apply_sample_to_transform_property(&sampled, &track.property, &mut transform);
                continue;
            }
            let Some(mut layout) = ui_layout else {
                continue;
            };
            apply_sample_to_ui_layout_property(&sampled, &track.property, &mut layout);
            let layout = *layout;
            if let Some(mut node) = node {
                *node = runtime_ui_node(layout);
            }
            if let Some(mut ui_transform) = ui_transform {
                *ui_transform = scene_ui_transform(layout);
            }
        }
    }
}

/// 把 Transform 动画样本应用到指定属性。旧场景中的 `transform` 仍然更新整组值，
/// Inspector 新建的子轨道则只更新自己的属性，允许 Position、Rotation、Scale 并存。
pub fn apply_sample_to_transform_property(
    sample: &Transform,
    property: &str,
    transform: &mut Transform,
) {
    match property {
        "transform.position" => transform.translation = sample.translation,
        "transform.rotation" => transform.rotation = sample.rotation,
        "transform.scale" => transform.scale = sample.scale,
        _ => *transform = *sample,
    }
}

/// UI 的 Position 使用左上角像素偏移，Rotation 使用 Z 轴角度。
pub fn apply_sample_to_ui_layout(sample: &Transform, layout: &mut SceneUiLayout) {
    layout.offset = (sample.translation.x, sample.translation.y);
    layout.rotation = sample.rotation.to_euler(EulerRot::XYZ).2.to_degrees();
    layout.scale = (sample.scale.x, sample.scale.y);
}

/// UI 节点使用布局数据而不是 Bevy Transform，因此属性轨道需要映射到对应布局字段。
pub fn apply_sample_to_ui_layout_property(
    sample: &Transform,
    property: &str,
    layout: &mut SceneUiLayout,
) {
    match property {
        "transform.position" => layout.offset = (sample.translation.x, sample.translation.y),
        "transform.rotation" => {
            layout.rotation = sample.rotation.to_euler(EulerRot::XYZ).2.to_degrees();
        }
        "transform.scale" => layout.scale = (sample.scale.x, sample.scale.y),
        _ => apply_sample_to_ui_layout(sample, layout),
    }
}

/// 把 UI 布局转换为 Transform 关键帧值，供编辑器与运行时共享。
pub fn animation_transform_from_ui_layout(layout: &SceneUiLayout) -> Transform {
    Transform {
        translation: Vec3::new(layout.offset.0, layout.offset.1, 0.0),
        rotation: Quat::from_rotation_z(layout.rotation.to_radians()),
        scale: Vec3::new(layout.scale.0, layout.scale.1, 1.0),
    }
}

fn parse_vec3(value: &str, field: &str) -> Result<Vec3, String> {
    let values = parse_floats::<3>(value, field)?;
    Ok(Vec3::new(values[0], values[1], values[2]))
}

fn parse_quat(value: &str) -> Result<Quat, String> {
    let values = parse_floats::<4>(value, "rotation")?;
    let rotation = Quat::from_xyzw(values[0], values[1], values[2], values[3]);
    if !rotation.is_finite() || rotation.length_squared() <= f32::EPSILON {
        return Err("animation rotation must be a finite non-zero quaternion".into());
    }
    Ok(rotation.normalize())
}

fn parse_floats<const N: usize>(value: &str, field: &str) -> Result<[f32; N], String> {
    let values: Vec<_> = value.split(',').map(str::trim).collect();
    if values.len() != N {
        return Err(format!("animation {field} requires {N} values"));
    }
    let mut parsed = [0.0; N];
    for (index, value) in values.into_iter().enumerate() {
        parsed[index] = value
            .parse::<f32>()
            .map_err(|_| format!("animation {field} contains invalid number: {value}"))?;
        if !parsed[index].is_finite() {
            return Err(format!("animation {field} contains a non-finite number"));
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(time: f32, transform: Transform) -> SceneAnimationKey {
        SceneAnimationKey {
            time,
            value: format_animation_transform(&transform),
        }
    }

    #[test]
    fn transform_format_round_trips() {
        let source = Transform {
            translation: Vec3::new(12.5, -8.25, 3.0),
            rotation: Quat::from_rotation_z(0.75),
            scale: Vec3::new(2.0, 0.5, 1.0),
        };
        let parsed = parse_animation_transform(&format_animation_transform(&source)).unwrap();
        assert!(parsed.translation.abs_diff_eq(source.translation, 0.0001));
        assert!(parsed.rotation.abs_diff_eq(source.rotation, 0.00001));
        assert!(parsed.scale.abs_diff_eq(source.scale, 0.0001));
    }

    #[test]
    fn samples_position_rotation_and_scale_between_unsorted_keys() {
        let start = Transform::default();
        let end = Transform {
            translation: Vec3::new(100.0, 40.0, 2.0),
            rotation: Quat::from_rotation_z(std::f32::consts::PI),
            scale: Vec3::new(3.0, 2.0, 1.0),
        };
        let sampled = sample_animation_transform(&[key(2.0, end), key(0.0, start)], 1.0)
            .expect("the track has valid keys");
        assert!(sampled
            .translation
            .abs_diff_eq(Vec3::new(50.0, 20.0, 1.0), 0.001));
        assert!(sampled.scale.abs_diff_eq(Vec3::new(2.0, 1.5, 1.0), 0.001));
        assert!(sampled
            .rotation
            .mul_vec3(Vec3::X)
            .abs_diff_eq(Vec3::Y, 0.001));
    }

    #[test]
    fn sampling_clamps_to_first_and_last_valid_keys() {
        let start = Transform::from_xyz(5.0, 0.0, 0.0);
        let end = Transform::from_xyz(15.0, 0.0, 0.0);
        let keys = [key(1.0, start), key(2.0, end)];
        assert_eq!(
            sample_animation_transform(&keys, 0.0)
                .unwrap()
                .translation
                .x,
            5.0
        );
        assert_eq!(
            sample_animation_transform(&keys, 4.0)
                .unwrap()
                .translation
                .x,
            15.0
        );
    }

    #[test]
    fn damaged_keys_do_not_block_valid_keys() {
        let keys = [
            SceneAnimationKey {
                time: 0.0,
                value: "broken".into(),
            },
            key(1.0, Transform::from_xyz(9.0, 0.0, 0.0)),
        ];
        assert_eq!(
            sample_animation_transform(&keys, 0.5)
                .unwrap()
                .translation
                .x,
            9.0
        );
    }

    #[test]
    fn sprite_frames_use_step_sampling_and_skip_damaged_keys() {
        let keys = [
            SceneAnimationKey {
                time: 1.0,
                value: "4".into(),
            },
            SceneAnimationKey {
                time: 0.0,
                value: "1".into(),
            },
            SceneAnimationKey {
                time: 0.5,
                value: "broken".into(),
            },
        ];
        assert_eq!(sample_sprite_frame(&keys, 0.5), Some(1));
        assert_eq!(sample_sprite_frame(&keys, 0.999), Some(1));
        assert_eq!(sample_sprite_frame(&keys, 1.0), Some(4));
        assert_eq!(sample_sprite_frame(&keys, 4.0), Some(4));
    }

    #[test]
    fn runtime_applies_sprite_frame_by_stable_id() {
        let mut app = App::new();
        app.add_systems(Update, apply_runtime_animations);
        app.world_mut().spawn((
            SceneAnimationPlayer {
                clips: vec![crate::scene::SceneAnimationClip {
                    name: "Run".into(),
                    length: 1.0,
                    tracks: vec![crate::scene::SceneAnimationTrack {
                        target_node: "target".into(),
                        property: "sprite.frame".into(),
                        kind: SceneAnimationTrackKind::SpriteFrame,
                        keys: vec![
                            SceneAnimationKey {
                                time: 0.0,
                                value: "1".into(),
                            },
                            SceneAnimationKey {
                                time: 0.5,
                                value: "6".into(),
                            },
                        ],
                    }],
                    ..default()
                }],
                ..default()
            },
            RuntimeAnimationPlayback {
                clip_name: "Run".into(),
                time: 0.75,
                playing: true,
            },
        ));
        let target = app
            .world_mut()
            .spawn((
                RuntimeSceneNode {
                    id: "target".into(),
                    parent_id: None,
                    order: 0,
                    kind: "sprite2d".into(),
                },
                SceneSprite2D {
                    hframes: 4,
                    vframes: 2,
                    ..default()
                },
            ))
            .id();

        app.update();

        assert_eq!(app.world().get::<SceneSprite2D>(target).unwrap().frame, 6);
    }

    #[test]
    fn runtime_applies_sample_to_spatial_target_by_stable_id() {
        let keys = [
            key(0.0, Transform::from_xyz(0.0, 0.0, 0.0)),
            key(2.0, Transform::from_xyz(100.0, 20.0, 0.0)),
        ];
        let mut app = App::new();
        app.add_systems(Update, apply_runtime_animations);
        app.world_mut().spawn((
            SceneAnimationPlayer {
                clips: vec![crate::scene::SceneAnimationClip {
                    name: "Move".into(),
                    length: 2.0,
                    tracks: vec![crate::scene::SceneAnimationTrack {
                        target_node: "target".into(),
                        property: "transform".into(),
                        kind: SceneAnimationTrackKind::Transform,
                        keys: keys.into(),
                    }],
                    ..default()
                }],
                ..default()
            },
            RuntimeAnimationPlayback {
                clip_name: "Move".into(),
                time: 1.0,
                playing: true,
            },
        ));
        let target = app
            .world_mut()
            .spawn((
                RuntimeSceneNode {
                    id: "target".into(),
                    parent_id: None,
                    order: 0,
                    kind: "sprite2d".into(),
                },
                Transform::default(),
            ))
            .id();
        app.update();
        assert!(app
            .world()
            .get::<Transform>(target)
            .unwrap()
            .translation
            .abs_diff_eq(Vec3::new(50.0, 10.0, 0.0), 0.001));
    }

    #[test]
    fn autoplay_advances_and_applies_with_runtime_system_chain() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(GamePaused(false))
            .add_systems(
                Update,
                (
                    initialize_runtime_animation_players,
                    advance_runtime_animation_players,
                    apply_runtime_animations,
                )
                    .chain(),
            );
        app.world_mut().spawn(SceneAnimationPlayer {
            autoplay: "Move".into(),
            clips: vec![crate::scene::SceneAnimationClip {
                name: "Move".into(),
                length: 2.0,
                tracks: vec![crate::scene::SceneAnimationTrack {
                    target_node: "target".into(),
                    property: "transform".into(),
                    kind: SceneAnimationTrackKind::Transform,
                    keys: vec![
                        key(0.0, Transform::default()),
                        key(2.0, Transform::from_xyz(100.0, 0.0, 0.0)),
                    ],
                }],
                ..default()
            }],
            ..default()
        });
        let target = app
            .world_mut()
            .spawn((
                RuntimeSceneNode {
                    id: "target".into(),
                    parent_id: None,
                    order: 0,
                    kind: "sprite2d".into(),
                },
                Transform::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time<()>>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();
        assert!(app
            .world()
            .get::<Transform>(target)
            .unwrap()
            .translation
            .abs_diff_eq(Vec3::new(50.0, 0.0, 0.0), 0.001));
    }

    #[test]
    fn ui_layout_conversion_uses_top_left_position() {
        let mut layout = SceneUiLayout::sized(120.0, 40.0);
        let sample = Transform {
            translation: Vec3::new(32.0, 48.0, 0.0),
            rotation: Quat::from_rotation_z(0.5),
            scale: Vec3::new(2.0, 3.0, 1.0),
        };
        apply_sample_to_ui_layout(&sample, &mut layout);
        assert_eq!(layout.offset, (32.0, 48.0));
        assert!((layout.rotation - 0.5_f32.to_degrees()).abs() < 0.001);
        assert_eq!(layout.scale, (2.0, 3.0));
        let round_trip = animation_transform_from_ui_layout(&layout);
        assert!(round_trip
            .translation
            .abs_diff_eq(sample.translation, 0.001));
        assert!(round_trip.scale.abs_diff_eq(sample.scale, 0.001));
    }

    #[test]
    fn transform_property_tracks_only_change_the_requested_group() {
        let original_rotation = Quat::from_rotation_z(0.25);
        let mut target = Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: original_rotation,
            scale: Vec3::new(2.0, 3.0, 4.0),
        };
        let sample = Transform {
            translation: Vec3::new(40.0, 50.0, 60.0),
            rotation: Quat::from_rotation_z(1.0),
            scale: Vec3::splat(8.0),
        };

        apply_sample_to_transform_property(&sample, "transform.position", &mut target);

        assert_eq!(target.translation, sample.translation);
        assert_eq!(target.rotation, original_rotation);
        assert_eq!(target.scale, Vec3::new(2.0, 3.0, 4.0));
    }

    #[test]
    fn ui_property_tracks_preserve_unrelated_layout_values() {
        let mut layout = SceneUiLayout::sized(100.0, 50.0);
        layout.offset = (8.0, 12.0);
        layout.rotation = 10.0;
        layout.scale = (2.0, 3.0);
        let sample = Transform::from_rotation(Quat::from_rotation_z(0.5));

        apply_sample_to_ui_layout_property(&sample, "transform.rotation", &mut layout);

        assert_eq!(layout.offset, (8.0, 12.0));
        assert!((layout.rotation - 0.5_f32.to_degrees()).abs() < 0.001);
        assert_eq!(layout.scale, (2.0, 3.0));
    }
}
