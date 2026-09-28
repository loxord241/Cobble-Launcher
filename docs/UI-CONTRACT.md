# UI-CONTRACT

Контракт «ядро ↔ UI» (спека §4.3). UI знает ТОЛЬКО этот список: имена команд,
типы входа/выхода и события. Смена UI = переписать `src/ui` + `src/theme`,
не трогая `src/api` и ядро.

Все ошибки команд приходят как `{code, message, hint?}` (`LauncherError.payload`),
код маппится в `src/i18n/errors.json`, `message` — fallback.
Полный реестр кодов (`src-tauri/src/errors.rs`): `network`, `hash_mismatch`,
`not_found`, `invalid_input`, `zip_slip`, `java_not_found`, `version_not_found`,
`cancelled`, `timeout`, `io`, `json`, `http`, `zip`, `internal`. Любая команда
дополнительно может вернуть `io`/`internal` (файловая система, фоновая задача).

Регистр ключей: camelCase — на обоих концах (`Instance`, `Settings`, события;
D25). Исключения — ответы Modrinth (`SearchHit`/`SearchResult`: как отдаёт API,
snake_case) и `DeviceCodeStart` (snake_case, как в протоколе OAuth).

## Команды (invoke)

Все 52 команды `invoke_handler` (`src-tauri/src/lib.rs:104`), обработчики —
`src-tauri/src/commands/mod.rs`.

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
| `instance_kill` | `{instanceId}` | `{ok}` (гасит процесс, отменяет активную загрузку группы `instance:<id>`, чистит `.lock`, шлёт `launch_state: exited`) | not_found, io, internal |
| `process_status` | — | `[id, pid][]` — запущенные инстансы | — |
| `instance_optimize` | `{instanceId}` | `string[]` (файлы стека; несовместимые версии честно пропускаются, не хардкодятся) | invalid_input (не Fabric-инстанс), version_not_found, network, hash_mismatch, io |
| `ram_guide` | — | `RamGuide` (рекомендация RAM по объёму памяти машины) | — |

### Контент, версии, загрузчики

| Команда | Вход | Выход | Коды ошибок |
|---|---|---|---|
| `modrinth_search` | `{query, mcVersion?, loader?, projectType?, limit?, offset?}` (по умолчанию limit 20, offset 0) | `SearchResult` (snake_case) | network, http, json |
| `content_install` | `{instanceId, projectId, versionId?}` | `ContentEntry[]` (обязательные зависимости ставятся рекурсивно) | network, http, version_not_found, hash_mismatch, invalid_input, io, json |
| `content_installed` | `{instanceId}` | `ContentEntry[]` (нет манифеста → `[]`) | — |
| `content_toggle` | `{instanceId, file}` | `ContentEntry` (на диске `x.jar` ↔ `x.jar.disabled`, логический путь в манифесте не меняется) | invalid_input (только `mods`/`resourcepacks`/`shaderpacks`, без traversal), not_found, io |
| `content_remove` | `{instanceId, file}` | `null` (файл + запись манифеста) | invalid_input, not_found, io |
| `content_update_check` | `{instanceId}` | `UpdateCheck[]` | network, http, io, json |
| `content_update_all` | `{instanceId}` | `number` (обновлено; старый jar → `.trash/`) | network, hash_mismatch, io, json |
| `mrpack_install` | `{path, name?}` | `Instance` (создаёт инстанс из dependencies; группа отмены `mrpack:<file_stem>`) | invalid_input, version_not_found, zip, zip_slip, java_not_found, network, hash_mismatch, cancelled, io, json |
| `modpack_install` | `{projectId, name?}` | `Instance` (последняя версия проекта → `.mrpack` → тот же установщик) | invalid_input, version_not_found, zip, zip_slip, java_not_found, network, hash_mismatch, cancelled, io, json |
| `instance_backup` | `{id}` | `ArchiveResult` (zip в `backups/`; сейвы входят, `.lock`/`logs`/`crash-reports`/`cache` — нет) | not_found, io, zip, internal |
| `instance_export` | `{id}` | `ArchiveResult` (`.mrpack` в `exports/`; сейвы НЕ входят, контент без url → `overrides/`) | not_found, io, json, zip, internal |
| `instance_icon_set` | `{id, path}` | `Instance` (PNG/JPEG/WebP ≤ 300 КБ) | invalid_input, not_found, io, internal |
| `instance_icon_remove` | `{id}` | `Instance` | not_found, io, internal |
| `instance_import` | `{path, name?}` | `Instance` (CF-zip / MultiMC-Prism / свой формат; распознанный загрузчик ставится сразу) | invalid_input, zip, zip_slip, version_not_found, java_not_found, network, hash_mismatch, io, json |
| `manifest_versions` | `{showSnapshots, showOld}` | `VersionEntry[]` | network, json, io |
| `loader_versions` | `{loader}` | `LoaderVersionEntry[]` (`fabric`/`quilt` — все, `neoforge` — 30 последних стабильных) | network, invalid_input, json |
| `loader_install` | `{id, loader, loaderVersion?}` | `Instance` (после установки `versionId` = профиль загрузчика) | invalid_input, version_not_found, java_not_found, network, hash_mismatch, zip, io, internal |

