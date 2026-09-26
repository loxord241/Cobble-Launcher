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

---

## M1. Vanilla-конвейер (CLI-доказательство)

**Дата:** 2026-09-26

**Сделано:**
- Ядро: `errors.rs` (LauncherError→{code,message,hint}), `util/` (атомарные записи,
  потоковые SHA1/SHA256, long-пути `\\?\`, zip-slip-безопасная распаковка с лимитами
  zip-bomb, санитизация имён, FNV-1a 32), `paths.rs`, `settings.rs` (атомарно, миграции
  по schemaVersion), `events.rs` (типизированные события + broadcast-шина).
- Движок загрузок `net/download.rs`: очередь с приоритетами (ассеты — наименьший),
  параллелизм (дефолт 16), retry 3+1 с экспоненциальным backoff и jitter, 429 c
  уважением Retry-After, обязательная SHA1-проверка каждого файла, атомарные `.part` →
  rename, дедуп по URL/dest, отмена по группе и глобальная, троттлинг прогресса 10 соб./с,
  персистентность `store/queue.json` + resume после рестарта.
- `net/http.rs`: UA `mc-launcher-v2/<ver> (github:…)`, таймауты, backoff на 429/5xx.
- Mojang: `manifest.rs` (кэш TTL 1 ч), `version.rs` (полная модель version JSON,
  inheritsFrom-резолвинг с защитой от циклов и глубины >5, дедуп библиотек — ребёнок
  приоритетнее), `rules.rs` (allow=false, последнее подходящее правило побеждает,
  arch `x86` = 32-бит), `libraries.rs` (maven-пути, классификаторы нативов с `${arch}`,
  zip-распаковка только dll/so/jnilib без META-INF), `assets.rs` (поле `hash`!
  объекты в стор по `<h[:2]>/<h>`, virtual/legacy с hardlink-материализацией),
  `client.rs` (клиент в стор по sha1, без хэша — отказ), `launch.rs` (подстановки,
  новый/старый формат аргументов, log4j-конфиг через `${path}`, фичи
  has_custom_resolution).
- `java/detect.rs`: PATH + Program Files (Adoptium/Java/Zulu/Microsoft/Corretto) +
  `~/.jdks` + runtime лаунчера; разбор версии и битности; выбор точный→ближайший больший.
- CLI `src/bin/mcl.rs`: `mcl launch` (полный конвейер) + `mcl java-scan`; супервизия
  с live-tail лога в файл, ожидание главного меню + 8 с на атласы, kill дерева процессов.
- `auth/offline.rs`: offline-UUID (md5 `OfflinePlayer:<ник>`, version 3).

**Проверка:**
- `cargo test` — **55 юнит + 10 интеграционных (mock-сервер axum) — все зелёные**:
  rules-таблица (8 кейсов), inheritsFrom (мердж/цикл/глубина), подстановки, парсеры
  реальных фикстур 1.20.1/1.12.2/fabric (sha1-проверенные, заморожены
  `scripts/fetch_fixtures.py`), zip-slip (вверх/абсолютные/`\\`/NUL/усечённый zip),
  санитизация, FNV-вектора; движок загрузок: успех+хэш, перекачка при битом хэше,
  постоянный битый хэш → failed, 429 с Retry-After, обрыв соединения, таймаут,
  resume валидного файла, persist/load очереди, отмена группы с очисткой .part, дедуп.
- `cargo clippy --all-targets -- -D warnings` — чисто.
- **Реальный запуск latest release (id `26.3` — Mojang перешли на годовую нумерацию):
  «Sound engine started» + 36 строк «Created: … minecraft:textures/atlas/…», EXIT=0,
  общее время 9.6 с (повторный прогон, кэш).** Лог:
  `%LOCALAPPDATA%\mc-launcher-v2\cli-run\26.3\logs\latest.log`.
- **Реальный запуск 1.20.1 (регресс эпохи Java 17, свежие 3641 файлов): «Sound engine
  started», EXIT=0, 19.1 с.** Лог: `cli-run/1.20.1/logs/latest.log`.
- Java-детект: JDK 25.0.2 (Temurin) найден и использован обеими версиями.

**Открытые проблемы:** нет. 1.12.2/1.7.10 (legacy-эпоха, Java 8) — после Adoptium в M2.
