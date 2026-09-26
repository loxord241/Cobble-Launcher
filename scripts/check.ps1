# Полная проверка качества: типы TS, clippy (-D warnings), тесты Rust.
# Критерий зелёности: exit code 0.
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot\..

Write-Host "==> npm run build (tsc strict + vite)" -ForegroundColor Cyan
npm run build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Push-Location src-tauri
Write-Host "==> cargo clippy -- -D warnings" -ForegroundColor Cyan
cargo clippy -- -D warnings
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }

Write-Host "==> cargo test" -ForegroundColor Cyan
cargo test
$code = $LASTEXITCODE
Pop-Location
exit $code