### Аккаунты

| Команда | Вход | Выход | Коды ошибок |
|---|---|---|---|
| `account_list` | — | `Account[]` | — |
| `account_add_offline` | `{name}` | `Account` (kind `offline`, uuid = offline-UUID ника; сразу активный) | invalid_input, io, json, internal |
| `account_add_msa_start` | — | `DeviceCodeStart` (snake_case: `device_code`, `user_code`, `verification_uri`, `interval`, `expires_in`) | invalid_input (нет `azureClientId`), network, http, json |
| `account_add_msa_poll` | `{deviceCode}` | `Account \| null` (`null` — пользователь ещё не ввёл код; refresh-токен → keyring) | invalid_input, network, http, json, internal |
| `account_add_msa_browser` | — | `Account` (auth-code flow: локальный порт 127.0.0.1, системный браузер, таймаут 300 с) | invalid_input, network, http, json, internal, io |
| `account_add_ely` | `{username, password}` | `Account` (kind `ely`; пароль не сохраняется, в keyring — refresh-токен) | invalid_input, network, http, json, internal |
| `account_remove` | `{id}` | `{ok}` (удаляет и запись в keyring) | not_found, io, internal |
| `account_active_set` | `{id}` | `{ok}` | not_found, io |
| `modrinth_projects` | `{ids: string[]}` | `ProjectMeta[]` (батч /v2/projects?ids=, чанки по 100; поле `id` алиасится в projectId) | invalid_input, network, http, json |
| `content_backfill_projects` | `{instanceId}` | `number` (сколько манифестов залечено sha1→projectId) | not_found, network, http, json |
| `update_check` | — | `UpdateInfo` {current, latest?, url?, updateAvailable, checked, note?} (GitHub releases/latest) | network, http, json |
| `java_recommended` | `{mcVersion}` | `number` (мажорная Java: 8/17/21, таблица как run.rs fallback_major) | — |
| `instance_force_unlock` | `{id}` | `{ok}` (мёртвый .lock без живого PID удаляется + событие Exited; запущено → instance_running) | not_found, instance_running, io |
| `account_skin` | `{accountId}` | `SkinInfo \| null` ({source: `ely`\|`mojang`, dataUrl} — PNG как data-URL; `null` — скина нет: offline/404. Источники allowlist: skinsystem.ely.by, textures.minecraft.net; дисковый кэш 1 ч) | not_found, invalid_input, network, http, json, internal, io |

### Диагностика, Java, настройки

