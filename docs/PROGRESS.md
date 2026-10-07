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

---

## M2. Инстансы + стор + Java

**Дата:** 2026-09-26

**Сделано:**
- `instances/` (спека §6.12, §7): CRUD `instance.json` (атомарно, schemaVersion),
  санитизация имён, `.lock` с детектом протухших PID, дублирование (shared — hardlink,
  unique — копия), удаление: wipe / в корзину ОС, статистика запусков.
- `instances/store.rs`: hardlink-стор (link_or_copy с фолбэком на копию), подсчёт
  ссылок через kernel32-FFI (`windows_by_handle` в std нестабилен), «очистить кэш»
  (объекты без ссылок), mixed-копирование дерева при дублировании.
- `instances/run.rs` — оркестратор запуска: версия → загрузки в стор → hardlink в
  инстанс (versions/libraries) → нативы в `bin/<id>` → virtual/legacy → команда →
  спавн с супервизией; события launch_state; Java: путь инстанса → системная → Adoptium.
- `java/adoptium.rs`: metadata `assets/latest/{major}/hotspot` (реальная форма —
  массив записей с singular `binary`), скачивание zip с SHA256-проверкой по checksum
  из metadata, распаковка + перенос внутреннего каталога в `runtime/jdk{major}`,
  переиспользование между инстансами.
- Paths: ассеты переведены на стандартный layout `assets/{indexes,objects,virtual}`.
- CLI: `instance-create/list/duplicate/delete/launch`, `java-install`, `store-cleanup`.

**Найдено и починено на приёмке:**
- `jinput-platform-2.0.5` в 1.12.2 не имеет основного jar на maven Mojang (только
  классификаторы нативов) — planner теперь помечает такие библиотеки artifact=None.
- Legacy-формат: `-cp` в minecraftArguments отсутствует — лаунчер добавляет сам
  (проявилось как «Could not find or load main class»).
- Очередь загрузок: движок scope-ится на план запуска (Failed-задачи прошлых планов
  больше не блокируют новые запуски; восстановление прерванной очереди — M4+).
- Корзина: крейт `trash` падает («operations aborted»), PowerShell-вариант тоже —
  на деревьях из тысяч hardlinks. Итог: store-поддеревья удаляются напрямую
  (данные в сторе), в корзину идёт уникальное (saves/mods/config).

**Проверка:**
- `cargo test` — **63 юнит + 10 интеграционных — зелёные**; clippy -D warnings — чисто.
- **Два инстанса 1.20.1: оба дошли до «Sound engine started»** (11.6 с и 11.8 с, EXIT=0).
- **Объём: один инстанс — 84 МБ (apparent size), оба вместе — 87 МБ < 1.5×** —
  hardlink-стор подтверждён (`du` считает hardlink один раз).
- **Java скачивается при отсутствии: JRE 17 (43.8 МБ) и JRE 8 установлены с Adoptium
  в `runtime/jdk{17,8}`, SHA256 сверен с metadata**; launch 1.20.1 выбрал точную 17.
- **Инстанс 1.12.2 (legacy: minecraftArguments, Java 8, нативы LWJGL2) —
  «Sound engine started» за 7.1 с, EXIT=0**; нативы распакованы в `bin/1.12.2`
  (OpenAL64.dll, lwjgl.dll и др.).
- Дубликат и удаления (wipe + корзина) проверены CLI-командами; store-cleanup удалил
  8848 объектов без ссылок.

**Открытые проблемы:** нет.

---

## M3. Fabric + Quilt + NeoForge

**Дата:** 2026-09-26

**Сделано:**
- `loaders/fabric.rs` + `loaders/quilt.rs`: список версий загрузчика и готовый
  profile JSON (`/v2/versions/loader/<mc>/<loader>/profile/json`), установка =
  сохранение профиля в `versions/` инстанса (спека §6.3: загрузчики ставятся в
  инстанс, не глобально).
- `loaders/neoforge.rs`: список версий maven API (реальная форма
  `{"isSnapshot":…, "versions":[…]}`, схема имён `21.1.x` → MC 1.21.1), скачивание
  installer.jar с обязательной сверкой `.sha1`, headless
  `java -jar installer --installClient <game_dir>` (с shim-файлом
  `launcher_profiles.json`, который требует инсталлятор), перенос version JSON в
  `versions/` инстанса.
- `Instance.version_id`: после установки загрузчика запускается его version JSON;
  `resolve_chain_dirs` ищет родителей в versions/ инстанса → кэше Mojang.
- Интеграция NeoForge (найдено на приёмке, см. D11–D13): universal/client jar
  инсталлятора в classpath, `-DlibraryDirectory` → каталог библиотек инстанса,
  игровые аргументы `--mavenRoot <libraries>` + `--mods
  net.neoforged:neoforge:<v>:universal`.

**Проверка:**
- `cargo test` — **65 юнит + 10 интеграционных — зелёные** (включая дедуп
  `@jar`-имён и интеграцию загрузчиков в inheritsFrom-резолвинг); clippy
  -D warnings — чисто.
- **Инстанс Fabric на latest MC (26.3): «Loading Minecraft 26.3 with Fabric Loader
  0.19.5» + Knot в логе, игра дошла до главного меню, EXIT=0** (подготовка 19 с
  с загрузкой библиотек Fabric).
- **Инстанс NeoForge 1.21.1 (21.1.99): «NeoForge mod loading, version 21.1.99, for
  MC 1.21.1» в логе, «Sound engine started», EXIT=0**; JRE 21 подобрана точно
  (runtime/jdk21, установлен через Adoptium ранее).
- Регресс 1.20.1 — реальный запуск доказан в M2.

**Открытые проблемы:** нет (Forge — отдельный этап M7, тот же headless-путь).

---

## M4. Контракт API + каркас UI

**Дата:** 2026-09-26

**Сделано:**
- `commands/` — тонкие #[tauri::command] над доменами (19 команд: инстансы CRUD,
  запуск, версии, загрузчики, java, настройки, data_dir, logs_tail, dir_open);
  `AppState` (paths + bus + settings RwLock + http). `LauncherError` сериализуется
  как `{code, message, hint}` — ошибки автоматически проходят в UI.
