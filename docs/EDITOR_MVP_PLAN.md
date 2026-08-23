# Editor MVP 实施计划

> 优先级：最高  
> 目标平台：Windows Desktop  
> 基线：Bevy 0.19 + Bevy UI + Feathers  
> 计划周期：约 17-21 周完成内部 Alpha，1 名全职开发者；另预留稳定化时间

## 1. MVP 完成定义

Editor MVP 不是界面截图，而是下面这条链路完整可用：

```text
打开项目 -> 新建场景 -> 创建实体 -> 编辑组件 -> 导入资产
         -> 保存 -> 关闭并重开 -> PIE -> 退出并继续编辑
```

所有文档修改都必须支持 Undo/Redo；任何耗时磁盘工作不得阻塞主线程；任何保存失败都必须在 UI 中可见。

## 2. 界面目标

采用接近专业 3D 编辑器的工作区结构，但建立自己的命名、主题和交互规范：

```text
+--------------------------------------------------------------------------+
| Menu | Main Toolbar | Scene/Workspace | Play Controls | Status           |
+----------------+--------------------------------------+------------------+
|                | Scene Tabs                           |                  |
| World Outliner +--------------------------------------+ Details          |
|                | Viewport Toolbar                     |                  |
| Search         +--------------------------------------+ Search           |
| Hierarchy      |                                      | Components       |
|                |  2D / 3D Viewport                    | Properties       |
|                |                                      |                  |
+----------------+--------------------------------------+------------------+
| Content Browser / Output / Problems / Profiler                           |
+--------------------------------------------------------------------------+
```

第一阶段只支持单窗口可停靠布局。多窗口、跨窗口拖放和多显示器留到 MVP 之后。

## 3. UI 架构规则

### 3.1 UI 不直接修改场景

所有入口统一发送 `EditorAction`：

```text
Menu / Shortcut / Toolbar / Context Menu
                  |
                  v
             EditorAction
                  |
          validate + execute
                  |
       EditorCommand or Service
                  |
       SceneDocument / ProjectSession
```

同一个功能只能有一个执行路径。例如 Ctrl+S、菜单 Save 和工具栏 Save 必须触发同一个 `scene.save` Action。

### 3.2 UI 与业务状态单向同步

- `SceneDocument`、`SelectionSet`、`CommandHistory` 和 `AssetDatabase` 是真实状态。
- UI Entity 只是投影，不持有场景事实。
- 业务状态通过 revision、事件或变更检测使局部视图失效。
- 禁止每帧销毁并重建完整面板。
- 长列表必须虚拟化，只生成可见行。

### 3.3 Feathers 隔离

MVP 延续当前 Bevy UI + Feathers，先复用已验证的相机 Viewport 和 Picking。新增控件全部放到 `editor_ui/widgets`，面板不得直接依赖 Feathers 的内部实现。

第 2 周进行 UI Spike，必须验证：

1. 数值输入、文本选择、焦点链、复制粘贴和输入法。
2. 菜单、弹出层、Tooltip、快捷键冲突。
3. 1,000 行虚拟列表和树控件性能。
4. Dock 拖放预览和布局序列化。
5. 125%、150%、200% DPI。

任一核心项不能在 2 周内达到可接受质量，就记录 ADR 并评估整体切换 UI 后端。不要在业务面板中混合两套 UI 框架。

## 4. 设计系统

将 `editor/src/ui/theme.rs` 扩展为集中式 `EditorTheme`：

| 类别 | 必备 Token |
| --- | --- |
| 色彩 | window、panel、surface、hover、selected、border、text、muted、accent、warning、error、success |
| 间距 | 2、4、6、8、12、16 px |
| 尺寸 | menu 28、toolbar 32、tab 28、row 24、icon button 26、splitter 5 px |
| 文字 | caption 11、body 12、label 13、title 14 px；字距始终为 0 |
| 状态 | normal、hovered、pressed、focused、disabled、invalid、mixed |
| 图标 | 单一图标库和统一 16/20 px 尺寸，禁止用 `+`、`Link`、`Refresh` 等文本冒充工具图标 |