| Команда | Вход | Выход | Коды ошибок |
|---|---|---|---|
| `crash_analyze` | `{logText}` | `Diagnosis[]` (правила — расширяемый JSON) | — |
| `java_list` | — | `JavaInstall[]` (PATH, Adoptium, Java, Zulu, `~/.jdks` + `runtime/` + `extraJavaPaths`) | — |
| `java_install` | `{major}` | `JavaInstall` (Adoptium JRE в `runtime/`, проверка хэша) | network, hash_mismatch, java_not_found, zip, io, internal |
| `settings_get` | — | `Settings` | — |
| `settings_set` | `{settings}` | `{ok}` (смена `proxyUrl` пересоздаёт HTTP-клиент ядра) | io, json |
| `data_dir` | — | `string` (корень данных лаунчера) | — |
| `logs_tail` | `{lines}` | `string` (хвост `launcher.log`; файла нет → `""`) | io |
| `dir_open` | — | `{ok}` (открывает корень данных лаунчера) | internal |

## Типы (зеркало Rust-структур, `src/api/types.ts`)

```ts
Instance {
  schemaVersion, id, name, icon?, mcVersion,
  loader?, loaderVersion?, versionId?, javaPath?,
  ramMb, jvmFlags[], gameArgsExtra[], createdAt,
  lastPlayed?, playSeconds, launchCount, notes
}
Settings {
  schemaVersion, theme: "dark"|"light", language: "en"|"ru"|"uk", downloadParallelism,
  defaultRamMb, defaultJvmFlags[], defaultResolution?, showSnapshots,
  showOldVersions, proxyUrl?, mirrorBase?, curseforgeApiKey?,
  azureClientId?, accountsActiveId?, onboardingDone, extraJavaPaths[],
  uiScale (масштаб интерфейса в процентах, 80–150)
}
Resolution { width, height }
VersionEntry { id, type: "release"|"snapshot"|"old_beta"|"old_alpha", releaseTime }
LoaderVersionEntry { version, stable }
JavaInstall { javaExe, major, archBits, origin }
Account { id, kind: "offline"|"msa"|"ely", name, uuid, refreshRef? }
                    // "offline" в UI помечается как нелицензионный профиль
SearchHit { project_id, project_type, slug, author, title, description, categories[], versions[], downloads, icon_url?, date_modified } (Modrinth отдаёт snake_case)
SearchResult { hits[], total_hits }
ContentEntry { kind: "mod"|"resourcepack"|"shader", file, source: "modrinth"|"curseforge"|"local", projectId?, versionId?, sha1?, url?, enabled }
                    // file — путь относительно minecraft/ (`mods/sodium.jar`)
UpdateCheck { file, projectId, currentVersionId, latestVersionId, latestVersionNumber, changelog? }
ArchiveResult { path, files, bytes }   // бэкап zip / экспорт .mrpack
RamGuide { totalMb, recommendedMb, warning? }
Diagnosis { ruleId, title, advice }
DeviceCodeStart { device_code, user_code, verification_uri, interval, expires_in }
                    // исключение: без camelCase (поля протокола OAuth)
```

Язык интерфейса по умолчанию — `en`; при старте его перекрывает ключ
`localStorage.lang` (`ru`/`en`/`uk`) — конвенция приёмочных скриптов
(`scripts/*.cjs`), настройки меняются через `settings_set`.

## События (listen)

| Событие | Payload |
|---|---|
| `dl_progress` | `{event:"dl_progress", doneFiles, totalFiles, doneBytes, totalBytes, bytesPerSec, etaSecs?}` |
| `dl_queue_state` | `{event:"dl_queue_state", pending, downloading, done, failed, cancelled, failedItems}` |
| `launch_state` | `{event:"launch_state", instanceId, phase:"preparing"\|"downloading"\|"launching"\|"running"\|"exited", exitCode?}` |
| `game_log_line` | `{event:"game_log_line", instanceId, line, stream:"stdout"\|"stderr"}` |
| `account_refresh_failed` | `{event:"account_refresh_failed", accountId, reason}` |

События приходят в окно под своими именами (`spawn_event_bridge`,
`src-tauri/src/commands/mod.rs:899`), payload — `LauncherEvent` c тегом `event`.

## Ограничения UI (запрещено)

- Знать URL'ы, форматы (version JSON, mrpack), пути, хэши, имена файлов стора.
- Держать бизнес-логику (резолвы версий, вычисления RAM) — это ядро.
- Обращаться к fs/процессам в обход команд.
