# Revy

This is **Revy 0.1.0 Alpha 1**, an early preview intended for evaluation and
contribution. Project APIs, editor workflows, and the BSN scene format may
change before the Beta stage. Back up projects before opening them in a newer
Alpha build.

This package contains the Release editor, the Rust engine SDK, and a starter
game project. The unified engine source workspace lives under
`sdk/source/engine`; it is a build input and is never mounted as `res://` in
the editor.

Building Rust game code requires Rust 1.97 or newer. `TOOLCHAIN.txt` records
the exact compiler used to produce this editor package. All crates.io
dependencies are bundled under `sdk/source/vendor/cargo`, and generated
projects use Cargo offline mode by default.

## Start

Run `revy_editor.exe` to open the project manager. Create a project, import
an existing project, or reopen a recent project from there. To bypass the
project manager and open a project directly, pass its directory:

```powershell
.\revy_editor.exe E:\Games\Demo
.\revy_editor.exe --project E:\Games\Demo
```

The project manager creates `Cargo.toml`, `project.toml`, `.cargo/config.toml`,
`src/main.rs`, and `assets/scenes/` from the bundled Rust game template. It
validates imported `project.toml` files and never deletes project files when a
recent-project record is removed.

For automated or unattended setup, the equivalent PowerShell entry point is:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\new_project.ps1 -ProjectPath E:\Games\Demo -Name Demo
```

The generated project references this installation's SDK. Moving the engine
installation later requires regenerating or updating that path in the game's
`Cargo.toml`.

Revy is provided under the MIT License. Bevy is provided under the MIT or
Apache-2.0 license. The license texts are in the package root and `licenses`
directory.