首批可复用控件：

- `EditorIconButton`、`EditorTextButton`、`ToggleButton`
- `TextField`、`SearchField`、`NumericDragValue`
- `Checkbox`、`ComboBox`、`ColorField`、`AssetReferenceField`
- `MenuBar`、`PopupMenu`、`ContextMenu`、`Tooltip`
- `TabBar`、`DockHost`、`Splitter`
- `VirtualList`、`TreeView`
- `Toast`、`ModalDialog`、`ProgressTask`

每个控件必须支持 disabled、keyboard focus、hover/pressed、Tooltip 和主题切换；输入控件还必须支持 validation 和 mixed value。

## 5. 面板功能规格

### 5.1 Menu 与主工具栏

**菜单**

- File：New/Open Project，New/Open/Save/Save As Scene，Recent，Exit。
- Edit：Undo/Redo，Cut/Copy/Paste/Duplicate/Delete，Editor Preferences。
- Scene：Add Entity，Add Component，Play Current Scene。
- Window：面板显示、布局 Reset/Save/Load。
- Help：About、Diagnostics、Open Logs。

**工具栏**

- Undo/Redo。
- Select/Translate/Rotate/Scale 模式，使用图标、Tooltip 和状态高亮。
- Local/World segmented control。
- Grid/Angle/Scale Snap toggle 及数值菜单。
- Play/Pause/Stop，状态机严格限制可用按钮。

**验收**

- Action 不可用时，菜单、按钮和快捷键同时不可用。
- Shortcut 在文本输入聚焦时不会误触全局动作。
- 所有图标按钮有 Tooltip，不使用说明文字占据工具栏。

### 5.2 Scene Tabs

- 支持多个 SceneDocument 打开、切换、关闭和重排。
- 未保存标签显示脏状态；关闭前提供 Save/Discard/Cancel。
- 场景保存失败保留文档和脏状态。
- 恢复上次会话打开的文档；缺失文件显示可恢复错误页。

### 5.3 World Outliner

- 真正展示 Bevy `ChildOf/Children` 层级，不按名称伪造树。
- 展开/折叠、搜索、单选、Ctrl 多选、Shift 范围选择。
- 创建、删除、复制、重命名、拖动重设父级。
- 显示可见、锁定、Prefab/Scene 实例和警告状态。
- 删除父实体时明确采用级联删除或子节点提升策略。
- 10,000 实体使用虚拟树，不做完整 UI 重建。

**验收**

- 重命名、重设父级和撤销后，Outliner、Viewport、Details 同帧一致。
- 非法循环父级被拒绝并给出错误。
- 展开状态按 SceneDocument 保存到编辑器会话，而不是场景资产。

### 5.4 Viewport

- 右键飞行、Alt 环绕、中键平移、滚轮缩放、F 聚焦选择。
- Perspective/Top/Front/Side 视图，2D 正交视图。
- 网格、世界坐标轴、选择轮廓、相机速度和渲染模式菜单。
- Picking 支持遮挡、空白取消选择、多选和框选。
- Gizmo 支持 Translate/Rotate/Scale、Local/World、轴约束和 Snapping。
- 资产从 Content Browser 拖入 Viewport 可创建实体。

**事务规则**

- Gizmo pointer-down 保存 before snapshot。
- drag 中实时预览，不向历史栈写多条命令。
- pointer-up 生成一条可合并 Command。
- Escape 取消并恢复 before snapshot。

### 5.5 Details

- 顶部显示名称、稳定 ID、启用状态和 Add Component。
- 基于 Reflect 枚举组件和字段。
- 内置 Drawer：bool、整数、浮点、字符串、Vec2/3/4、Quat/Euler、Color、Enum、Option、数组、Asset Handle。
- Transform 使用专用 Drawer，支持 Reset、复制、粘贴和整组/单轴编辑。
- 多选时显示共同组件；不同值显示 mixed 状态。
- 每个字段支持 Tooltip、验证错误和默认值恢复。
- 修改必须经过 Command，连续拖动合并为一条历史。