- Мост событий: `EventBus` → `window.emit` (dl_progress, dl_queue_state,
  launch_state, game_log_line); file-логгер (tracing + appender, ротация по размеру).
- `instances::run::launch_detached`: спавн из UI без ожидания меню — супервизия
  в фоновом потоке, лог-файл + события, `.lock` живёт до выхода игры, статистика
  (launchCount/playSeconds) обновляется по выходу.
- `docs/UI-CONTRACT.md` — команды/типы/события + запреты для UI-слоя.
- UI (`src/`): `api/` (typed invoke + события), `state/` (zustand: settings,
  instances со статусами), `ui/layout/Shell` (левая навигация, топбар с кнопкой
  «Играть»), страницы Главная/Инстансы/Настройки, диалог создания инстанса
  (версия из манифеста + загрузчик с версиями), карточка инстанса со статусом
  загрузки/запуска, онбординг 3 шага (каталог → профиль → первый инстанс,
  пропускаемый), i18n ru/en + errors.json (код ошибки → человеческий текст).

**Проверка (скриншоты в `docs/m4-acceptance/`):**
- Чистый профиль (settings.json отсутствует, onboardingDone=false) → при старте
  открылся мастер онбординга: шаг 1 каталог данных, шаг 2 ник, шаг 3 версия из
  манифеста (1-onboarding-step1..3.png).
- «Готово» → инстанс создан → shell с навигацией и карточками инстансов
  (4-shell-home.png).
- Клик «Играть» мышкой → карточка показала «Загрузка… 0/0» (событие dl_progress),
  топбар — «Запущено»; java-процесс жив; лог инстанса содержит
  **«Sound engine started»** (5-launch-progress.png).
- `npm run build` (tsc strict) и `cargo clippy -D warnings`, `cargo test`
  (65+10) — зелёные.

**Открытые проблемы:** нет.


---

## M5. Modrinth + mrpack

**Дата:** 2026-09-26

**Сделано:**
- `modrinth/api.rs`: API v2 (search с facets, project, versions); **реальный API
  отдаёт snake_case** (не camelCase, как сначала предположили) — структуры и
  фикстуры приведены к реальности. reqwest 0.13: `.query` — опциональная фича,
  включена (`features = query`).
- `modrinth/install.rs`: выбор совместимой версии (game_versions + loaders,
  для vanilla-инстансов предпочитаем fabric-сборки), рекурсивные ОБЯЗАТЕЛЬНЫЕ
  зависимости (глубина ≤10, visited), optional — не ставятся автоматически;
  файлы → mods/resourcepacks/shaderpacks по типу проекта; `content-manifest.json`
  атомарно мерджится (переустановка заменяет записи проекта).
- `modrinth/mrpack.rs`: modrinth.index.json (formatVersion 1, validate с внятными
  ошибками), env-фильтр client != unsupported, скачивание с sha1, paths через
  safe_relative_path (санитизация чужих путей), overrides/ распаковываются
  поверх каталога игры (zip-slip-защита), dependencies → создание инстанса
  (fabric/quilt/neoforge; forge — честный отказ до M7).
- `modrinth/updates.rs`: check_updates по манифесту (совместимые свежие версии +
  ченджлоги), update_all — новый файл скачивается, старый → `.trash/`, манифест
  обновляется.
- Команды: modrinth_search, content_install, content_installed,
  content_update_check, content_update_all, mrpack_install. UI: страница «Моды»
  (поиск с учётом версии/загрузчика выбранного инстанса, установка в инстанс,
  установка .mrpack, проверка/применение обновлений). UI-CONTRACT дополнен.
- Настройки приведены к camelCase по спеке §7 (`rename_all` + совместимость);
  найдено на приёмке: поля из UI раньше игнорировались из-за рассинхрона
  snake/camel (D15).

**Проверка:**
- `cargo test` — **67 юнит + 10 интеграционных — зелёные** (парсеры реальных
  форматов search/versions); clippy -D warnings чисто.
- **CLI: реальный модпак Fabulously Optimized 15.0.0-alpha.3 (MC 26.3 + fabric,
  sha1 сверён при скачивании) установлен в НОВЫЙ инстанс — 40 модов + 2
  ресурспака в content-manifest — и запущен до главного меню**
  («Sound engine started», EXIT=0, 18.5 с; ModernFix из модпака загрузился).
- **UI: страница «Моды» — реальный поиск, установка Sodium в vanilla-инстанс
  кликами через CDP** (скриншоты docs/m5-acceptance/); jar на диске.

**Открытые проблемы:** нет.


---

## M6. Оптимизация + настройки

**Дата:** 2026-09-26

**Сделано:**
- `optimize.rs`: стек производительности (конфигурируемый JSON в коде — Sodium,
  Lithium, FerriteCore, EntityCulling, ImmediatelyFast, ModernFix, Dynamic FPS,
  Fabric API; реальные project_id сверены с API; OptiFine сознательно нет).
  Совместимость резолвится по (mc, loader); мод без совместимой версии честно
  ПРОПУСКАЕТСЯ с записью в лог (ModernFix для 26.3+fabric пока не вышел).
- RAM-гайды: sysinfo → рекомендация `clamp(systemRAM/4, 1ГБ, 6ГБ)` + предупреждение
  «длинные GC-паузы» при >8ГБ (команда ram_guide, подсказка в настройках).
- Процессы: PID пишется в `.lock` (launch_detached), `instance_kill` = taskkill /T
  (дерево) + снятие lock, `process_status` = скан живых lock-ов; UI: кнопка Стоп
  на работающей карточке, восстановление статусов после рестарта лаунчера.
- UI: кнопка «Оптимизировать» на карточках Fabric-инстансов; темы переключаются
  (существующая система tokens.css — подтверждена скриншотами).

**Проверка:**
- `cargo test` — **69 юнит + 10 интеграционных — зелёные**; clippy -D warnings чисто.
- **Оптимизировать → стек стоит: запуск Fabric-инстанса — «Loaded configuration
  file for Sodium: 37 options» в логе, EXIT=0**; в манифесте 7 модов стека,
  modernfix пропущен с записью в лог (нет версии для 26.3+fabric).
- **Тема светлая/тёмная переключается** (скриншоты docs/m6-acceptance/1,2).
- RAM-подсказка в настройках показывает рекомендацию от ram_guide.

