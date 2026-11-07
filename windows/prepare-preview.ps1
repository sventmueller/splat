$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$source = Join-Path $repoRoot "resources/js"
$destination = Join-Path $PSScriptRoot "ui/preview/js"

if (-not (Test-Path (Join-Path $source "render.js"))) {
    throw "The bundled macOS preview resources were not found at: $source"
}

if (Test-Path $destination) {
    Remove-Item -LiteralPath $destination -Recurse -Force
}
New-Item -ItemType Directory -Path $destination -Force | Out-Null
Copy-Item -Path (Join-Path $source "*") -Destination $destination -Recurse -Force

# The upstream drawing helpers normally draw into the macOS render page's #out element.
# Windows keeps the editable document in #out, so route all temporary SVG drawing through
# a private off-screen work surface instead.
$renderer = Join-Path $destination "render.js"
$javascript = [System.IO.File]::ReadAllText($renderer)
$javascript = $javascript.Replace('document.getElementById("out")', 'document.getElementById("splat-diagram-work")')
[System.IO.File]::WriteAllText($renderer, $javascript, [System.Text.UTF8Encoding]::new($false))

Write-Host "Prepared offline preview resources: $destination"
