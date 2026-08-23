# Revy GitHub 上传与更新方案

目标仓库：<https://github.com/Gin-Grey/Revy-Bevy>

## 1. 远程仓库职责

固定使用两个远程，禁止混用：

```text
origin    你的 Revy-Bevy 仓库，日常拉取和推送
upstream  Bevy 官方仓库，只用于查看和获取上游源码
```

当前本地仓库已经配置 `origin` 和 `upstream`。地址变化时执行：

```powershell
git remote set-url origin https://github.com/Gin-Grey/Revy-Bevy.git
git remote set-url --push upstream DISABLED
git remote -v
```

禁止把 Revy 分支推送到 Bevy 官方 `upstream`。`upstream` 的 push URL 设置为
`DISABLED` 是额外保险，不影响 `git fetch upstream`。

## 2. 首次上传

当前工作树包含大量尚未提交的编辑器、运行时、Bevy 底层和资源改动。先在本地
完成检查，再创建 Revy 基线提交：

```powershell
git status --short
cargo fmt --all -- --check
cargo check -p revy_editor --offline -j 1
cargo test -p revy_editor --offline -j 1
cargo test -p arisna_engine --offline -j 1
git add --all
git commit -m "Rename Arisna to Revy and publish the editor baseline"
git branch -M main
git push -u origin main
```

不要在未审核 `git status` 和大文件列表时直接执行 `git add --all`。`target/`、
RustRover 的 `.idea/`、项目运行缓存和编辑器状态已经由 `.gitignore` 排除。

## 3. 日常开发分支

`main` 始终保持能够编译和打开编辑器。每个功能使用独立分支：

```powershell
git switch main
git pull --ff-only origin main
git switch -c feature/animation-timeline

# 修改、格式化、测试
git add <本次相关文件>
git commit -m "Implement animation timeline key editing"
git push -u origin feature/animation-timeline
```

在 GitHub 创建 Pull Request，检查通过后合并。Bug 修复使用 `fix/...`，文档使用
`docs/...`。不要把数周的 UI、场景格式、Bevy 底层修改混在一个提交中。

## 4. Bevy 上游更新

Revy 是 Bevy 0.19 的派生版本，不适合周期性把 Bevy `main` 整体合并进来。更新
应采用“按需移植”：

```powershell
git fetch upstream --tags
git log --oneline upstream/main -- engine/crates/bevy_winit
git show <上游提交>
```

确认修复适用于 Revy 后，在独立分支手动移植或 `cherry-pick -x`，随后验证编辑器
窗口嵌入、BSN、文本输入、GLTF/FBX 和运行时场景。任何修改 `engine/crates/bevy_*`
的提交都要在说明中记录上游提交号及 Revy 的额外适配。

升级到新的 Bevy 大版本应单独建立长期分支，例如 `upgrade/bevy-0.20`，不能在功能
分支中顺便完成。

## 5. 版本与发布

建议使用语义化版本：

```text
0.1.x  当前编辑器功能迭代和修复
0.2.0  场景/API 出现向后兼容的新能力
1.0.0  场景格式、脚本 API 和运行流程稳定
```

发布步骤：

1. 更新根 `Cargo.toml` 版本、`README.md` 和架构文档。
2. 运行编辑器与引擎测试，并完成“打开 `.bsn` -> 修改 -> 保存 -> 编辑器内运行”。
3. 执行 `build/scripts/package_release.ps1 -Configuration Release`。
4. 检查 `target/package/Revy`，不要提交打包目录。
5. 创建带注释标签，例如 `git tag -a v0.1.0 -m "Revy 0.1.0"`。
6. 推送标签并在 GitHub Release 上传压缩后的发布包。

## 6. 改名兼容策略

本次对外名称已经改为 Revy：

- 编辑器包和程序：`revy_editor` / `revy_editor.exe`
- 默认游戏包：`revy_game`
- 新项目依赖名：`revy_engine`
- 运行目录：`target/revy-generated`
- 新环境变量：`REVY_*`

底层 Cargo 包和源码目录暂时仍叫 `arisna_engine`。新项目通过 Cargo 的
`package = "arisna_engine"` 使用 `revy_engine` 别名；旧项目仍可直接使用
`arisna_engine`。旧 `.arisna` 状态目录、`ARISNA_*` 环境变量和
`add_arisna_*` API 暂时保留兼容，至少经过一个明确的迁移版本后才能移除。

## 7. 大文件规则

GitHub 单文件上限是 100 MB。上传前检查：

```powershell
Get-ChildItem -Recurse -File |
    Where-Object { $_.FullName -notmatch '\\(target|\.git)\\' -and $_.Length -ge 90MB } |
    Sort-Object Length -Descending |
    Select-Object FullName, Length
```

源码、场景和小型测试资产直接使用 Git。仓库根 `.gitattributes` 已将 FBX、GLB
和 Blend 模型交给 Git LFS；构建缓存、导入缓存、生成项目和可执行文件永远不上传。

首次提交前执行一次 `git lfs install --local`。克隆者需要安装 Git LFS，随后正常
执行 `git clone` 即可取得模型实际内容。当前完整源码、离线依赖和测试资源合计约
0.9 GB，这是保留可离线构建能力的预期体积。

## 8. 开源协议

Revy 自有源码统一使用仓库根目录的 MIT `LICENSE`，Cargo 工作区也声明为 MIT。
Bevy 派生源码原本采用 `MIT OR Apache-2.0`，其原始许可证和版权声明必须保留；
第三方离线依赖同样保留各自许可证。完整公开这些来源和声明才是合规的完全开源，
不能把第三方版权声明删除后重新声称全部由 Revy 原创。