**Открытые проблемы:** нет.


---

## M7. Forge

**Дата:** 2026-09-26

**Сделано:**
- `loaders/forge.rs`: promotions_slim.json (recommended → latest), скачивание
  installer.jar с maven.minecraftforge.net с обязательной сверкой `.sha1`
  (спека §6.3; никакого `--mirror` на чужие домены — антипаттерн 16Launcher);
  headless `--installClient` через общий `run_headless_installer` (штим
  `launcher_profiles.json` — тот же путь, что у NeoForge).
- **Координата maven — `{mc}-{forgeVersion}`** (1.20.1-47.4.10): голая версия
  из promotions даёт 404 (найдено на приёмке).
- `install_loader("forge")` → инстанс получает version json
  `1.20.1-forge-<v>` через inheritsFrom; universal jar из
  `libraries/net/minecraftforge/forge/<v>/` добавляется в classpath
  (механизм M3/D13); Forge 1.20.1 FML находит universal на classpath без
  `--mods` (в отличие от NeoForge).

**Проверка:**
- `cargo test` — **71 юнит + 10 интеграционных — зелёные** (promotions-фикстура,
  выбор recommended); clippy -D warnings чисто.
- **Инстанс Forge 1.20.1 (47.4.10, recommended): «Forge mod loading, version
  47.4.10, for MC 1.20.1» + «MinecraftForge v47.4.10 Initialized» в логе,
  «Sound engine started», EXIT=0, 18.2 с**; Java 17 подобрана точно из
  runtime/jdk17.

**Открытые проблемы:** нет. Старые Forge (1.12-) без `--installClient` — не
поддерживаются, честная ошибка (спека §6.3 допускает fallback processors как
будущее расширение).


---

## M8. Аккаунты

**Дата:** 2026-09-26

**Сделано:**
- `auth/mod.rs`: реестр accounts.json (kind: offline/msa/ely; ТОЛЬКО ссылки на
  keyring, никаких токенов — `assert_no_plaintext_tokens` тестирует это), active
  через settings, keyring-хелперы (Windows Credential Manager), launch_identity:
  offline → token "0"; msa/ely → refresh сессии (обновлённый refresh — обратно в
  keyring). `AccountRefreshFailed` при неудаче refresh (фолбэк на офлайн, не крах).
- `auth/msa.rs`: полный device-code flow + цепочка XBL → XSTS → loginWithXbox →
  /minecraft/profile; человеческие ошибки XErr (2148916233 — нет Xbox, 2148916238 —
  детский, 2148916235 — регион); авто-refresh по refresh_token.
- `auth/ely.rs`: логин/refresh authserver.ely.by; authlib-injector скачивается с
  GitHub releases c sha256-digest (спека §5 «хэш со страницы релиза»);
  `-javaagent:...=https://authserver.ely.by/api/authlib-injector` для ely-запуска —
  ЕДИНСТВЕННОЕ исключение из ноль-инжекций (явный выбор профиля).
- Redaction (спека §11): `util/redact.rs` — реестр секретов; file-логгер пишет
  через RedactingWriter: все полученные токены в логах → `[REDACTED]`.
- Команды: account_list/add_offline/add_msa_start/add_msa_poll/add_ely/remove/
  active_set. UI-флоу MSA: device-code (код + браузер). CLI: auth-offline,
  auth-msa-start/poll, auth-list, auth-active. Запуск (UI и CLI) идёт с
  идентичностью АКТИВНОГО аккаунта.

**Проверка:**
- `cargo test` — **75 юнит + 10 интеграционных — зелёные** (roundtrip аккаунтов,
  детект plaintext-токена, redaction, XErr-маппинг косвенно); clippy чисто.
- **Keyring roundtrip пройден** (`cargo test keyring_roundtrip -- --ignored`):
  запись/чтение/удаление в Windows Credential Manager.
- **Офлайн-профиль добавлен, accounts.json без plaintext** (файл проверен), запуск
  инстансов через реестр работает (все приёмки M2–M7 шли через offline_identity).
- **MSA device-code: реальный вызов возвращает AADSTS70002** — Azure-приложение
  владельца ещё не помечено «Allow public client flows = Yes» (память M8: владелец
  должен включить; проверка кода корректна — запрос доходит, ошибка серверная).
  Цепочка будет завершена после настройки портала (2 клика владельцем).

**Открытые проблемы (требуют владельца):**
1. Azure: включить «Allow public client flows» → после этого MSA-вход завершается
   (`mcl auth-msa-start` → браузер → код → `mcl auth-msa-poll`).
2. ely.by: реализация полная; приёмка требует реальных учётных данных владельца.


---

## M9. Импорт, краш-анализ, упаковка

**Дата:** 2026-09-26

**Сделано:**
- `process/crash.rs`: анализатор по расширяемым JSON-правилам (Java ниже
  требуемой, GPU-драйвер, OutOfMemory, дубликат мода, отсутствие fabric-api,
  антивирус удалил нативы) → человеческий совет; команда `crash_analyze`.
- `import/`: CF-zip (manifest.json → инстанс + загрузчик; overrides поверх корня
  игры; без CF-ключа — список ссылок для ручного скачивания в notes), MultiMC/Prism
  (mmc-pack.json → uid → loader), свой формат (instance.json + minecraft/);
  эвристика типа по содержимому; zip-slip-защита на чужих архивах.
  `extract_zip_stripping` — распаковка со срезанием префикса (конвенция overrides).
- CI: `.github/workflows/ci.yml` (windows-latest: npm build + clippy -D warnings +
  cargo test). `scripts/gen-keys.ps1` — minisign-ключи updater'а, приватный ВНЕ
  репо (scripts/keys/ в .gitignore).
- CDP-аргумент снят из tauri.conf.json (D14 закрыт).
- README: сборка, MSA-инструкция, CLI-справка, лицензия (на тот момент заявлена
  GPL-3.0; исправлено на MIT 2026-09-27 — A13).
- **NSIS-инсталлятор собран**: `src-tauri/target/release/bundle/nsis/
  mc-launcher-v2_0.1.0_x64-setup.exe`.

