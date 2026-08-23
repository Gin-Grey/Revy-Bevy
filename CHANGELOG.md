# Changelog

Revy 的重要变更记录在此文件中。版本号遵循语义化版本；Alpha、Beta 和 RC
属于预发布版本，可能包含尚未稳定的项目格式与编辑器工作流。

## [0.1.0-alpha.1] - 2026-08-24

首个公开 Alpha 版本，用于代码审查、架构验证和社区协作，不承诺向后兼容。

### Highlights

- 基于 Bevy 0.19 源码的统一引擎工作区与 Revy 桌面编辑器。
- Scene、FileSystem、Inspector、2D、3D、UI 和 Animation 工作区。
- BSN 场景保存、加载、层级关系、动画轨道及编辑器内运行流程。
- Rust Entity Script、Component 与 System 的编辑器集成。
- GLTF、GLB 和 FBX 模型资源支持。
- Windows 项目管理器、项目模板和仅写入 `target/` 的发布流程。

### Known limitations

- 大部分现有代码由 AI 辅助完成，仍在持续进行人工审查与重构。
- 公共 API、编辑器交互和 BSN 格式在 Beta 前可能发生破坏性变化。
- 当前版本不提供跨 Alpha 版本的自动项目迁移保证。
- 嵌入式设备与机器人仿真仍属于后续发展方向。

[0.1.0-alpha.1]: https://github.com/Gin-Grey/Revy-Bevy/releases/tag/v0.1.0-alpha.1
