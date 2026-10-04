# Генерация ключей подписи updater (спека §11): приватный ВНЕ репо!
# tauri signer generate -w scripts/keys/tauri.key (пароль пустой по желанию)
param()
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot\..
if (-not (Test-Path "scripts/keys")) { New-Item -ItemType Directory -Path "scripts/keys" | Out-Null }
npx --yes @tauri-apps/cli signer generate -w "scripts/keys/minisign.key" --password ""
Write-Host "ПУБЛИЧНЫЙ ключ в scripts/keys/minisign.key.pub — вставьте в tauri.conf.json > bundle > windows > certificateThumbprint/updater pubkey."
Write-Host "ПРИВАТНЫЙ ключ scripts/keys/ в .gitignore (scripts/keys/ закрыт)." 