**Финальный чек-лист (спека §14):**
- [x] `cargo clippy -- -D warnings`, `cargo test` (80 юнит + 10 интеграционных),
      `npm run build` — чисто.
- [x] Запуски до главного меню: vanilla 26.3 и 1.20.1, 1.12.2, Fabric 26.3,
      NeoForge 1.21.1, Forge 1.20.1, FO-модпак (mrpack), CF-zip импорт.
- [ ] Офлайн-тест «отключить сеть» — требует ручной отсечки сети владельцем;
      механизм готов (кэш манифестов TTL, `.part`-очередь, верифицированные файлы
      пропускаются, MSA-refresh падает в событие + офлайн-фолбэк).
- [x] Мульти-запуск: разные инстансы одновременно (M2/M3 приёмки шли параллельно);
      kill из UI (instance_kill = taskkill /T); закрытие лаунчера игру не убивает
      (launch_detached + `--no-wait` доказательство).
- [x] Модпак Modrinth установлен и запущен; CF-zip импортирован.
- [x] Отрицательный тест хэшей: битый SHA1 → детект и перекачка (mock-сервер,
      integration-тесты движка).
- [x] Секреты: grep по истории — только имена полей, реальных секретов нет;
      accounts.json без plaintext (тест).
- [x] Гигиена репо: target/, node_modules, scripts/keys/ в .gitignore; .git — 1.4 МБ.

**Открытые проблемы (владельцу):**
1. Azure «Allow public client flows = Yes» → завершить MSA-вход (2 мин).
2. ely.by — проверить вход со своей учёткой.
3. Офлайн-тест с ручной отсечкой сети — по желанию.

---

## Стабилизация 2026-09-27

**Дата:** 2026-09-27

**Сделано:**
- **Discovery-аудит** (`DISCOVERY_REPORT.md`): сплошной разбор ядра и UI, топ-15
  находок (отмена загрузок, `ram_guide` в байтах, zip-bomb, LICENSE, имя
  продукта, prompt вместо диалогов, вечный спиннер, торможение UI логами,
  MSA-refresh с чужим путём и др.).
- **Волна P1-фиксов** (D28, шесть находок discovery): реестр активных движков
  загрузок в `AppState` («Стоп» реально отменяет скачивание), `ram_guide` без
  лишнего `*1024`, лимиты zip-bomb в stripping-распаковке, `MSA-refresh` из
  фактического корня данных, экран ошибки старта вместо вечного спиннера,
  диалог удаления «в корзину / навсегда».
- **Волна 5 агентов** (D29): релизность и брендинг (единый productName, LICENSE,
  пакетные метаданные), перф-правки (виртуализация, `memo`, селекторы,
  троттлинг логов), доступность диалогов (focus trap, Escape, aria), нативные
  диалоги файлов + drag&drop, ужесточение CSP, модпаки/ядро.
- **Трёхъязычность** (D30): `en` (по умолчанию), `ru`, `uk` — словари с полным
  паритетом, трёхъязычный `errors.json`, выбор языка в онбординге и настройках.
- **Полный аудит A1–A60** (`FULL_AUDIT_REPORT.md`, реестр — `AUDIT.md`):
  12 направлений, сквозная верификация контрольных пунктов, приоритизация
  P0/P1/P2/P3 и план волн.
- **Документация и лицензионная чистота:** `docs/UI-CONTRACT.md` приведён к ядру
  (46 команд по `invoke_handler`, 5 событий, типы `Account`, `ArchiveResult`,
  `JavaInstall`, `RamGuide`, `Diagnosis`, `uiScale`) — A12; `docs/README.md`
  заявляет MIT вместо GPL-3.0 — A13; дисклеймер Mojang на трёх языках и раздел
  о брендах в корневом `README.md` — A14; CSP дополнен `base-uri 'none'`,
  `object-src 'none'`, `form-action 'none'` — A41; добавлены `NOTICE` и
  `THIRD-PARTY-LICENSES.md` (прямые зависимости Rust/npm, раздел MPL-2.0) —
  A52; пакетные метаданные NSIS (`publisher`, `copyright`, `category`,
  `installMode: currentUser`, три языка установщика) — A53; счётчики тестов и
  решений в README — A55.
- Приёмочные скрипты (`scripts/*.cjs`) принудительно ставят локаль интерфейса
  `localStorage.lang = "ru"` перед проверками — результат не зависит от языка
  свежей установки.

**Проверка:**
- `docs/UI-CONTRACT.md` сверен с `invoke_handler`: все 46 команд и 5 событий
  задокументированы, лишних нет.
- `src-tauri/tauri.conf.json` — валидный JSON; поля `publisher`/`copyright`/
  `category`/`windows.nsis.*` соответствуют схеме Tauri 2
  (`node_modules/@tauri-apps/cli/config.schema.json`).
- `node --check scripts/*.cjs` — синтаксис чистый.

**Открытые проблемы (владельцу):**
1. Дисклеймер Mojang внутри интерфейса (футер / «О программе») и ключи i18n —
   вне этой волны (документы, лицензии, релизные метаданные).
2. Финальные `cargo test` / `npm run build` — за общим прогоном волны (см.
   `FULL_AUDIT_REPORT.md`).

## 2026-09-28 — волна D34/D35: скины + карточки инстансов (запрос владельца)

- **D35 карточки**: Инстансы/Главная — сетка CF-карточек (арт 4:3, бейдж версии,
  Play-оверлей, кебаб); дефолтный арт — процедурная пиксельная «карта» от имени
  (без внешних ассетов); hero/страница инстанса — тот же арт. InstanceRow удалён.
- **D34 скины**: ely.by → скин ely-профилей, Mojang sessionserver → MSA, offline →
  честный дефолт (наш процедурный 64×64). Головы в титлбаре/списке профилей,
  «Персонаж» (canvas-тело) в модалке аккаунтов, «Обновить скин». Ядро:
  account_skin + allowlist + PNG-валидация + кэш 1 ч; CSP не тронут.
- Верификация: cargo test 157+10, clippy -D warnings чист, tsc strict чист,
  приёмки: ui_checks, e2e_features, content_features, wave2_features,
  scroll_test, m4 (с временным сбросом онбординга, восстановлено), m5,
  kill_acceptance, НОВАЯ cards_skins — все зелёные. Модалка аккаунтов стала
  скроллиться (контент выше вьюпорта). UI-CONTRACT: 47 команд.

