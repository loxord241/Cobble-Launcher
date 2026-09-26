# UI-CONTRACT

Контракт «ядро ↔ UI» (спека §4.3). UI знает ТОЛЬКО этот список: имена команд,
типы входа/выхода и события. Смена UI = переписать `src/ui` + `src/theme`,
не трогая `src/api` и ядро.

Все ошибки команд приходят как `{code, message, hint?}` (`LauncherError.payload`),
код маппится в `src/i18n/errors.json`, `message` — fallback.

## Команды (invoke)

| Команда | Вход | Выход | Коды ошибок |
|---|---|---|---|
| `instance_list` | — | `Instance[]` | io, json |
| `instance_create` | `{mcVersion, name}` | `Instance` | version_not_found, invalid_input, io |
| `instance_rename` | `{id, name}` | `Instance` | not_found, invalid_input |
| `instance_duplicate` | `{id}` | `Instance` | not_found, io, internal |
| `instance_delete` | `{id, wipe}` | `{ok}` | not_found, internal |
| `instance_open_dir` | `{id}` | `{ok}` | not_found, internal |
| `instance_settings_get` | `{id}` | `Instance` | not_found |
| `instance_settings_set` | `{instance}` | `Instance` | invalid_input, io |
| `instance_launch` | `{id, player}` | `{ok}` (спавн мгновенный, прогресс — событиями) | version_not_found, java_not_found, hash_mismatch, network, zip, internal, invalid_input |
| `manifest_versions` | `{showSnapshots, showOld}` | `VersionEntry[]` | network, json |
| `loader_versions` | `{loader}` | `LoaderVersionEntry[]` | network, invalid_input |
| `loader_install` | `{id, loader, loaderVersion?}` | `Instance` | network, version_not_found, java_not_found, internal |
| `java_list` | — | `JavaInstall[]` | — |
| `java_install` | `{major}` | `JavaInstall` | network, hash_mismatch, java_not_found |
| `modrinth_search` | `{query, mcVersion?, loader?, projectType?, limit?, offset?}` | `SearchResult` (snake_case) | network, http |
| `content_install` | `{instanceId, projectId, versionId?}` | `ContentEntry[]` (обязательные зависимости ставятся рекурсивно) | network, version_not_found, hash_mismatch, invalid_input |
| `content_installed` | `{instanceId}` | `ContentEntry[]` | — |
| `content_update_check` | `{instanceId}` | `UpdateCheck[]` | network |
| `content_update_all` | `{instanceId}` | `number` (обновлено; старый jar → `.trash/`) | network, hash_mismatch |
| `mrpack_install` | `{path, name?}` | `Instance` (создаёт инстанс из dependencies) | network, version_not_found, zip, invalid_input |
| `settings_get` | — | `Settings` | — |
| `settings_set` | `{settings}` | `{ok}` | io |
| `data_dir` | — | `string` | — |
| `logs_tail` | `{lines}` | `string` | io |
| `dir_open` | — | `{ok}` | internal |

## Типы (зеркало Rust-структур, `src/api/types.ts`)

```ts
Instance {
  schemaVersion, id, name, icon?, mcVersion,
  loader?, loaderVersion?, versionId?, javaPath?,
  ramMb, jvmFlags[], gameArgsExtra[], createdAt,
  lastPlayed?, playSeconds, launchCount, notes
}
Settings {
  schemaVersion, theme: "dark"|"light", language, downloadParallelism,
  defaultRamMb, defaultJvmFlags[], defaultResolution?, showSnapshots,
  showOldVersions, proxyUrl?, mirrorBase?, curseforgeApiKey?,
  azureClientId?, accountsActiveId?, onboardingDone, extraJavaPaths[]
}
VersionEntry { id, type: "release"|"snapshot"|"old_beta"|"old_alpha", releaseTime }
LoaderVersionEntry { version, stable }
JavaInstall { javaExe, major, archBits, origin }
SearchHit { project_id, project_type, slug, author, title, description, categories[], versions[], downloads, icon_url?, date_modified } (Modrinth отдаёт snake_case)
SearchResult { hits[], total_hits }
ContentEntry { kind: "mod"|"resourcepack"|"shader", file, source: "modrinth"|"curseforge"|"local", projectId?, versionId?, sha1?, enabled }
UpdateCheck { file, projectId, currentVersionId, latestVersionId, latestVersionNumber, changelog? }
```

## События (listen)

| Событие | Payload |
|---|---|
| `dl_progress` | `{event:"dl_progress", doneFiles, totalFiles, doneBytes, totalBytes, bytesPerSec, etaSecs?}` |
| `dl_queue_state` | `{event:"dl_queue_state", pending, downloading, done, failed, cancelled, failedItems}` |
| `launch_state` | `{event:"launch_state", instanceId, phase:"preparing"\|"downloading"\|"launching"\|"running"\|"exited", exitCode?}` |
| `game_log_line` | `{event:"game_log_line", instanceId, line, stream:"stdout"\|"stderr"}` |
| `account_refresh_failed` | (M8) `{event:"account_refresh_failed", accountId, reason}` |

## Ограничения UI (запрещено)

- Знать URL'ы, форматы (version JSON, mrpack), пути, хэши, имена файлов стора.
- Держать бизнес-логику (резолвы версий, вычисления RAM) — это ядро.
- Обращаться к fs/процессам в обход команд.