### 5.6 Content Browser

- 左侧目录树，右侧可切换缩略图网格/列表。
- 搜索、类型过滤、排序、面包屑和历史导航。
- 拖入外部文件触发异步 import，不直接把复制成功等同为导入成功。
- 显示 importing、ready、failed、stale 状态。
- 支持重命名、移动、删除到回收站、重新导入、在文件管理器中显示。
- 删除或移动前检查依赖并显示引用方。
- 选中资产时 Details 显示 importer 设置和预览。

### 5.7 Bottom Panel

- Output：结构化日志，按 level/target 筛选，支持复制和清空。
- Problems：保存、导入、验证和运行错误，可跳转到资产或实体。
- Tasks：显示后台导入、缩略图和构建进度，可取消。
- Profiler：MVP 先展示 FPS、Frame Time、实体数和资产任务数。

### 5.8 Status Bar

- 当前项目、当前场景、脏状态、后台任务、Bevy/引擎版本。
- 成功提示使用短暂 Toast；失败进入 Problems 并保留，不只显示瞬时字符串。

## 6. 编辑器核心模型

建议先在当前 crate 中增加以下模块，稳定后再提取 crate：

```text
src/
  editor/
    app.rs
    action.rs
    command.rs
    document.rs
    project.rs
    selection.rs
    settings.rs
  scene/
    id.rs
    format.rs
    registry.rs
    migration.rs
  asset/
    database.rs
    importer.rs
    watcher.rs
    task.rs
  ui/
    theme.rs
    widgets/
    shell.rs
    docking.rs
  panels/
    outliner.rs
    details.rs
    content_browser.rs
    output.rs
  viewport/
    camera.rs
    picking.rs
    gizmo.rs
    overlay.rs
```

### 6.1 Action 接口草案

```rust
pub struct ActionId(pub &'static str);

pub struct EditorAction {
    pub id: ActionId,
    pub label: &'static str,
    pub shortcut: Option<Shortcut>,
    pub execute: fn(&mut EditorContext) -> Result<(), EditorError>,
    pub can_execute: fn(&EditorContext) -> bool,
}
```

具体签名可按 Bevy SystemParam 调整，但必须保留唯一 ID、执行条件和单一执行路径。

### 6.2 Command 接口草案

```rust
pub trait EditorCommand: Send + Sync + 'static {
    fn label(&self) -> &str;
    fn apply(&mut self, world: &mut World) -> Result<(), CommandError>;
    fn revert(&mut self, world: &mut World) -> Result<(), CommandError>;
    fn try_merge(&mut self, newer: &dyn EditorCommand) -> bool;
}
```

删除实体命令必须保存可恢复快照；修改属性命令保存稳定实体 ID、组件类型路径、属性路径、before 和 after。

### 6.3 场景文件最小 envelope

```ron
(
  format_version: 1,
  engine_version: "0.1.0",
  scene_id: "...",
  root_entities: ["..."],
  entities: [
    (
      id: "...",
      parent: None,
      name: "Cube",
      components: { /* registered reflected data */ },
    ),
  ],
)
```

必须先定义格式测试和迁移入口，再让编辑器保存真实用户数据。

## 7. 17-21 周实施顺序

### Sprint 0：基线与决策，1 周

- 固定当前可运行截图和交互清单。
- 修复警告，加入 `fmt/check/clippy/test` 脚本或 CI。
- 建立错误类型、日志目录和 Problems 数据模型。
- 完成 ADR-001 UI 栈实验计划。

**出口**：现有功能有基线，后续重构能判断是否回归。

### Sprint 1：设计系统与输入，2 周

- 拆分 `ui.rs`，建立 theme、widgets、shell。
- 完成 IconButton、TextField、NumericDrag、Popup、Tooltip、Tab。
- 建立 Focus、Shortcut 和 ActionRegistry。
- 把当前 1/2/3/X、Refresh、Workspace Tab 接入 Action。

