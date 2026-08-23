[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ProjectPath,

    [string]$Name
)

$ErrorActionPreference = 'Stop'

$installRoot = (Resolve-Path -LiteralPath $PSScriptRoot).Path
$templateRoot = Join-Path $installRoot 'templates\rust_game'
$engineRoot = Join-Path $installRoot 'sdk\source\engine\crates\arisna_engine'
$targetRoot = [System.IO.Path]::GetFullPath($ProjectPath)

if (-not (Test-Path -LiteralPath $templateRoot -PathType Container)) {
    throw "Project template not found: $templateRoot"
}
if (-not (Test-Path -LiteralPath $engineRoot -PathType Container)) {
    throw "Engine SDK not found: $engineRoot"
}

if ([string]::IsNullOrWhiteSpace($Name)) {
    $Name = Split-Path -Leaf $targetRoot
}
if ([string]::IsNullOrWhiteSpace($Name)) {
    throw 'Project name cannot be empty.'
}

$packageName = $Name.ToLowerInvariant() -replace '[^a-z0-9_-]', '_'
$packageName = $packageName.Trim([char[]]'_-')
if ([string]::IsNullOrWhiteSpace($packageName)) {
    $packageName = 'revy_game'
}
if ($packageName[0] -match '[0-9]') {
    $packageName = "game_$packageName"
}

if (Test-Path -LiteralPath $targetRoot) {
    $existing = Get-ChildItem -LiteralPath $targetRoot -Force
    if ($existing.Count -gt 0) {
        throw "Target directory is not empty: $targetRoot"
    }
} else {
    New-Item -ItemType Directory -Path $targetRoot | Out-Null
}

Get-ChildItem -LiteralPath $templateRoot -Force | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $targetRoot -Recurse -Force
}

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$enginePath = $engineRoot.Replace('\', '/')
$manifestTemplatePath = Join-Path $targetRoot 'Cargo.toml.template'
$manifest = [System.IO.File]::ReadAllText($manifestTemplatePath)
$manifest = $manifest.Replace('{{PACKAGE_NAME}}', $packageName)
$manifest = $manifest.Replace('{{ENGINE_PATH}}', $enginePath)
[System.IO.File]::WriteAllText((Join-Path $targetRoot 'Cargo.toml'), $manifest, $utf8NoBom)
Remove-Item -LiteralPath $manifestTemplatePath

$projectTemplatePath = Join-Path $targetRoot 'project.toml.template'
$project = [System.IO.File]::ReadAllText($projectTemplatePath)
$project = $project.Replace('{{PROJECT_NAME}}', $Name)
[System.IO.File]::WriteAllText((Join-Path $targetRoot 'project.toml'), $project, $utf8NoBom)
Remove-Item -LiteralPath $projectTemplatePath

$mainPath = Join-Path $targetRoot 'src\main.rs'
$main = [System.IO.File]::ReadAllText($mainPath)
$main = $main.Replace('{{PROJECT_NAME}}', $Name)
[System.IO.File]::WriteAllText($mainPath, $main, $utf8NoBom)

$cargoConfigTemplatePath = Join-Path $targetRoot '.cargo\config.toml.template'
$cargoConfig = [System.IO.File]::ReadAllText($cargoConfigTemplatePath)
$cargoVendorPath = (Join-Path $installRoot 'sdk\source\vendor\cargo').Replace('\', '/')
$cargoConfig = $cargoConfig.Replace('{{CARGO_VENDOR_PATH}}', $cargoVendorPath)
[System.IO.File]::WriteAllText((Join-Path $targetRoot '.cargo\config.toml'), $cargoConfig, $utf8NoBom)
Remove-Item -LiteralPath $cargoConfigTemplatePath

Write-Output "Created project '$Name' at $targetRoot"
