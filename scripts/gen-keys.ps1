# Генерация ключей подписи updater (спека §11): приватный ВНЕ репо!
# D62: ключи генерируются в %LOCALAPPDATA%\cobble-updater-keys — ровно там,
# где их ищет build.ps1 -Bundle, а не в scripts/keys/ внутри репо.
# Старые ключи в scripts/keys/ не удаляются и не перезаписываются.
param()
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot\..

$keyDir = Join-Path $env:LOCALAPPDATA "cobble-updater-keys"
$keyPath = Join-Path $keyDir "cobble.key"
if (-not (Test-Path $keyDir)) { New-Item -ItemType Directory -Path $keyDir | Out-Null }
if (Test-Path $keyPath) {
    Write-Warning "Ключ уже существует: $keyPath — НЕ перезаписываю. Удалите вручную, если нужна регенерация."
    exit 0
}
npx --yes @tauri-apps/cli signer generate -w $keyPath --password ""
Write-Host "ПУБЛИЧНЫЙ ключ: $keyPath.pub — вставьте в tauri.conf.json > bundle > updater > pubkey."
Write-Host "ПРИВАТНЫЙ ключ: $keyPath — лежит ВНЕ репо, build.ps1 -Bundle подхватит его оттуда."
Write-Host "Старые ключи scripts/keys/ (minisign.key) не тронуты — при желании перенесите в $keyDir."