**出口**：工具栏不再是静态文本；至少 10 个 Action 可由菜单、快捷键和按钮共同触发。

### Sprint 2：Dock 与持久布局，2 周

- `PanelLayout` 升级为 Dock tree model。
- 支持显示/隐藏、调整、移动、Reset Layout。
- 保存到用户级编辑器设置，不写入项目资产。
- 验证 1280x720 到 4K 和高 DPI。

**出口**：重启后恢复布局，异常布局可一键重置。

### Sprint 3：SceneDocument 与 Command，2-3 周

- 引入 StableEntityId、SceneDocument、DocumentManager。
- 完成 Create/Delete/Rename/Reparent/Transform 命令。
- 完成 Undo/Redo、事务合并、脏状态和 scene tabs。
- 将 `viewport.rs` 中演示实体迁移为 sample scene fixture。

**出口**：界面中每一次场景修改都进入历史，重做结果稳定。

### Sprint 4：Outliner 与 Selection，2 周

- 真实父子树和虚拟列表。
- 多选、搜索、重命名、拖放重设父级、上下文菜单。
- Selection 改为稳定 ID 集合，维护 active entity。

**出口**：10,000 实体可搜索和滚动；重设父级可撤销。

### Sprint 5：Details，2-3 周

- 建立反射注册和 PropertyEditorRegistry。
- 完成基础 Drawer、Transform 专用 Drawer、多选 mixed 状态。
- 所有属性编辑进入 CommandHistory。

**出口**：新增一个已注册的简单 Component 后，不修改 Details 主逻辑即可编辑其字段。

### Sprint 6：Viewport 工具，2 周

- 相机飞行、环绕、平移、聚焦和速度。
- Picking 多选、空白取消、网格和 overlay。
- Gizmo 接入 Command 事务和 Snapping。

**出口**：Outliner、Viewport 和 Details 能完成同一对象的双向选择与编辑闭环。

### Sprint 7：Content Browser 最小闭环，2-3 周

- 拆分 `filesystem.rs` 为 model/service/view/importer。
- 后台扫描、文件 watcher、缩略图任务和状态模型。
- 资产拖入创建 Mesh/Sprite 实体。

**出口**：导入任务不会冻结 UI，失败原因可查，重启后数据库可恢复。

### Sprint 8：保存、恢复与 PIE，2-3 周

- 场景保存/load round-trip、Save As、自动保存和恢复。
- EditWorld 到 RuntimeWorld 快照。
- Play/Pause/Stop 状态机，退出不回写编辑态。
- 最小 Player 打包流程。

**出口**：完成 MVP 全链路演示，并通过故障与回归测试。

## 8. 任务依赖与优先级

| ID | 任务 | 优先级 | 依赖 | 验收摘要 |
| --- | --- | --- | --- | --- |
| FND-001 | ActionRegistry | P0 | 无 | 菜单/按钮/快捷键同源 |
| FND-002 | Focus/Shortcut 路由 | P0 | FND-001 | 输入框不会触发场景快捷键 |
| FND-003 | Error/Problems 模型 | P0 | 无 | 错误可持久查看和定位 |
| UI-001 | 设计 Token | P0 | 无 | 无散落硬编码主题色 |
| UI-002 | 基础输入控件 | P0 | UI-001 | 文本、数值、验证、焦点可用 |
| UI-003 | DockModel | P0 | UI-001 | 布局可序列化和恢复 |
| DOC-001 | StableEntityId | P0 | 无 | Entity 重建后引用仍可解析 |
| DOC-002 | SceneDocument | P0 | DOC-001 | 多文档、路径、脏状态 |
| CMD-001 | CommandHistory | P0 | DOC-002 | apply/revert/merge 通过测试 |
| SCN-001 | Scene format v1 | P0 | DOC-001 | round-trip 与错误测试通过 |
| OUT-001 | 真实层级树 | P0 | DOC-001、UI-002 | 创建/重命名/重设父级 |
| DET-001 | Reflect Drawer | P0 | CMD-001、UI-002 | 通用组件字段可编辑 |
| VWP-001 | 编辑器相机 | P0 | FND-002 | 飞行/环绕/聚焦完整 |
| VWP-002 | Gizmo Command | P0 | CMD-001 | 一次拖动一条历史 |
| AST-001 | AssetDatabase | P1 | DOC-002 | ID、类型、状态、哈希可查询 |
| AST-002 | 后台 Import | P1 | AST-001、FND-003 | UI 无阻塞且可取消 |
| PIE-001 | Edit/Runtime 隔离 | P0 | SCN-001 | 退出 PIE 不污染编辑态 |
| QA-001 | UI 分辨率矩阵 | P0 | UI-003 | 3 种分辨率和高 DPI 通过 |
| QA-002 | 场景/命令测试 | P0 | CMD-001、SCN-001 | CI 自动运行 |

