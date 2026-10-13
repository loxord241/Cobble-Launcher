# Полная проверка качества: типы TS, clippy (-D warnings), тесты Rust.
# Критерий зелёности: exit code 0.
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot\..

Write-Host "==> npm run build (tsc strict + vite)" -ForegroundColor Cyan
npm run build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Push-Location src-tauri
Write-Host "==> cargo clippy --all-targets -- -D warnings (тот же набор, что в ci.yml)" -ForegroundColor Cyan
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }

Write-Host "==> cargo test" -ForegroundColor Cyan
cargo test
$code = $LASTEXITCODE
Pop-Location
exit $code