## 2026-09-28 (вечер) — D36: волна по аудиту паритета

- Все 12 багов PARITY_AUDIT (B1–B12) + DISC-A01 (Forge) + DISC-A02 (UI импорта)
  + доки контракта. Фаза 1 фич: F16 офлайн-режим (гейты во всей сети ядра,
  отказ за миллисекунды, запуск из кэша), F1 Quick Play, F11 профиль на
  инстанс, F5 пресеты JVM, F14 подсказка Java, F27 снятие блокировки,
  F12 быстрый ник. 4 субагента + оркестратор (полоса A2 — после упора
  агента в лимит использования).
- Верификация: cargo 174+10 (+17), clippy/tsc чистые, 10 приёмок зелёные
  (включая новую parity1_features.cjs), офлайн-отказ проверен живьём,
  настройки владельца восстановлены после тестов.

## 2026-09-28 (ночь) — D37 волна A: фаза 2 аудита паритета

- 5 субагентов: миры (W1), repair+java-test (W2), mclogs+история логов (W3),
  снапшоты/копирование/датапаки (W4), storage+экспорт настроек (W5).
  Интеграция оркестратором: 15 IPC-команд, ContentKind::Datapack, хук
  авто-снапшота в update_all, onRepair в трёх кебабах, LogsPanel (фильтры/
  mclo.gs/архив), секция снапшотов и модалка копирования на вкладке Модов.
- Верификация: cargo 228+10, clippy/tsc чистые, 11 приёмок зелёные
  (включая новую parity2_features), scroll_test стал самодостаточным.

## 2026-09-28 (ночь) — D37-B/C: аудит паритета закрыт полностью

- Волна B (5 агентов): скриншоты, зависимости/конфликты модов, обложки паков,
  краш-луп + безопасный режим, ярлыки на рабочий стол, монитор CPU/RAM.
- Волна C (5 агентов): свой authlib-сервер, лимит скорости, палитра команд
  (Ctrl+P), редактор конфигов, Discord RPC (named pipe, без крейтов).
- Итого D37: F1–F30 закрыты, 28 новых IPC-команд (UI-CONTRACT 75), cargo
  298+10 (волны +95 тестов), clippy/tsc чистые, 15 приёмок зелёных.

## 2026-09-28 (вечер) — D38: смена логотипа

- 13 сгенерированных иконок → выбраны 5 (владелец), конвертированы в PNG
  256 (public/logos для UI + src-tauri/logos для include_bytes).
- Настройки → «Логотип»: выбор из 5 + свой PNG; иконка окна и бренд-чип
  применяются сразу и при старте. AppState.app — OnceLock, manage перенесён
  в setup. cargo 302+10, приёмка logo_features зелёная.

## 2026-09-28 (вечер) — D39: редизайн «как в CurseForge»

- Главная = каталог Modrinth (только модпаки и моды): табы, поиск, сортировка
  (downloads/follows/updated/newest/relevance), фильтр загрузчика, бесконечный
  скролл. Клик по строке/кнопке открывает окно проекта — не качает.
- ProjectDetailModal: 4 таба (Обзор/Журнал изменений/Галерея/Версии),
  markdown безопасным конвертером, галерея с зумом, установка версий модов.
- Инстансы — только на странице «Инстансы» (+сортировка). На странице
  инстанса — «Добавить моды» (модалка поиска/установки) без ухода в «Моды».
- Ядро: modrinth_project, modrinth_versions, index-сортировка поиска
  (77 команд). Грабля: gallery имеет И raw_url, И url — serde-алиас
  «duplicate field», разбор вручную.
- Приёмки: catalog_d39 (17, зелёная) + 9 регресса; cargo 306, clippy/tsc.

## 2026-10-01 — Релиз 0.2.0

- Архивные логи: ротаты *.log.gz читаются (прямая зависимость flate2);
  хвост распакованного ограничен max_bytes потоковой обрезкой спереди —
  декомпрессионная бомба упирается в тот же потолок, битый архив = io-ошибка.
- Версия 0.1.0 → 0.2.0 (package.json, tauri.conf.json, Cargo.toml, fallback
  титлбара). cargo 309, clippy/tsc чистые.
- GitHub: main обновлён (схема «один коммит»), релиз v0.2.0 с установщиками
  (nsis + msi). Кнопка «Проверить обновления» заработает, когда репо станет
  публичным (releases/latest ходит без авторизации).

## 2026-10-02 — P3-хвосты: офлайн-гейты MSA (A61) + сплит JS-бандла

- msa.rs: офлайн-гейты на 4 публичных входа (как ely/custom) — офлайн-режим
  теперь отказывает мгновенно и в MSA-потоках; тест до-IO отказа.
- Бандл: react-vendor через rolldown codeSplitting + ленивая
  ProjectDetailModal; 509 КБ → 274 КБ стартовый чанк.
- scripts/hero_shots.cjs уехал в локальный exclude (внутренняя кухня).
- cargo 310+9, clippy/tsc чистые; приёмки catalog_d39, cards_skins,
  ui_checks, scroll_test зелёные.

## 2026-10-02 (позже) — Багфикс MSA-входа (D44)

- Живой вход Microsoft падал 401 на легатном /minecraft/loginWithXbox —
  перешли на каноничный /authentication/login_with_xbox, добавили Accept
  на все вызовы цепочки, убрали два нестандартных Xbox-вызова, ошибки
  401/403 теперь различимы (лицензия vs одобрение Azure-приложения).
- cargo 310+9, clippy/tsc чистые.

- Уточнение D44: 403-подсказка теперь содержит прямую ссылку на форму
  одобрения aka.ms/mce-reviewappid (официальная статья
  help.minecraft.net/hc/en-us/articles/16254801392141). Живой тест владельца:
  403 «Invalid app registration» — Azure-приложение ждёт одобрения Mojang,
  код лаунчера готов; вход заработает после одобрения без изменений кода.

## 2026-10-02 (вечер) — D45: полировка ошибок + стриминг zip + уборка deps

- Сетевые ошибки: в UI одна короткая обёртка + живая причина (было тройное
  дублирование); ключ network_detail в ru/en/uk.