## 9. 现有文件迁移建议

| 当前文件 | 处理方式 |
| --- | --- |
| `main.rs` | 只保留参数解析、日志和 Editor App 启动 |
| `editor_plugin.rs` | 成为顶层组合根，不保存业务状态 |
| `ui.rs` | 拆为 theme/widgets/shell；删除未使用的静态 helper |
| `panels.rs` | 从固定四个数值迁移为可序列化 DockModel |
| `workspace.rs` | 只表示视口模式；场景归属由 SceneDocument 决定 |
| `selection.rs` | 从 `Option<Entity>` 迁移为 `SelectionSet<StableEntityId>` |
| `hierarchy.rs` | 重写为真实树、虚拟化和增量更新 |
| `inspector.rs` | 保留 Transform 体验，底层替换为反射 Drawer 注册表 |
| `viewport.rs` | 拆相机、Picking、Gizmo、overlay；移除硬编码场景 |
| `filesystem.rs` | 拆成 AssetDatabase、异步服务、Content Browser 视图和平台集成 |

迁移期间每次只替换一条垂直链路，旧模块仍能运行。不要一次性重命名所有文件并同时改行为。

## 10. 测试计划

### 单元测试

- Command 的 apply/revert/merge。
- Stable ID 映射和失效处理。
- 层级循环检测、级联删除和恢复。
- 属性路径解析、类型不匹配和数值验证。
- Asset path 规范化、冲突命名和数据库迁移。

### 集成测试

- 创建 3 层实体，编辑 Transform，Undo/Redo，保存并重开。
- 删除带子节点的实体并 Undo。
- 移动/删除被场景引用的资产。
- 导入失败、项目只读、磁盘空间不足和损坏场景。
- PIE 中修改实体，Stop 后编辑场景保持原值。

### UI 回归

- 1280x720、1920x1080、3840x2160。
- 100%、125%、150%、200% DPI。
- 空项目、10,000 实体、10,000 资产、长文件名和中英文文本。
- Popup 不越界，文本不遮挡，按钮尺寸不随内容抖动。

## 11. MVP 不包含

- 多用户实时协作。
- Blueprint 等级可视化脚本。
- 多进程资产构建农场。
- World Partition、Landscape、Foliage 完整工具。
- Nanite、Lumen、Chaos、Niagara 对等实现。
- 商业插件市场和二进制兼容承诺。

这些能力只有在 Editor MVP 和第一个真实游戏项目稳定后，才进入路线图评审。

## 12. 开始实施时的前三个 PR

1. **PR 1：Action + UI foundation**  
   拆分 theme/widgets/shell，建立 ActionRegistry、Shortcut 路由和基础按钮状态，保持当前外观与功能不回归。
2. **PR 2：SceneDocument + CommandHistory**  
   引入稳定 ID、文档脏状态和 Transform Command，把 Gizmo 与 Inspector 修改接入 Undo/Redo。
3. **PR 3：Outliner vertical slice**  
   将扁平 Hierarchy 替换为真实层级，完成 Create/Rename/Delete/Reparent 和对应撤销测试。

完成这三个 PR 后，项目才拥有继续扩展编辑器界面的可靠基础。
