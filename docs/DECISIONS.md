# DECISIONS

Каждое отклонение от спеки — с причиной. Реальность побеждает спеку, но не молча.

---

## D1. Rust установлен в сессии M0

Спека предполагала, что Rust может отсутствовать («проверить в начале и установить при
отсутствии»). Установлен rustup stable 1.98.1, target x86_64-pc-windows-msvc; MSVC Build
Tools 14.51 и Windows SDK уже были на машине. Отклонения нет — зафиксировано как факт.

## D2. Tauri CLI: npm (@tauri-apps/cli) как основной, cargo tauri-cli установлен

Критерий M0 в спеке — «cargo tauri dev». Установлены оба варианта: `@tauri-apps/cli`
(npm, используется в `npm run tauri …` и в скриптах) и `cargo tauri-cli ^2` (для
`cargo tauri dev` из спеки). Оба читают один `tauri.conf.json`.

## D3. README в docs/ + указатель в корне

Спека (§4.2) помещает README.md в `docs/`. Корневой README — короткий указатель на
`docs/README.md`, чтобы GitHub-визиты не терялись. Это не расширение scope.
