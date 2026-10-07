# Генерация ключей подписи updater (спека §11): приватный ВНЕ репо!
# D62: ключи генерируются в %LOCALAPPDATA%\cobble-updater-keys — ровно там,
# где их ищет build.ps1 -Bundle, а не в scripts/keys/ внутри репо.
# D64: рядом с ключом сразу создаётся cobble.key.pass со СЛУЧАЙНЫМ паролем —
# без него build.ps1 -Bundle падает, а пустой пароль использовать нельзя
# (присвоить "" в $env: невозможно — переменная удаляется, бандл зависает).
# Формат pass-файла — ровно тот, что читает build.ps1: ASCII без BOM,
# пароль + один CRLF в конце (build.ps1 отрезает один хвостовой CRLF/LF).
# Старые ключи в scripts/keys/ не удаляются и не перезаписываются.
param()
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot\..

$keyDir = Join-Path $env:LOCALAPPDATA "cobble-updater-keys"
$keyPath = Join-Path $keyDir "cobble.key"
$passPath = Join-Path $keyDir "cobble.key.pass"
if (-not (Test-Path $keyDir)) { New-Item -ItemType Directory -Path $keyDir | Out-Null }
if (Test-Path $keyPath) {
    if (Test-Path $passPath) {
        Write-Warning "Ключ уже существует: $keyPath — НЕ перезаписываю. Удалите вручную, если нужна регенерация."
        exit 0
    }
    Write-Error "Ключ $keyPath есть, а файла пароля $passPath нет — пароль восстановить невозможно. Удалите оба файла и запустите скрипт заново (ключ будет перевыпущен)."
    exit 1
}

# Случайный пароль (D46): 24 байта CSPRNG -> 32 ASCII-символа base64.
# Работает одинаково в Windows PowerShell 5.1 и pwsh 7.
$rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
$bytes = New-Object byte[] 24
$rng.GetBytes($bytes)
$rng.Dispose()
$password = [Convert]::ToBase64String($bytes)

npx --yes @tauri-apps/cli signer generate -w "$keyPath" --password "$password"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# cobble.key.pass в формате build.ps1: Get-Content -Raw + отрезание одного
# хвостового CRLF/LF. Пишем ASCII без BOM — одинаково читается в 5.1 и pwsh.
[System.IO.File]::WriteAllText($passPath, $password + "`r`n", [System.Text.Encoding]::ASCII)
Write-Host "ПУБЛИЧНЫЙ ключ: $keyPath.pub — вставьте в tauri.conf.json > bundle > updater > pubkey."
Write-Host "ПРИВАТНЫЙ ключ: $keyPath — лежит ВНЕ репо, build.ps1 -Bundle подхватит его оттуда."
Write-Host "Пароль: $passPath (случайный; потеря = перевыпуск ключа). Его содержимое также положите в секрет TAURI_SIGNING_PRIVATE_KEY_PASSWORD для release.yml."
Write-Host "Старые ключи scripts/keys/ (minisign.key) не тронуты — при желании перенесите в $keyDir."
