# Полная сборка приложения (frontend + Rust, без бандла-инсталлятора).
# NSIS-бандл собирается на этапе M9: .\build.ps1 -Bundle
param(
    [switch]$Bundle
)
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot\..

Write-Host "==> npm install" -ForegroundColor Cyan
npm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> tauri build (--no-bundle)" -ForegroundColor Cyan
if ($Bundle) {
    npm run tauri build
} else {
    npm run tauri build -- --no-bundle
}
exit $LASTEXITCODE
