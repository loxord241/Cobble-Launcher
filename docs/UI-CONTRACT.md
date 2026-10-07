# UI-CONTRACT

Ревизия: 2026-10-07 (D64). Контракт «ядро ↔ UI» (спека §4.3). UI знает ТОЛЬКО
этот список: имена команд, типы входа/выхода и события. Смена UI = переписать
`src/ui` + `src/theme`, не трогая `src/api` и ядро.

Все ошибки команд приходят как `{code, message, hintCode?}` (`LauncherError.payload`;
LOC#2: подсказка — КОД, текст локализует фронт из `errors.json` → `hints`),
код маппится в `src/i18n/errors.json`, `message` — fallback.
Полный реестр кодов (`src-tauri/src/errors.rs`): `network`, `hash_mismatch`,
`not_found`, `invalid_input`, `zip_slip`, `java_not_found`, `version_not_found`,
`cancelled`, `timeout`, `io`, `json`, `http`, `zip`, `instance_running`,
`offline_mode`, `internal` (16 кодов). Любая команда дополнительно может
вернуть `io`/`internal` (файловая система, фоновая задача). Хинт-коды ядра:
`hash_retry`, `java_settings`, `cert_time`, `instance_running`, `offline_toggle`;
UI-сторона добавляет `network_detail` в `errors.json`.

Регистр ключей: camelCase — на обоих концах (`Instance`, `Settings`, события;
D25). Исключения — ответы Modrinth (`SearchHit`/`SearchResult`: как отдаёт API,
snake_case) и `DeviceCodeStart` (snake_case, как в протоколе OAuth).

## Команды (invoke)

Все 93 команды `invoke_handler` (`src-tauri/src/lib.rs:211-305`; пересчитано по
реестру, D64), обработчики — `src-tauri/src/commands/mod.rs` (кроме
`shortcut_take_pending` — он в `lib.rs:61`).

### Инстансы и процессы

| Команда | Вход | Выход | Коды ошибок |
|---|---|---|---|
| `instance_list` | — | `Instance[]` (сортировка по имени; битый `instance.json` пропускается с предупреждением) | — |
| `instance_create` | `{mcVersion, name}` | `Instance` (имя санитизируется) | version_not_found, network, json, invalid_input, io |
| `instance_rename` | `{id, name}` | `Instance` | invalid_input (работающий инстанс), not_found, io |
| `instance_duplicate` | `{id}` | `Instance` (имя «… (копия)»; общее — hardlink из стора, уникальное — копия) | invalid_input (работающий инстанс), not_found, io |
| `instance_delete` | `{id, wipe}` | `{ok}` (`wipe:false` → корзина ОС, `true` → насовсем) | invalid_input (работающий инстанс), not_found, io, internal |
| `instance_open_dir` | `{id}` | `{ok}` (создаёт каталог и открывает проводник) | not_found, io, internal |
| `instance_settings_get` | `{id}` | `Instance` | not_found, invalid_input, io, json |
| `instance_settings_set` | `{instance}` | `Instance` | invalid_input, io, json |
| `instance_launch` | `{id, player}` | `{ok}` (спавн мгновенный, прогресс — событиями) | invalid_input (уже запущен/запускается), version_not_found, java_not_found, hash_mismatch, network, zip, io, json, internal |
| `instance_version_change` | `{id, newMcVersion}` | `{ok}` (смена версии MC как в Prism, D62: запущено → invalid_input; нет версии загрузчика под новую MC → invalid_input, инстанс не тронут; поля атомарно: `mcVersion` новый, `versionId` сброшен; докачка client+libraries+assets ремонт-цепочкой — валидные хэши на диске пропускаются; ошибка докачки (в т.ч. `offline_mode`) уходит в UI как есть — поля уже сохранены, следующий запуск/ремонт докачает) | invalid_input (запущен/та же версия/нет версии загрузчика), version_not_found, network, offline_mode, hash_mismatch, io, json |
| `instance_kill` | `{instanceId}` | `{ok}` (гасит процесс, отменяет активную загрузку группы `instance:<id>`, чистит `.lock`, шлёт `launch_state: exited`) | not_found, io, internal |
| `shortcut_take_pending` | — | `string \| null` (D48: id инстанса из ярлыка второго процесса, пересланный до готовности окна — иначе `null`; Shell забирает при монтировании) | — |
| `process_status` | — | `[id, pid][]` — запущенные инстансы | — |
| `instance_optimize` | `{instanceId}` | `string[]` (файлы стека; несовместимые версии честно пропускаются, не хардкодятся) | invalid_input (не Fabric-инстанс), version_not_found, network, hash_mismatch, io |
| `ram_guide` | — | `RamGuide` (рекомендация RAM по объёму памяти машины) | — |

### Контент, версии, загрузчики

| Команда | Вход | Выход | Коды ошибок |
|---|---|---|---|
| `modrinth_search` | `{query, mcVersion?, loader?, projectType?, categories?, index?, limit?, offset?}` (categories — фильтр тегов, валидируется ядром; index: relevance/downloads/follows/updated/newest; limit 20, offset 0) | `SearchResult` (snake_case) | network, http, json, invalid_input |
| `content_install` | `{instanceId, projectId, versionId?}` | `ContentEntry[]` (обязательные зависимости ставятся рекурсивно) | network, http, version_not_found, hash_mismatch, invalid_input, io, json |
| `content_installed` | `{instanceId}` | `ContentEntry[]` (нет манифеста → `[]`) | — |
| `content_toggle` | `{instanceId, file}` | `ContentEntry` (на диске `x.jar` ↔ `x.jar.disabled`, логический путь в манифесте не меняется; каталоги `mods`/`resourcepacks`/`shaderpacks`/`datapacks` — F7 добавил датапаки, без traversal), запущено → invalid_input) | invalid_input (только 4 контентных каталога), not_found, io |
| `content_remove` | `{instanceId, file}` | `null` (Rust `Result<()>`; файл + запись манифеста), запущено → invalid_input) | invalid_input, not_found, io |
| `content_update_check` | `{instanceId}` | `UpdateCheck[]` | network, http, io, json |
| `content_update_all` | `{instanceId}` | `number` (обновлено; группа `instance:{id}:update`; перед пакетом — авто-снапшот mods/ (F8); старый jar → `.trash/`) | network, hash_mismatch, io, json |
| `mrpack_install` | `{path, name?}` | `ModpackInstallResult` {instance, group} (создаёт инстанс из dependencies; группа отмены `mrpack:<file_stem>:<uuid8>` — хвост от параллельных установок, D64) | invalid_input, version_not_found, zip, zip_slip, java_not_found, network, hash_mismatch, cancelled, io, json |
| `modpack_install` | `{projectId, name?}` | `ModpackInstallResult` {instance, group} (последняя версия проекта → `.mrpack` → тот же установщик; группа `mrpack:<projectId>:<uuid8>`) | invalid_input, version_not_found, zip, zip_slip, java_not_found, network, hash_mismatch, cancelled, io, json |
| `downloads_cancel_group` | `{group}` | `null` (Rust `Result<()>`; снимает движок группы с регистрации и гасит его задачи; для уже завершённой группы — no-op; D64: отмена установки модпака) | — |
| `instance_backup` | `{id}` | `ArchiveResult` (zip в `backups/`; сейвы входят, `.lock`/`logs`/`crash-reports`/`cache` — нет) | not_found, io, zip, internal |
| `instance_export` | `{id}` | `ArchiveResult` (`.mrpack` в `exports/`; сейвы НЕ входят, контент без url → `overrides/`) | not_found, io, json, zip, internal |
| `instance_icon_set` | `{id, path}` | `Instance` (PNG/JPEG/WebP ≤ 300 КБ) | invalid_input, not_found, io, internal |
| `instance_icon_remove` | `{id}` | `Instance` | not_found, io, internal |
| `instance_import` | `{path, name?}` | `Instance` (CF-zip / MultiMC-Prism / свой формат; распознанный загрузчик ставится сразу) | invalid_input, zip, zip_slip, version_not_found, java_not_found, network, hash_mismatch, io, json |
| `manifest_versions` | `{showSnapshots, showOld}` | `VersionEntry[]` | network, json, io |
| `loader_versions` | `{loader}` | `LoaderVersionEntry[]` (`fabric`/`quilt` — все, `neoforge` — 30 последних стабильных) | network, invalid_input, json |
| `loader_install` | `{id, loader, loaderVersion?}` | `Instance` (после установки `versionId` = профиль загрузчика) | invalid_input, version_not_found, java_not_found, network, hash_mismatch, zip, io, internal |

