# PROGRESS

Журнал этапов: что сделано, команды проверки, краткий вывод, открытые проблемы.
Этап закрыт, только когда критерии фактически выполнены.

---

## M0. Каркас

**Дата:** 2026-09-26

**Сделано:**
- Установлен Rust 1.98.1 (rustup, stable-x86_64-pc-windows-msvc); Node v24.16.0 и MSVC
  Build Tools 14.51 уже были на машине; диск C: — 388 ГБ свободно.
- Скаффолд Tauri 2 + React 19 + TypeScript strict + Vite (create-tauri-app, react-ts,
  identifier `dev.mclauncher.v2`).
- Подключены Tailwind CSS 4 (@tailwindcss/vite), zustand, lucide-react,
  @tanstack/react-virtual. Создан `src/theme/tokens.css` — тёмная тема по умолчанию
  (#1a1b1e / #242629 / акцент #f16436), светлая через `.theme-light`, маппинг на
  утилиты Tailwind через `@theme inline`.
- `.gitignore` с первого коммита: `target/`, `node_modules`, `dist`, `.env`, `*.key`,
  `*.pem`, `secrets/`, `scripts/keys/`.
- `scripts/build.ps1` (npm install + tauri build --no-bundle; `-Bundle` для инсталлятора),
  `scripts/check.ps1` (tsc strict + clippy -D warnings + cargo test).
- `docs/README.md`, `docs/PROGRESS.md`, `docs/DECISIONS.md`.

**Проверка:**
- `npm run build` — зелёный (tsc + vite, 16 модулей).
- `cargo check` — зелёный (первая компиляция Tauri 2, 1m24s).
- Окно открывается: `npm run tauri dev` → процесс `mc-launcher-v2` (PID 19472),
  MainWindowTitle = «mc-launcher-v2»; скриншот: [m0-window-proof.png](m0-window-proof.png)
  (тёмная тема из tokens.css применяется).

**Открытые проблемы:** нет.
