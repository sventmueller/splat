$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$tauriDir = Join-Path $PSScriptRoot "src-tauri"

Push-Location $tauriDir
try {
    cargo tauri build --bundles nsis
} finally {
    Pop-Location
}

Write-Host "Installer output: $(Join-Path $tauriDir 'target/release/bundle/nsis')"