### Аккаунты, миры, логи, скины, хранилище

| Команда | Вход | Выход | Коды ошибок |
|---|---|---|---|
| `account_list` | — | `Account[]` | — |
| `account_add_offline` | `{name}` | `Account` (kind `offline`, uuid = offline-UUID ника; сразу активный) | invalid_input, io, json, internal |
| `account_add_msa_start` | — | `DeviceCodeStart` (snake_case: `device_code`, `user_code`, `verification_uri`, `interval`, `expires_in`; аннотация в `client.ts` с `interval_secs`/`expires_in_secs` не соответствует wire-полям — UI эти поля не читает) | invalid_input (нет `azureClientId`), network, http, json |
| `account_add_msa_poll` | `{deviceCode}` | `Account \| null` (`null` — пользователь ещё не ввёл код; refresh-токен → keyring) | invalid_input, network, http, json, internal |
| `account_add_msa_browser` | — | `Account` (auth-code flow: локальный порт 127.0.0.1, системный браузер, таймаут 300 с) | invalid_input, network, http, json, internal, io |
| `account_add_ely` | `{username, password}` | `Account` (kind `ely`; пароль не сохраняется, в keyring — refresh-токен) | invalid_input, network, http, json, internal |
| `account_remove` | `{id}` | `{ok}` (удаляет и запись в keyring) | not_found, io, internal |
| `account_active_set` | `{id}` | `{ok}` | not_found, io |
| `modrinth_projects` | `{ids: string[]}` | `ProjectMeta[]` (батч /v2/projects?ids=, чанки по 100; поле `id` алиасится в projectId) | invalid_input, network, http, json |
| `modrinth_project` | `{idOrSlug}` | `ProjectDetail` (camelCase: тело markdown, галерея, лицензия, статистика; D39) | invalid_input, network, http, json |
| `modrinth_versions` | `{projectId}` | `ModrinthVersion[]` (camelCase, с changelog; D39) | invalid_input, network, http, json |
| `content_backfill_projects` | `{instanceId}` | `number` (сколько манифестов залечено sha1→projectId) | not_found, network, http, json |
| `update_check` | — | `UpdateInfo` {current, latest?, url?, updateAvailable, checked, note?} (GitHub releases/latest; 404 закрытого репозитория — нормальный исход: `checked`, без ошибки) | network, http, json |
| `java_recommended` | `{mcVersion}` | `number` (мажорная Java: 8/17/21, таблица как run.rs fallback_major) | — |
| `instance_worlds` | `{id}` | `WorldInfo[]` {name, lastModified, sizeBytes, hasNether, hasEnd} (каталоги saves/ с level.dat) | not_found, invalid_input |
| `world_backup` | `{id, world}` | `ArchiveResult` (zip в backups/worlds/, DIM-1/DIM1 внутри; запущено → instance_running) | not_found, invalid_input, instance_running, io |
| `world_delete_data` | `{id, world, scope: all\|nether\|end}` | `{ok}` (в корзину ОС, как instance_delete) | not_found, invalid_input, instance_running |
| `instance_screens` | `{id}` | `ShotInfo[]` {name, bytes, modified, thumb? (миниатюра data-URL, ширина 320)} | not_found, invalid_input |
| `screenshot_delete` | `{id, name}` | `{ok}` (прямое удаление — пересоздаваемый мусор) | not_found, invalid_input |
| `instance_screens_open` | `{id}` | `{ok}` (каталог не создаётся) | not_found, internal |
| `content_metadata` | `{id}` | `ModMetadata[]` {file, id?, missingDeps, disabledDeps, conflicts} (из fabric/quilt.mod.json jar'ов; окружение minecraft/fabricloader/java не считается missing) | not_found, invalid_input |
| `pack_art` | `{id, kinds[]}` | `PackArtInfo[]` {file, description?, png?} (pack.png/pack.mcmeta из zip; >300 КБ → png нет) | not_found |
| `instance_configs` | `{id}` | `ConfigFile[]` {rel, bytes} (config/, ≤4 компонентов пути, ≤500 файлов) | not_found, invalid_input |
| `config_read` | `{id, rel}` | `{rel, text}` (текст ≤1 МБ, lossy UTF-8) | not_found, invalid_input |
| `config_write` | `{id, rel, text}` | `{ok}` (запущено → instance_running; текст ≤1 МБ; каталог не создаётся) | not_found, invalid_input, instance_running, io |
| `authlib_server_info` | `{serverUrl}` | `AuthlibServerInfo` {serverUrl, serverName?} (https; http — только localhost) | invalid_input, network, offline_mode, http |
| `account_add_authlib` | `{serverUrl, username, password}` | `Account` (kind authlib, authlibServer нормализован; токен в keyring; сразу активен) | invalid_input, network, offline_mode, http, io, internal |
| `instance_shortcut` | `{id}` | `{path}` (.lnk на рабочем столе → mcl instance-launch --id … --no-wait) | not_found, invalid_input, internal |
| `game_resources` | `{id}` | `GameResourceSample \| null` {cpuPercent, ramBytes} (sysinfo, опрос UI 2 с) | — |
| `instance_launch_safe` | `{id, player}` | `{ok}` (F30: сброс JVM-флагов + отключение шейдеров, затем обычный запуск) | not_found, invalid_input |
| `instance_repair` | `{id}` | `RepairReport` {checked, redownloaded} (клиент+библиотеки+нативы+ассеты по хэшам; saves/config/mods не трогаются) | not_found, instance_running, network, http |
| `java_test_path` | `{javaExe}` | `JavaTestInfo` {major, bits, versionLine} (java -version, таймаут 5 с) | invalid_input, timeout |
| `log_share_mclogs` | `{id}` | `{url}` (хвост latest.log 512 КБ → redact → api.mclo.gs; офлайн → offline_mode) | not_found, network, offline_mode, http |
| `instance_logs_list` | `{id}` | `LogFileInfo[]` {name, bytes, modified} (*.log и *.log.gz кроме latest; .gz читается — хвост распакованного, D42) | not_found, invalid_input |
| `instance_log_read` | `{id, name}` | `string` (хвост 512 КБ, целые строки) | not_found, invalid_input |
| `content_snapshots` | `{id}` | `SnapshotInfo[]` {name, bytes, createdAt} (snapshots/mods-*.zip, ротация 3) | not_found, invalid_input |
| `content_rollback` | `{id, name}` | `ContentEntry[]` (манифестные файлы заменяются содержимым снапшота) | not_found, invalid_input, instance_running |
| `content_copy` | `{fromId, toId, file}` | `ContentEntry` (копия файла+записи манифеста; оба инстанса не запущены) | not_found, invalid_input, instance_running |
| `storage_stats` | — | `StorageStats` {instancesBytes, cacheBytes, trashBytes, logsBytes} | io |
| `storage_clean` | — | `{ok}` (`ok` = освобождено больше 0 байт; аннотация `client.ts` `{freedBytes}` не соответствует факту — байты не возвращаются) | io |
| `settings_export` | `{path}` | `{ok}` (JSON без curseforgeApiKey/azureClientId) | io |
| `settings_import` | `{path}` | `Settings` (validate+migrate; секреты из файла → None; schemaVersion принудительно текущая; прокси — тем же путём, что settings_set: битый → invalid_input, файл не пишется; применяются offline-флаг и лимит скорости) | invalid_input, io, json |
| `instance_force_unlock` | `{id}` | `{ok}` (мёртвый .lock без живого PID удаляется + событие Exited; запущено → instance_running) | not_found, instance_running, io |
| `account_skin` | `{accountId}` | `SkinInfo \| null` ({source: `ely`\|`mojang`, dataUrl} — PNG как data-URL; `null` — скина нет: offline/404. Источники allowlist: skinsystem.ely.by, textures.minecraft.net; дисковый кэш 1 ч) | not_found, invalid_input, network, http, json, internal, io |
| `skin_library_list` | — | `SkinInfo[]` (source `default`, D59: Steve/Alex × classic/slim из клиент-jar стора; id `steve-classic`/`alex-classic`/`steve-slim`/`alex-slim`; jar в сторе нет — `[]`) | — |
| `skin_user_list` | — | `SkinInfo[]` (source `user`; каталог `skins/` в данных лаунчера) | — |
| `skin_user_save` | `{path, name}` (path — PNG-файл из нативного диалога, только домашний каталог) | `SkinInfo` (валидация: PNG 64×64 или legacy 64×32, ≤ 50 КБ; имя санитизируется в id) | invalid_input, not_found, io, internal |
| `skin_user_delete` | `{skinId}` | `{ok}` (id — имя файла каталога `skins/`, traversal отсечён) | invalid_input, not_found, io |
| `skin_apply` | `{accountId, skinId, model: "classic"\|"slim"}` | `{ok}` (skinId — из `skin_user_list` или `skin_library_list`; msa → PUT Mojang `profile/skins` (403 = нет разрешения приложения — код `network`); ely → API ely.by; offline → invalid_input «только через бесплатный Ely.by», authlib → invalid_input; офлайн-режим → `offline_mode` до любых IO) | not_found, invalid_input, offline_mode, network, http, json, internal, io |

### Диагностика, Java, настройки, брендинг

| Команда | Вход | Выход | Коды ошибок |
|---|---|---|---|
| `crash_analyze` | `{logText}` | `Diagnosis[]` (правила — расширяемый JSON) | — |
| `java_list` | — | `JavaInstall[]` (PATH, Adoptium, Java, Zulu, `~/.jdks` + `runtime/` + `extraJavaPaths`) | — |
| `java_install` | `{major}` | `JavaInstall` (Adoptium JRE в `runtime/`, проверка хэша) | network, hash_mismatch, java_not_found, zip, io, internal |
| `settings_get` | — | `Settings` + `hasCurseforgeKey: boolean` (D62: значение `curseforgeApiKey` в рендер НЕ отдаётся — в памяти WebView секрету делать нечего; вместо него флаг «ключ задан») | — |
| `settings_set` | `{settings}` | `{ok}` (D62: отсутствие `curseforgeApiKey` = «не менялось», `Some("")` = сброс, значение = явная установка; пустой/пробельный `proxyUrl` нормализуется в None; смена `proxyUrl` пересоздаёт HTTP-клиент ядра, битый прокси → invalid_input и настройки НЕ сохраняются; `accountsActiveId` берётся с диска — его снимок с фронта не пишется) | invalid_input (некорректный proxyUrl), io, json |
| `data_dir` | — | `string` (корень данных лаунчера) | — |
| `logs_tail` | `{lines}` | `string` (хвост `launcher.log` до 2 МБ; файла нет → `""`) | io |
| `dir_open` | — | `{ok}` (открывает корень данных лаунчера) | internal |
| `logo_set_custom` | `{path}` | `{ok}` (D38: выбранный пользователем PNG логотипа копируется в каталог данных) | invalid_input (размер/формат), io |
| `apply_logo` | `{logo}` | `LogoResult` {ok, dataUrl} (D38: применяет PNG к иконке окна и возвращает data-URL для бренд-чипа интерфейса) | internal (приложение ещё не готово), invalid_input, io |

## Типы (зеркало Rust-структур, `src/api/types.ts`)

```ts
Instance {
  schemaVersion, id, name, icon?, art? (data-URL арта модпака),
  mcVersion, loader?, loaderVersion?, versionId?, javaPath?,
  ramMb, jvmFlags[], gameArgsExtra[], createdAt,
  lastPlayed?, playSeconds, launchCount, notes,
  quickPlayWorld?, quickPlayServer? (MC 1.20+),
  accountId? (профиль F11), crashCount (F30)
}
Settings {
  schemaVersion, theme: "dark"|"light", language: string (en|ru|uk|pl),
  downloadParallelism, defaultRamMb, defaultJvmFlags[], defaultResolution?,
  showSnapshots, showOldVersions, proxyUrl?, mirrorBase?,
  curseforgeApiKey? (только ВХОД settings_set; settings_get его не отдаёт),
  hasCurseforgeKey? (только ВЫХОД settings_get, D62),
  azureClientId?, accountsActiveId?, onboardingDone, extraJavaPaths[],
  uiScale (80–150, проценты), uiFont ("" | system|serif|mono|round),
  uiAccent ("" | #RRGGBB; валидация до записи),
  workOffline (F16), speedLimitKbps (F17, 0 — без лимита),
  discordRpc (F26), logo ("" | grass|copper|chest|honey|flat|custom)
}
Resolution { width, height }
VersionEntry { id, type: "release"|"snapshot"|"old_beta"|"old_alpha", releaseTime }
LoaderVersionEntry { version, stable }
JavaInstall { javaExe, major, archBits, origin }
Account { id, kind: "offline"|"msa"|"ely"|"authlib", name, uuid, refreshRef?,
          authlibServer? }        // authlib = вход через Ely.by/authlib-инжектор (F13);
                                  // authlibServer — нормализованный URL сервера
SearchHit { project_id, project_type, slug, author, title, description, categories[], versions[], downloads, icon_url?, date_modified } (Modrinth отдаёт snake_case)
SearchResult { hits[], total_hits }
ContentEntry { kind: "mod"|"resourcepack"|"shader"|"datapack",
               file, source: "modrinth"|"curseforge"|"local",
               projectId?, versionId?, sha1?, url?, enabled }
                    // file — путь относительно minecraft/ (`mods/sodium.jar`);
                    // datapack — F7: каталог minecraft/datapacks
UpdateCheck { file, projectId, currentVersionId, latestVersionId, latestVersionNumber, changelog? }
ArchiveResult { path, files, bytes }   // бэкап zip / экспорт .mrpack / бэкап мира
ModpackInstallResult { instance, group }
                    // mrpack_install / modpack_install (D64): инстанс + группа
                    // загрузок для dl_progress и downloads_cancel_group
RamGuide { totalMb, recommendedMb, warning? }
Diagnosis { ruleId, title, advice }
DeviceCodeStart { device_code, user_code, verification_uri, interval, expires_in }
                    // исключение: без camelCase (поля протокола OAuth);
                    // interval/expires_in — секунды опроса/жизни кода
SkinInfo { source: "ely"|"mojang"|"default"|"user", dataUrl }
SkinItemInfo { source, dataUrl, id?, model?: "classic"|"slim" }
                    // профильный скин (account_skin) — без id/model;
                    // id/model — у списков библиотеки (skin_library_list, D59)
                    // и каталога skins/ (skin_user_list): SkinLibraryItem/SkinUserItem
LogoResult { ok, dataUrl }             // apply_logo (D38)
```

Язык интерфейса по умолчанию — `en`; при старте его перекрывает ключ
`localStorage.lang` (`ru`/`en`/`uk`/`pl`) — конвенция приёмочных скриптов
(`scripts/*.cjs`), настройки меняются через `settings_set`.
Словари: `src/i18n/{en,ru,uk,pl}.json` — 4 языка, по 478 ключей в каждом,
паритет ключей проверен (D62-чистка; flat-ключи вида `mods.search`).

## События (listen)

| Событие | Payload |
|---|---|
| `dl_progress` | `{event:"dl_progress", group, instanceId?, doneFiles, totalFiles, doneBytes, totalBytes, bytesPerSec, etaSecs?}` — `group` = `instance:{id}` \| `mrpack:{...}` (D18) \| `jre:{major}` (D64: тихая докачка Java при запуске, рантайм общий — без `instanceId`); `instanceId` — владелец из группы, None в JSON не пишется (D60) |
| `dl_group_done` | `{event:"dl_group_done", group, instanceId?, failed}` (D62: группа задач дошла до конца — успех/ошибка/отмена; фронт снимает виртуальный статус занятости инстанса по нему, а не watchdog-тишиной; `failed` = есть проваленные задачи; `instanceId` опционален как в dl_progress; `jre:{major}` — как в dl_progress) |
| `dl_queue_state` | `{event:"dl_queue_state", pending, downloading, done, failed, cancelled, failedItems}` |
| `launch_state` | `{event:"launch_state", instanceId, phase:"preparing"\|"downloading"\|"launching"\|"running"\|"exited", exitCode?, launchStartedAt?}` (`launchStartedAt` — unix-ms старта ЭТОГО запуска, только у событий живого супервизора; D64: фронт отличает устаревший `exited` старого процесса от свежего `preparing` после быстрого перезапуска) |
| `game_log_line` | `{event:"game_log_line", instanceId, line, stream:"stdout"\|"stderr"}` |
| `account_refresh_failed` | `{event:"account_refresh_failed", accountId, reason}` |
| `shortcut_launch` | payload — `string` (id инстанса). НЕ событие шины ядра: эмитится плагином single-instance (`src-tauri/src/lib.rs:196`), когда второй процесс принёс аргументы ярлыка `instance-launch --id <id>`; слушается в `src/ui/layout/Shell.tsx:238`. Если окно ещё не создано, id ждёт в статике и забирается командой `shortcut_take_pending` |

События шины приходят в окно под своими именами (`spawn_event_bridge`,
`src-tauri/src/commands/mod.rs:1995`; маппинг имён — `emit_event`, там же
ниже), payload — `LauncherEvent` c тегом `event`. Подписка фронта —
`onCoreEvent` (`src/api/client.ts`), `dl_group_done` включён в список
слушателей и в union `CoreEvent` (`types.ts`).

## Ограничения UI (запрещено)

- Знать URL'ы, форматы (version JSON, mrpack), пути, хэши, имена файлов стора.
- Держать бизнес-логику (резолвы версий, вычисления RAM) — это ядро.
- Обращаться к fs/процессам в обход команд.
