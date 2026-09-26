# mc-launcher-v2

Полнофункциональный лаунчер Minecraft уровня Prism Launcher: vanilla + Fabric + Quilt +
NeoForge + Forge, Modrinth/CurseForge, модпаки, Microsoft-аккаунты, импорт инстансов.

Приоритеты проекта (в порядке важности): честность и безопасность (хэши каждого байта,
ноль инжекций в игру, секреты вне git) → скорость (нативный Tauri-бинарник) → полнота →
сменяемость UI (весь бизнес в Rust-ядре, UI — тонкий слой).

## Стек

- Ядро: Rust + Tauri 2, tokio, reqwest, serde, thiserror, tracing.
- UI: React 19 + TypeScript (strict) + Vite, zustand, Tailwind CSS 4, lucide-react.
- Платформа: Windows 11 (первичная), Linux — по возможности.

## Сборка

```powershell
# полная сборка (frontend + release-бинарник без инсталлятора)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build.ps1

# проверка качества (tsc strict, clippy -D warnings, тесты)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check.ps1

# режим разработки (окно + HMR)
npm run tauri dev
```

Требования: Node LTS, Rust (stable, target `x86_64-pc-windows-msvc`), MSVC Build Tools.

## Документация

- [docs/PROGRESS.md](PROGRESS.md) — статус этапов M0–M9, доказательства приёмки.
- [docs/DECISIONS.md](DECISIONS.md) — отклонения от спеки и их причины.
- [docs/UI-CONTRACT.md](UI-CONTRACT.md) — контракт «ядро ↔ UI» (появится на M4).

## Лицензия

GPL-3.0 (планируется, см. M9). Minecraft © Mojang Studios; проект не связан с Mojang
и не распространяет файлы игры, только скачивает их с официальных серверов по хэшам.