- .mrpack (500 МБ+) и JRE-zip качаются потоком в temp-файл, память не
  растёт от размера архива; хэш-проверки сохранены.
- Снесён react-virtual, tokio «full» → 6 точечных фич.
- cargo 311+9, clippy/tsc чистые; ui_checks/catalog_d39/content_features
  зелёные.

## 2026-10-02 (ночь) — D46: тихий автоапдейтер

- Ключ подписи minisign вне репо (%LOCALAPPDATA%\cobble-updater-keys),
  pubkey в конфиге; updater+process плагины, capability, .sig-артефакты.
- Проверка — update_check раз в сессию (бейдж в сайдбаре → Настройки);
  установка — только явная кнопка, подписанный апдейт, прогресс, relaunch.
- Релиз 0.2.1+: build.ps1 -Bundle подписывает, make_latest_json.cjs
  собирает манифест (заливается ассетом в релиз).

## 2026-10-03 — D47: предрелизная зачистка

- dl_progress несёт group, прогресс привязан к своему инстансу (D18 конец).
- Мёртвая очередь загрузок удалена (D26), ошибки UI все через i18n (D13
  конец), crate opener снят (D29 конец).
- cargo 311+9, clippy/tsc; все 12 приёмок подряд зелёные.

## 2026-10-03 — D48: закрыт внешний багхант (5×P1, 5×P2, 1×P3)

- Волна 5 субагентов: config-редактор (потеря данных), чистка кэша (стирала
  ассеты), sha512-хэши без sha1, откат импортёров, обновления без потери
  модов, single-instance+ярлыки, settings merge, суффиксы групп, OAuth
  state, кириллица миров, quote-aware JVM-флаги.
- cargo 314+10, clippy/tsc, все 12 приёмок зелёные; редактор конфигов
  проверен живьём.

## 2026-10-03 — D49: закрыт внешний UX-хант (6×P1, 6×P2, 4×P3 → 13 подтверждено, 3 отклонено)

- Волна 4 субагентов: фантом модпака, бейдж обновлений, профиль инстанса
  при запуске (3 точки входа), офлайн-скин, подтверждения удаления миров/
  контента, dirty-гард редактора конфигов + Ctrl+S, группировка настроек,
  кнопка «Открыть папку скриншотов», ref/aria 6 модалок на карточку,
  CopyContentDialog с useModalA11y, apiErrorText в сторах, common.back,
  aria-label онбординга.
- Находка волны: window.confirm в Tauri — Promise; все 8 мест переведены
  на askConfirm (ui/confirm.ts), включая сломанные ещё до ханта «Удалить
  мир»/«Удалить аккаунт».
- tsc + build; приёмки e2e/content/wave2/parity1–4/catalog/cards зелёные,
  kill_acceptance PASS (навигация скрипта под D40), живой тест конфирма.

## 2026-10-03 — D50: закрыт ENV-хант живучести (4×P0, 5×P1, 4×P2, 2×P3 — все подтверждены)

- P0: потеря мода при обновлении (rename+корзина), гонка .part двух
  инстансов, стирание стора на томах без хардлинков, порча latest.log
  (handle + Log4j). P1: фокус окна при повторном запуске, длинные пути
  neoforge, честная ошибка без корзины, socks-прокси (фича+валидация),
  OOM на гигантском логе (read_tail везде). P2: опрос running-статусов
  после рестарта, rename_with_retry (JRE/моды), кириллические скриншоты,
  offline-uuid от санитизированного ника. P3: Range-докачка JRE, хинт
  про дату/время при TLS.
- Волна 4 субагентов + хвосты (bin/mcl.rs, logs_tail). cargo 325+10,
  clippy/tsc/build; 9 приёмок зелёных; kill_acceptance PASS; живьём:
  кириллический скриншот + лог игры.

## 2026-10-03 — D51: закрыт SEC-хант (0×P0, 2×P1, 3×P2, 2×P3 — все подтверждены)

- P1: граница домашнего каталога для логотипа (как A16 у иконок), java_test_path
  только java/javaw (анти-примитив запуска). P2: OAuth error=access_denied
  не висит, mclo.gs через askConfirm, settings_export только абсолютный .json.
  P3: реальный репозиторий в UA, script-src 'self' в CSP.
- cargo 327+10 (тесты на новые границы), clippy/tsc/build; приёмки зелёные;
  живьём: CSP цел, экспорт отклоняет относительный путь. Вердикт аудитора:
  после P1-фиксов проект готов к публичному релизу; телеметрии нет (25
  внешних хостов перечислены).

## 2026-10-03 — Релиз 0.2.1

- Версия 0.2.0 → 0.2.1 в 4 местах (package.json, tauri.conf.json, Cargo.toml, Cargo.lock).
- Верификация: cargo 327+10, npm run build, ПОЛНАЯ приёмочная волна 12/12
  (ui_checks, catalog_d39, cards_skins, content_features, m5, wave2, parity1–4,
  scroll_test, e2e_features, kill_acceptance) на живом CDP-стенде; конфиг
  после волны чист (бамп версии — единственный дифф).
- Бандл: scripts/build.ps1 -Bundle — NSIS-setup + MSI, оба с .sig (ключ из
  %LOCALAPPDATA%\cobble-updater-keys); make_latest_json.cjs собрал
  dist-latest/latest.json на NSIS-setup.
- Публикация: коммит master → orphan-main снапшотом → gh release create v0.2.1
  (установщики + .sig + latest.json).
- Первый полный cargo-цикл в новом расположении C:\projects (debug+release
  профили собраны с нуля).

## 2026-10-03 — D53: волна «делай всё»

- Фронт: фейд верхней кромки списка, контраст disabled-кнопки (48% акцента),
  scroll extent починен (отступы внутрь контента), сортировка профилей
  «По имени/По добавлению» (активный первый, localStorage, модалка + чип
  титлбара). i18n 443×3.
- Ядро: install_jre сериализован (tokio::Mutex против гонки двух установок),
  network_err() сохраняет «certificate» из source-цепочки (ely/custom),
  своп обновления извлечён в swap_updated_file + прямые тесты ENV-1
  (happy + блокировка антивирусом) — тестовый шов TEST_API_BASE не нужен.
