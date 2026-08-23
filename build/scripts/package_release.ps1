[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',

    [string]$OutputDirectory,

    [string]$BuildTargetDirectory,

    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$workspaceRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$targetRoot = Join-Path $workspaceRoot 'target'
New-Item -ItemType Directory -Path $targetRoot -Force | Out-Null
$targetRoot = (Resolve-Path -LiteralPath $targetRoot).Path

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $outputRoot = Join-Path $targetRoot 'package\Revy'
} elseif ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
} else {
    $outputRoot = [System.IO.Path]::GetFullPath((Join-Path $workspaceRoot $OutputDirectory))
}

$targetPrefix = $targetRoot.TrimEnd('\') + '\'
if (-not $outputRoot.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Output directory must stay inside $targetRoot"
}

if ([string]::IsNullOrWhiteSpace($BuildTargetDirectory)) {
    $buildTargetRoot = Join-Path $workspaceRoot 'target'
} elseif ([System.IO.Path]::IsPathRooted($BuildTargetDirectory)) {
    $buildTargetRoot = [System.IO.Path]::GetFullPath($BuildTargetDirectory)
} else {
    $buildTargetRoot = [System.IO.Path]::GetFullPath((Join-Path $workspaceRoot $BuildTargetDirectory))
}

$cargoArguments = @('build', '--locked', '-p', 'revy_editor')
$profileDirectory = 'debug'
if ($Configuration -eq 'Release') {
    $cargoArguments += '--release'
    $profileDirectory = 'release'
}

if (-not $SkipBuild) {
    Push-Location $workspaceRoot
    try {
        $previousTargetDirectory = $env:CARGO_TARGET_DIR
        $env:CARGO_TARGET_DIR = $buildTargetRoot
        & cargo @cargoArguments
        if ($LASTEXITCODE -ne 0) {
            throw "Editor build failed with exit code $LASTEXITCODE"
        }
    } finally {
        if ($null -eq $previousTargetDirectory) {
            Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
        } else {
            $env:CARGO_TARGET_DIR = $previousTargetDirectory
        }
        Pop-Location
    }
}

$editorBinary = Join-Path $buildTargetRoot "$profileDirectory\revy_editor.exe"
if (-not (Test-Path -LiteralPath $editorBinary -PathType Leaf)) {
    throw "Editor binary not found: $editorBinary"
}

if (Test-Path -LiteralPath $outputRoot) {
    Remove-Item -LiteralPath $outputRoot -Recurse -Force
}
New-Item -ItemType Directory -Path $outputRoot | Out-Null

function Copy-DirectoryContents {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Source,

        [Parameter(Mandatory = $true)]
        [string]$Destination,

        [string[]]$ExcludeNames = @()
    )

    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    Get-ChildItem -LiteralPath $Source -Force | ForEach-Object {
        if ($ExcludeNames -contains $_.Name) {
            return
        }
        Copy-Item -LiteralPath $_.FullName -Destination $Destination -Recurse -Force
    }
}

Copy-Item -LiteralPath $editorBinary -Destination (Join-Path $outputRoot 'revy_editor.exe')
Copy-DirectoryContents -Source (Join-Path $workspaceRoot 'assets') -Destination (Join-Path $outputRoot 'assets')

$sdkRoot = Join-Path $outputRoot 'sdk\source'
New-Item -ItemType Directory -Path $sdkRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'build\packaging\sdk.Cargo.toml') -Destination (Join-Path $sdkRoot 'Cargo.toml')
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'Cargo.lock') -Destination (Join-Path $sdkRoot 'Cargo.lock')
$sdkCargoConfigRoot = Join-Path $sdkRoot '.cargo'
New-Item -ItemType Directory -Path $sdkCargoConfigRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'build\packaging\sdk.cargo-config.toml') -Destination (Join-Path $sdkCargoConfigRoot 'config.toml')
Copy-DirectoryContents -Source (Join-Path $workspaceRoot 'engine') -Destination (Join-Path $sdkRoot 'engine') -ExcludeNames @('.git', 'target')
Copy-DirectoryContents -Source (Join-Path $workspaceRoot 'build\vendor\cargo') -Destination (Join-Path $sdkRoot 'vendor\cargo')

Copy-DirectoryContents -Source (Join-Path $workspaceRoot 'build\templates') -Destination (Join-Path $outputRoot 'templates')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'new_project.ps1') -Destination (Join-Path $outputRoot 'new_project.ps1')
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'build\packaging\RELEASE_README.md') -Destination (Join-Path $outputRoot 'README.md')
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'LICENSE') -Destination (Join-Path $outputRoot 'LICENSE')

$toolchainInfo = "$(rustc --version)`r`n$(cargo --version)`r`n"
[System.IO.File]::WriteAllText((Join-Path $outputRoot 'TOOLCHAIN.txt'), $toolchainInfo, [System.Text.UTF8Encoding]::new($false))

$licenseRoot = Join-Path $outputRoot 'licenses'
New-Item -ItemType Directory -Path $licenseRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'LICENSE') -Destination (Join-Path $licenseRoot 'REVY-LICENSE-MIT')
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'engine\LICENSE-MIT') -Destination (Join-Path $licenseRoot 'BEVY-LICENSE-MIT')
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'engine\LICENSE-APACHE') -Destination (Join-Path $licenseRoot 'BEVY-LICENSE-APACHE')

$gameRoot = Join-Path $outputRoot 'game_project'
Copy-DirectoryContents -Source (Join-Path $workspaceRoot 'build\templates\rust_game') -Destination $gameRoot
$utf8NoBom = [System.Text.UTF8Encoding]::new($false)

$manifestTemplatePath = Join-Path $gameRoot 'Cargo.toml.template'
$manifest = [System.IO.File]::ReadAllText($manifestTemplatePath)
$manifest = $manifest.Replace('{{PACKAGE_NAME}}', 'revy_game')
$manifest = $manifest.Replace('{{ENGINE_PATH}}', '../sdk/source/engine/crates/arisna_engine')
[System.IO.File]::WriteAllText((Join-Path $gameRoot 'Cargo.toml'), $manifest, $utf8NoBom)
Remove-Item -LiteralPath $manifestTemplatePath

$projectTemplatePath = Join-Path $gameRoot 'project.toml.template'
$project = [System.IO.File]::ReadAllText($projectTemplatePath)
$project = $project.Replace('{{PROJECT_NAME}}', 'Revy Game')
[System.IO.File]::WriteAllText((Join-Path $gameRoot 'project.toml'), $project, $utf8NoBom)
Remove-Item -LiteralPath $projectTemplatePath

$mainPath = Join-Path $gameRoot 'src\main.rs'
$main = [System.IO.File]::ReadAllText($mainPath)
$main = $main.Replace('{{PROJECT_NAME}}', 'Revy Game')
[System.IO.File]::WriteAllText($mainPath, $main, $utf8NoBom)

$cargoConfigTemplatePath = Join-Path $gameRoot '.cargo\config.toml.template'
$cargoConfig = [System.IO.File]::ReadAllText($cargoConfigTemplatePath)
$cargoConfig = $cargoConfig.Replace('{{CARGO_VENDOR_PATH}}', '../sdk/source/vendor/cargo')
[System.IO.File]::WriteAllText((Join-Path $gameRoot '.cargo\config.toml'), $cargoConfig, $utf8NoBom)
Remove-Item -LiteralPath $cargoConfigTemplatePath

Write-Output "Packaged $Configuration editor and SDK at $outputRoot"
