# Full app build (frontend + Rust, no installer bundle by default).
# NSIS bundle: .\build.ps1 -Bundle
# D46: if the updater signing key exists (outside the repo), the bundle is
# signed (.sig appears next to installers); scripts/make_latest_json.cjs
# then builds latest.json from them. Keep this file ASCII-only:
# Windows PowerShell 5.1 misreads UTF-8 .ps1 files without BOM.
param(
    [switch]$Bundle
)
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot\..

$keyPath = Join-Path $env:LOCALAPPDATA "cobble-updater-keys\cobble.key"
$passPath = Join-Path $env:LOCALAPPDATA "cobble-updater-keys\cobble.key.pass"
if ($Bundle -and (Test-Path $keyPath)) {
    Write-Host "==> updater signing key found (D46)" -ForegroundColor Cyan
    # The bundler reads TAURI_SIGNING_PRIVATE_KEY (key contents). The password
    # comes from the file next to the key (both live outside the repo). Note:
    # assigning "" to $env: in PowerShell REMOVES the variable, which makes
    # the bundler hang on an interactive password prompt.
    if (-not (Test-Path $passPath)) {
        Write-Error "missing key password file: $passPath"
        exit 1
    }
    $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $keyPath -Raw
    # D62: .Trim() молча портил пароли с пробелами по краям. Убираем только
    # завершающий перевод строки файла (CRLF/LF) — сами пробелы не трогаем.
    $rawPass = Get-Content $passPath -Raw
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $rawPass -replace "\r?\n$", ""
}

Write-Host "==> npm install" -ForegroundColor Cyan
npm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> tauri build" -ForegroundColor Cyan
if ($Bundle) {
    npm run tauri build
} else {
    npm run tauri build -- --no-bundle
}
exit $LASTEXITCODE