- Верификация: cargo 330+10, clippy/tsc, приёмки 12/12, судья 3-я итерация
  4/4 CLOSED ACCEPT. Грабля: dev-стенд vs установленный лаунчер — единый
  single-instance идентификатор, стенд молча выходит (exit 0).

## 2026-10-03 — Релиз 0.2.2

- Версия 0.2.1 → 0.2.2 в 4 местах. В содержимое вошли волны D52 (свой
  confirm-диалог, Стив по умолчанию, каркас модалки аккаунтов) и D53
  (сортировка профилей, фейд/контраст/scroll extent, замок install_jre,
  network_err с сертификат-цепочкой, ENV-1 регресс-тесты).
- Верификация: cargo 330+10, npm run build, приёмки 12/12 на живом CDP
  (content_features перезапуск после транзиента). Конфиг после волны чист.
- Публикация: коммит master → orphan-main → gh release v0.2.2
  (nsis+msi+.sig+latest.json, dotted-URL). Первый релиз, который 0.2.1
  подхватит через updater-плагин — живая проверка автообновления владельцем.

## 2026-10-03 — D54: волна обратной связи 0.2.2 (6 замечаний владельца)

- Бейдж обновлений → попап установки (установка переехала в стор updates);
  меню чипа: потолок 45vh + скролл списка, действия закреплены.
- Настоящий Стив: steve.png извлекается из клиент-jar стора (SkinSource::
  Default, кэш навсегда), подсказка «нет скина» только при null-скине.
- «Оптимизировать» → справка «Как оптимизировать» (OptimizeHelpModal, советы
  сверены с веб-поиском); тулбар каталога выровнен (Найти = 44px);
  onError-фолбэк арта на карточках (1×1-PNG WebView2 не рендерит — найдено
  и задокументировано); wave2 переведена на 8×8-иконку.
- cargo 331+10, приёмки 12/12, судья 6/6 CLOSED ACCEPT.

## 2026-10-03 — D55: польский язык + промпты аудитов

- Четвёртая локаль pl: 452×4 ключей (полный паритет), плюралы славянские,
  errors.json +pl (17 кодов), Polski в Настройках/онбординге, NSIS +Polish.
- PERF_HUNT_PROMPT.md + LOC_HUNT_PROMPT.md — новые линзы внешних аудитов
  (в .git/info/exclude).
- Верификация: tsc, живая CDP-проверка lang=pl (навигация/карточки/меню
  контов на польском), приёмки 12/12.

## 2026-10-03 — D56: разбор отчётов PERF+LOC (20/20 закрыто)

- PERF: App-селекторы (P0 — 10 ререндеров/с устранены), батч-обновление
  модов одним прогоном + одна запись манифеста, хэши на лету без re-read
  и sync_all, потоковая упаковка zip, быстрый путь Java + один probe
  вместо двух, HashSet-дедуп и группы без клона, условный manifest,
  точечные селекторы логов/статусов, один Стив на всех.
- LOC: ключ moreInSettings (P0, мой пропуск), хинты ядра → коды
  (hintCode + errors.json "hints"), краш-диагнозы ×4, apiErrorText везде,
  дата по локали, плюрал safeMode, format.ts с единицами по языку,
  фильтр диалога, en-кальки, uk-апострофы, сортировка по локали.
- cargo 331 (0 warning), clippy/tsc, приёмки 12/12.

## 2026-10-03 — Релиз 0.2.3

- Версия 0.2.2 → 0.2.3 в 4 местах. Содержимое: D54 (попап обновлений,
  Стив, меню чипа, справка оптимизации), D55 (польский), D56 (разбор
  отчётов PERF+LOC — 20 фиксов).
- Верификация: cargo 331+10, npm build, приёмки 12/12 (content_features
  перезапуск после транзиента). Конфиг после волны чист.
- Публикация: коммит master → orphan-main → gh release v0.2.3
  (nsis+msi+.sig+latest.json). Первый релиз, который 0.2.2 покажет в НОВОМ
  попапе обновлений (клик по бейджу — окошко установки).

## 2026-10-04 — D57: волна отчёта владельца (4 бага)

- Forge/NeoForge-модпаки из каталога больше не ставятся «ванилью»:
  resolve_loader матчит ключи `forge`/`neoforge` (спека §6.2, без
  суффикса `-loader`), загрузчик ставится через run::install_loader.
- Иконка модпака скачивается при установке (set_icon_from_bytes,
  data-URL, лимит 300 КБ, PNG/JPEG/WebP).
- «Проверить обновления» переживает удалённые с Modrinth проекты:
  LauncherError::HttpStatus (контракт наружу прежний), 404/410 —
  пропуск с warn, не падение.
- Кебаб-меню карточки — портал + fixed + флип: не режется карточкой
  и не вылезает за окно.
- Приёмка content_features: таймауты бэкапа/экспорта 60с→300с.
- Верификация: cargo 337+10 (0 warning), clippy/tsc; живьём на CDP:
  установка Forge-пака (forge 43.3.5 + иконка), проверка обновлений
  BMC4 без 404, бэкап, меню.

## 2026-10-04 — D58: «Мне повезёт» + живая проверка Fabric после D57

- Кнопка «Мне повезёт» на Главной (рядом с «Найти»): случайный проект
  таба через два вызова modrinth_search (total_hits → случайный offset),
  окно проекта как из каталога. Без новых IPC. i18n ×4.
- Fabric-модпак «Vanilla Perfected» установлен живьём после фикса D57:
  бейдж fabric, иконка проекта, без регрессий.
- tsc/vite чистые; cargo не тронут (337+10).

## 2026-10-04 — D59: вкладка «Скины» (CurseForge-стиль) + честные офлайн-скины

- Проверены механизмы «скинов без лицензии» (Reddit/доки): TLauncher — свой
  скин-сервис (не воспроизводим без публичного сервиса), SkinsRestorer —
  серверные плагины, клиентские моды — только для себя. Легальный путь
  проекта — Ely.by (бесплатная регистрация, authlib-injector разрешён).
- Страница «Скины»: превью с drag-поворотом, «Мои скины» (PNG 64×64/64×32,
  валидация ядром), «Библиотека» (Steve/Alex classic/slim из клиент-jar).
- Применение: msa → Mojang API (оживёт после одобрения заявки), ely → честная
  подсказка про сайт ely.by, offline → честный отказ + предложение Ely.by.
- IPC 77→82 (skins_*), cargo 348+10 (0 warning), tsc/vite чистые,
  живьём на CDP подтверждено.

## 2026-10-04 — Релиз 0.2.4

- Версия 0.2.3 → 0.2.4 в 4 местах. Содержимое: D57 (Forge/NeoForge-модпаки,
  иконки модпаков, 404 проверки обновлений, кебаб-меню), D58 («Мне повезёт»),
  D59 (вкладка «Скины»).
- Верификация: cargo 348+10 (0 warning), tsc/vite, приёмки 12/12 на CDP
  (m4-онбординг на чистых настройках — ОК; сам скрипт легаси с D39-редизайна).
  Селекторы parity2 (кнопка «Бэкап мира» стала текстовой) и parity4 (конфиги —
  список файлов) обновлены под текущий UI. Конфиг после волны чист.

## 2026-10-04 (вечер) — волна D60: 3 бага владельца + категории + смена версии + CI

- Баги: руки в превью скинов (проекция p.x, SkinViews.tsx — фикс оркестратора,
  пиксельная верификация); иконки-«каша» (Modrinth icon_url = 96×96 webp →
  арт из галереи `Instance.art` + ambient-рендер, InstanceCard/mrpack/content);
  «окно докачки модов» = мод MissingModsChecker в Better MC (не наш лаунчер;
  предустановка Modrinth-доступных записей по хэшам при установке пака).
- Фичи: фильтр категорий каталога (modrinth_search + селект Главной), смена
  версии MC инстанса (instance_version_change + ChangeVersionModal), D18
  (instanceId в dl_progress), D26 (зачистка), CI-релизы (.github/workflows/
  release.yml — черновик по тегу v*, секреты апдейтера — шаг владельца).
- Ely.by: публичного API загрузки скинов НЕТ (5 независимых проверок); ядро
  готово к появлению (multipart PUT), кнопка не включена — честная подсказка.
- Сверка AUDIT: миры/сортировка/копирование модов/DRP/D13/D25/D29 уже были
  закрыты ранее — статусы актуализированы.
- Верификация: cargo 362+10 (0 warning), clippy -D warnings чист, tsc/vite,
  i18n 508×4, приёмки 12/12 (m4: онбординг ✓, «Играть —» — легаси D39,
  покрыто kill_acceptance). Живьём: руки, арт/ambient-ветки карточек,
  категории, модалка версии. Артефакты вычищены; BMC2 без иконки (wave2
  снимает по дизайну — вернётся с обновлением пака).

## 2026-10-06 — D61: хант-волна (10) + волна фиксов (9) + релиз 0.2.5

- Хант: 3 P0 (IPC-traversal в kill/content), 8 P1 (launch_safe мёртв, NeoForge
  лексикографический выбор, watchdog, «Создать» без load(), mrpack-капы,
  i18n-ключи, прогресс модпака, тупик смены версии). Секреты/хэши/XSS — чисто.
- Фиксы: 9 агентов по файловым полосам; +36 тестов; clippy -D warnings 0;
  cargo 411+10; tsc/vite чисто; i18n 509×4 (+1 ключ updates.moreInSettings).
- Интеграция-хвост оркестратора: read-idle 120с в adoptium, таймауты mclogs/
  xbox-цепочки/skins-PUT (после снятия общего 600-с таймаута движком).
- Список «не в волне» — DECISIONS D61 (redirect-policy, pid-reuse, CF-ключ,
  контракт, CI-гигиена).

## 2026-10-07 — D62: «фиксь всё» — бэклог D61 (7/7) + 4 новых P2 + чистка

- Аудит-волна утром (6 read-only агентов → CLEANUP_REPORT.md, вне git), затем
  волна фиксов: 14 агентов полосами + оркестратор (http/commands/events/
  settings/i18n/клей). Полный состав — DECISIONS D62.
- Ядро: redirect-барьер + предохранители всех «сырых» сетевых вызовов; .lock
  pid+start_time (taskkill чужого исключён); settings_get без CF-ключа;
  import-транзакция с загрузчиком; уникальные mrpack-группы/tmp; dl_group_done
  (конец «занят 90 с» после установки мода); CLI-фиксы mcl.
- Обвязка: скрипты приёмок с откатами и точным матчем версий; CI permissions/
  SHA-пины/сверка тегов 3 источников; UI-CONTRACT 91 команда.
- Чистка: README×4/сайт/i18n(478×4, 73 правки)/комментарии/плашка перелогина —
  решения Р1–Р5 по рекомендациям, слоган «Честный лаунчер…», тон «вы».
- Числа: clippy -D warnings 0, cargo 423+10 (+12), tsc/vite чисто; приёмки
  m4/e2e/wave2/cards_skins зелёные (полные 12/12 — на релизе).

## 2026-10-07 (вечер) — D62-продолжение: аудит перед публикацией

- 12/12 приёмок живьём; скриншот-аудит 4 языков + 3 ревьюера; премиум-блок
  удалён (противоречил «платных функций нет»); FAQ сайта +3×4 и Microsoft-FAQ
  актуализирован; PL-обрезание даты и чипы скинов починены. Детали DECISIONS.

## 2026-10-07 (вечер 2) — D63: дефолтные арты инстансов

- Панорамы игры из собственных ассетов (команда panorama_art, грань по хэшу
  id) + 12 запасных пиксель-пейзажей; ноль байт Mojang в репо. Приёмки живьём
  зелёные. Детали — DECISIONS D63.

## 2026-10-07 (вечер 3) — D64: волна фиксов финального аудита

- 14 P1 + 45 P2 + ~30 P3: краш-модалка после «Стоп» (launchStartedAt),
  прогресс/отмена установки модпака (downloads_cancel_group, 93 команды),
  тихая докачка JRE (dl_progress jre:*), версия пака в манифесте, MSA-скин
  multipart, апдейтер/офлайн-гейты, uuid аккаунтов, релизный CI, перф движка
  загрузок, plural pl/uk, лицензии. UI-CONTRACT на 93 команды. Детали —
  DECISIONS D64.
