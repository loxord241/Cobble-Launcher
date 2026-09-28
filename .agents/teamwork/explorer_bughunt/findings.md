# Static Bug Hunt & Reliability Audit Findings (B1–B12)

**Auditor**: Teamwork Explorer (`explorer_bughunt`)  
**Target Repository**: `mc-launcher-v2` (`src/`, `src-tauri/`, `scripts/*.cjs`, `docs/`)  
**Methodology**: Read-only static analysis, code verification via direct line inspections, logic chain tracing, CDP test contract audit.

---

## Executive Summary

A comprehensive static code analysis of the product frontend (`src/`), backend core (`src-tauri/`), acceptance scripts (`scripts/`), and specification contracts (`docs/`) was executed. A total of **12 confirmed, code-verified bugs** were identified:
- **7 Priority 1 (P1)** bugs: concurrency race conditions, memory/resource leaks, data loss hazards, broken acceptance test contracts, and dangling identities.
- **4 Priority 2 (P2)** bugs: inverted boolean logic, Windows OS file locking violations during active gameplay, phantom directory creation, and systemic i18n error localization bypass.
- **1 Priority 3 (P3)** bugs: hardcoded Russian string constants in settings UI.

None of these findings re-open known ceilings or backlog items (§1.4: MSA 403, unsigned auto-updater, A45 data-URL in instance.json, A17, A39, A44, A46, A48, A60, A16-tail, Installed tab file names, Versions/Changelog tabs, Modrinth filter/sort, saves/worlds, MC version change, mod copy, instance grouping, MSA in-game skins).

---

## Detailed Bug Findings

### B1. [P1] `scripts/ui_checks.cjs:35,41` & `scripts/e2e_features.cjs:20` — Selector regression after D35 InstanceCard refactor breaks acceptance assertions
- **Файл:строка**:
  - `scripts/ui_checks.cjs:35,41`
  - `scripts/e2e_features.cjs:20`
  - Referenced in `src/ui/components/InstanceCard.tsx:44` & `docs/DECISIONS.md:457`
- **Сценарий**:
  In D35, `InstanceRow` was replaced with `InstanceCard`. The element root class changed from `.group.grid` to `.group.flex`, and the parent container is `.grid` (`InstancesPage.tsx:168`).
  1. In `scripts/ui_checks.cjs`:
     ```javascript
     35: const before = await page.locator(".group.grid").count();
     ...
     41: const after = await page.locator(".group.grid").count();
     42: before === after ? ok(`поиск: очистка вернула ${before} строк`) : fail(`строки: ${before} -> ${after}`);
     ```
     `page.locator(".group.grid").count()` returns `0`. `before` is `0`, `after` is `0`. The test reports `поиск: очистка вернула 0 строк` and passes as a false positive without validating search filtering.
  2. In `scripts/e2e_features.cjs`:
     ```javascript
     20: const rowsCount = await page.locator(".group.grid").count();
     21: if (rowsCount === 0) {
     22:   console.log("Создаем тестовый инстанс...");
     ...
     ```
     Because `rowsCount` is always 0 even when instances exist, it redundantly creates a new test instance on every test execution.
  3. `docs/DECISIONS.md:457` notes that `content` and `wave2` scripts were updated to card selectors (`[role="button"][aria-label^="Открыть "]`), but `ui_checks.cjs` and `e2e_features.cjs` were omitted.
- **Ожидание vs факт**:
  - *Ожидание*: Acceptance scripts use selectors matching rendered DOM elements (`page.locator('[role="button"][aria-label^="Открыть "]')` or `.group.flex`), accurately counting visible instances.
  - *Факт*: Legacy `.group.grid` selector matches 0 elements, generating false-positive test passes in `ui_checks.cjs` and redundant instance creation in `e2e_features.cjs`.
- **Рекомендация**: Update selectors in `ui_checks.cjs:35,41` and `e2e_features.cjs:20` to `page.locator('[role="button"][aria-label^="Открыть "]')`.

---

### B2. [P1] `scripts/m4_acceptance.cjs:17`, `scripts/m5_acceptance.cjs:13,18`, `scripts/kill_acceptance.cjs:10` — Clean installation locale mismatch causes immediate timeout failure
- **Файл:строка**:
  - `scripts/m4_acceptance.cjs:17`
  - `scripts/m5_acceptance.cjs:13,18`
  - `scripts/kill_acceptance.cjs:10`
  - Referenced in `docs/DECISIONS.md:339` & `src-tauri/src/settings.rs`
- **Сценарий**:
  According to D30, clean installations default to English (`language: "en"`).
  In `scripts/ui_checks.cjs:29`, `scripts/content_features.cjs:15`, and `scripts/wave2_features.cjs:15`, the scripts explicitly ensure deterministic locale by running:
  ```javascript
  await page.evaluate(() => localStorage.setItem("lang", "ru"));
  ```
  However, `m4_acceptance.cjs`, `m5_acceptance.cjs`, and `kill_acceptance.cjs` omit this setup. On clean runs, they wait for Russian strings:
  ```javascript
  // m4_acceptance.cjs:17
  await page.waitForSelector("text=Добро пожаловать", { timeout: 15000 });
  
  // m5_acceptance.cjs:13,18
  await page.waitForSelector("text=Главная", { timeout: 15000 });
  await page.waitForSelector("text=Поиск модов на Modrinth…", { timeout: 15000 });

  // kill_acceptance.cjs:10
  await page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ }).click();
  ```
- **Ожидание vs факт**:
  - *Ожидание*: Clean acceptance tests either run against the default English UI (e.g. "Welcome", "Home", "Instances") or explicitly set `localStorage.setItem("lang", "ru")` before asserting Russian texts.
  - *Факт*: Tests hang and fail with 15000ms timeout on clean installations because the UI renders English text.
- **Рекомендация**: Add `await page.evaluate(() => localStorage.setItem("lang", "ru"));` and `await page.reload()` at the start of `m4_acceptance.cjs`, `m5_acceptance.cjs`, and `kill_acceptance.cjs`.

---

### B3. [P1] `src-tauri/src/auth/mod.rs:80-92,153-156` & `src/state/accounts.ts:49-52` — Active account deletion leaves orphaned settings ID causing fallback to "Player"
- **Файл:строка**:
  - `src-tauri/src/auth/mod.rs:80-92, 153-156`
  - `src/state/accounts.ts:49-52`
  - `src-tauri/src/commands/mod.rs:218-230`
- **Сценарий**:
  1. A user creates offline accounts (e.g., "Alice" and "Bob") and sets "Alice" as active (`settings.accounts_active_id = "alice-uuid"`).
  2. The user deletes "Alice" via UI `AccountsModal` -> `api.accountRemove(id)`.
  3. `auth::remove(paths, id)` removes "Alice" from `accounts.json` and deletes credentials from keyring.
  4. Neither `auth::remove` in Rust nor `accounts.ts:remove` in frontend updates or clears `settings.accounts_active_id`.
  5. The user launches an instance. `commands::instance_launch` calls `crate::auth::launch_identity(&state.paths, &http)`:
     ```rust
     // auth/mod.rs:153-156
     pub async fn launch_identity(...) -> Result<AccountIdentity> {
         let acc = active(paths).ok_or_else(|| LauncherError::Auth("нет активного аккаунта".into()))?;
     ...
     ```
     `auth::active` searches for `settings.accounts_active_id` in `accounts.json`. Because "Alice" was deleted, `active()` returns `None`.
  6. `commands::instance_launch` catches the error and executes fallback:
     ```rust
     // commands/mod.rs:228
     crate::auth::offline::identity(&player)
     ```
     Because `player` is passed as `"Player"` when no active account exists in store, the game launches with username `"Player"`.
- **Ожидание vs факт**:
  - *Ожидание*: Deleting the active account automatically promotes the next remaining account (or clears active ID and requires account selection), preventing silent launch under the generic name "Player".
  - *Факт*: `settings.accounts_active_id` remains pointing to a dead account ID; launch silently drops player identity, joining servers with "Player" and losing local player data/UUID.
- **Рекомендация**: In `auth::remove`, if the removed account ID matches `settings.accounts_active_id`, automatically update `settings.accounts_active_id` to the ID of the first remaining account (or `None`), and synchronize frontend `useAccounts` state.

---

### B4. [P1] `src-tauri/src/commands/mod.rs:214, 257-296` — Download engine registration leak in `instance_launch` retains stale HTTP clients
- **Файл:строка**:
  - `src-tauri/src/commands/mod.rs:63-86, 214, 257-296, 357, 478, 519, 545, 561`
- **Сценарий**:
  In `AppState`, `engine_for(group)` registers an `Arc<DownloadEngine>` in `AppState.engines` so that `instance_kill` / cancellation can abort active downloads. The explicit contract specified in comments (`commands/mod.rs:63-65`) and implemented in every other command is:
  ```rust
  /// Движок для группы с регистрацией в реестре. Работа с группой
  /// завершена → `engine_done(group)`, чтобы не держать старый HttpClient.
  pub async fn engine_for(&self, group: &str) -> Arc<DownloadEngine>
  ```
  Commands `content_install` (line 357), `content_update_all` (line 478), `mrpack_install` (line 519), `modpack_install` (line 545), and `instance_optimize` (line 561) all call `state.engine_done(&group)`.
  However, in `instance_launch`:
  ```rust
  214: let engine = state.engine_for(&group).await;
  ...
  240: let prepared = instances::run::prepare(..., engine, ...).await?;
  ...
  296: Ok(OkMsg { ok: true })
  ```
  `state.engine_done(&group)` is NEVER called—neither on successful launch, preparation failure, nor cancellation.
- **Ожидание vs факт**:
  - *Ожидание*: Once preparation completes or errors out, `state.engine_done(&group)` is invoked to unregister the engine from `AppState.engines`.
  - *Факт*: `Arc<DownloadEngine>` remains registered in `AppState.engines` indefinitely. If the user changes network proxy settings (`settings_set`), subsequent launches of that instance retrieve the cached engine with the stale `reqwest::Client`, bypassing the new proxy configuration and leaking connection pool memory.
- **Рекомендация**: Wrap preparation in `instance_launch` with a defer/scope-guard or call `state.engine_done(&group)` before returning.

---

### B5. [P1] `src-tauri/src/commands/mod.rs:262-296` vs `571-589` — Concurrency race condition between `instance_kill` and `instance_launch` leads to orphaned game process
- **Файл:строка**:
  - `src-tauri/src/commands/mod.rs:262-296`
  - `src-tauri/src/commands/mod.rs:571-589`
- **Сценарий**:
  A user clicks "Play", and while the assets/libraries are finishing preparation, clicks "Stop" (or automated supervisor invokes `instance_kill`).
  1. `instance_launch` finishes `prepare()` (line 259).
  2. At lines 262-265, `instance_launch` checks cancellation:
     ```rust
     let cancelled = {
         let launching = state.launching.lock().await;
         !launching.contains(&inst_id_clone)
     };
     ```
     `cancelled` evaluates to `false`. The lock on `state.launching` is released.
  3. At this precise instant (between line 265 and line 293), `instance_kill` runs:
     ```rust
     let was_launching = {
         let mut launching = state.launching.lock().await;
         launching.remove(&instance_id)
     };
     if was_launching {
         state.cancel_engine_group(...);
         state.bus.emit(crate::events::LauncherEvent::LaunchState {
             instance_id: instance_id.clone(),
             phase: crate::events::LaunchPhase::Exited,
             exit_code: None,
         });
         let inst_dir = instances::instance_dir(&state.paths, &instance_id);
         if instances::running_pid(&inst_dir).is_some() {
             let _ = instances::kill(&inst_dir);
         }
         return Ok(OkMsg { ok: true });
     }
     ```
     `instance_kill` removes `instance_id` from `launching`, finds `running_pid == None` (since Java hasn't spawned yet), emits `Exited` to the UI, and returns `Ok`.
  4. `instance_launch` resumes at line 293:
     ```rust
     let spawn_res = instances::run::launch_detached(&state.paths, &instance_id, prepared, &state.bus);
     state.launching.lock().await.remove(&inst_id_clone);
     spawn_res?;
     ```
     `launch_detached` creates `.lock` and executes `Command::spawn()`.
- **Ожидание vs факт**:
  - *Ожидание*: If `instance_kill` succeeds during launch preparation, no child process is spawned.
  - *Факт*: `instance_kill` reports success and notifies the UI that the instance exited, but `instance_launch` spawns the Minecraft Java process immediately afterwards in an unmonitored background state.
- **Рекомендация**: Hold the `state.launching` lock across the spawn check and detached invocation, or verify `launching.contains()` inside the spawn critical section.

---

### B6. [P1] `src-tauri/src/commands/mod.rs:173-188` vs `src/ui/components/InstanceSettingsModal.tsx:51-66` vs `src-tauri/src/instances/run.rs:812-816` — Non-atomic instance settings save overwrites supervisor statistics
- **Файл:строка**:
  - `src-tauri/src/commands/mod.rs:173-188`
  - `src/ui/components/InstanceSettingsModal.tsx:51-66`
  - `src-tauri/src/instances/run.rs:812-816`
  - `src-tauri/src/instances/mod.rs:232-243`
- **Сценарий**:
  `docs/DECISIONS.md:369` (D31.8) introduced `instances::update` to ensure atomic updates to `instance.json` for gameplay statistics (`play_seconds`, `launch_count`, `last_played`).
  However, `instance_settings_set` was not refactored:
  ```rust
  // commands/mod.rs:181-184
  instances::save(&paths, &instance)?;
  ```
  In the frontend `InstanceSettingsModal.tsx:51-66`:
  ```typescript
  const fresh = await api.instanceSettingsGet(instance.id);
  const updated: Instance = {
    ...fresh,
    ramMb,
    javaPath: ...,
    ...
  };
  const res = await api.instanceSettingsSet(updated);
  ```
  If a user opens the Instance Settings modal while the game is running (e.g. to adjust RAM or notes for the next launch), and saves after the game session ends:
  1. `instanceSettingsGet` reads `playSeconds = 120`.
  2. The game process exits. Background supervisor in `run.rs:812-816` calls `instances::update` and increments `playSeconds` to `7320` (+2 hours).
  3. The user clicks "Save" in `InstanceSettingsModal`.
  4. The frontend sends the entire `updated` object (`playSeconds: 120`) to `instance_settings_set`.
  5. `instances::save` overwrites `instance.json` with 120, obliterating 2 hours of recorded gameplay.
- **Ожидание vs факт**:
  - *Ожидание*: Instance settings modification updates only editable fields (RAM, JVM flags, Java path, game args, notes), preserving server-managed counters.
  - *Факт*: `instance_settings_set` overwrites the entire file from client state, creating a data-race that resets gameplay statistics.
- **Рекомендация**: Change `instance_settings_set` to take an `InstanceSettingsPatch` struct and apply changes via `instances::update(&paths, &id, |inst| { ... })`.

---

### B7. [P1] `src/ui/components/CreateInstanceDialog.tsx:66-73` — Multi-step instance creation lacks rollback on loader installation failure
- **Файл:строка**:
  - `src/ui/components/CreateInstanceDialog.tsx:66-73`
  - `src-tauri/src/commands/mod.rs:118-120`
- **Сценарий**:
  A user creates an instance with a mod loader (e.g. Fabric or NeoForge):
  ```typescript
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const inst = await api.instanceCreate(version, name || version);
      if (loader !== "none") {
        await api.loaderInstall(inst.id, loader, loaderVersion || undefined);
      }
      onCreated();
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };
  ```
  `api.instanceCreate` executes first, writing `instance.json` and directory structure to disk. If `api.loaderInstall` subsequently fails (e.g. timeout fetching Maven artifacts, offline network, loader service 503), execution jumps to `catch (e)`.
- **Ожидание vs факт**:
  - *Ожидание*: Transactional creation: if loader installation fails, the newly created directory is rolled back (analogous to `rollback_instance` in `mrpack.rs`), or the user is prompted to retry loader setup.
  - *Факт*: An abandoned vanilla instance remains registered on disk and in the UI. If the user clicks "Create" again with the same name, duplicate or colliding instances are created while leaving corrupted instances behind.
- **Рекомендация**: In `CreateInstanceDialog.tsx:71`, catch the error and invoke `api.instanceDelete(inst.id, true)` before displaying the error, or move loader installation into an atomic backend creation command.

---

### B8. [P2] `src-tauri/src/instances/content.rs:82-89` — Inverted boolean branch condition in `content::toggle`
- **Файл:строка**:
  - `src-tauri/src/instances/content.rs:80-89`
- **Сценарий**:
  ```rust
  75: let new_enabled = !entry.enabled;
  76: let base = minecraft_dir(&instance_dir(paths, id));
  77: let from = base.join(disk_path(file, entry.enabled));
  78: let to = base.join(disk_path(file, new_enabled));
  79: let from_long = crate::util::fs::long_path(&from);
  80: if from_long.exists() {
  81:     std::fs::rename(from_long, crate::util::fs::long_path(&to))?;
  82: } else if !new_enabled {
  83:     // Включаем, но активного файла нет: может лежать выключенным.
  84:     if !crate::util::fs::long_path(&to).exists() {
  85:         return Err(LauncherError::NotFound(from.to_string_lossy().into()));
  86:     }
  87: } else {
  88:     return Err(LauncherError::NotFound(from.to_string_lossy().into()));
  89: }
  ```
  The author intended to handle the case where a mod is being enabled (`new_enabled == true`), but the source file on disk is already in the target state. The code comment explicitly states:
  `// Включаем, но активного файла нет: может лежать выключенным.`
  However, line 82 checks `else if !new_enabled`.
  When enabling (`entry.enabled == false` -> `new_enabled == true`), `!new_enabled` evaluates to `false`!
- **Ожидание vs факт**:
  - *Ожидание*: Enabling a mod checks whether the file already exists in enabled form.
  - *Факт*: When enabling, line 82 is skipped and execution falls into `else { return Err(LauncherError::NotFound(...)); }`, throwing a `NotFound` error even though the target file exists. When disabling (`new_enabled == false`), `!new_enabled` is true, executing the check in reverse.
- **Рекомендация**: Change line 82 from `else if !new_enabled` to `else if new_enabled`.

---

### B9. [P2] `src-tauri/src/commands/mod.rs:371-397` & `src/ui/pages/InstanceDetailPage.tsx:657,672` & `src/ui/components/InstanceKebabMenu.tsx:126-133` — Content modification allowed during active gameplay causes Windows file locking crashes
- **Файл:строка**:
  - `src-tauri/src/commands/mod.rs:371-397, 552-563`
  - `src/ui/pages/InstanceDetailPage.tsx:657, 672`
  - `src/ui/components/InstanceKebabMenu.tsx:126-133`
- **Сценарий**:
  `instances::rename` and `instances::duplicate` explicitly guard against operations during gameplay via `instances::running_pid(&dir).is_some()`.
  However, `content_toggle` (line 371), `content_remove` (line 386), and `instance_optimize` (line 552) contain NO checks for `running_pid`.
  In the frontend:
  1. In `InstanceDetailPage.tsx:657` and `672`, the mod toggle switch and delete button only check `contentBusy`, not `busy` (which tracks whether the game is currently running).
  2. In `InstanceKebabMenu.tsx:126-133`, `onOptimize` does not specify `disabled={busy}`.
  On Windows, running JVM processes hold exclusive file locks on active JAR files in `mods/`.
- **Ожидание vs факт**:
  - *Ожидание*: Toggling, deleting, or optimizing mods is disabled in the UI while the game is running, and rejected by the backend with an informative error.
  - *Факт*: Users can toggle or delete active mods while playing Minecraft, causing raw Windows I/O sharing violations (`os error 32: The process cannot access the file because it is being used by another process`).
- **Рекомендация**: Add `if instances::running_pid(&dir).is_some()` checks to `content_toggle`, `content_remove`, and `instance_optimize` in `commands/mod.rs`, and disable the corresponding UI controls when `busy` is true.

---

### B10. [P2] `src-tauri/src/commands/mod.rs:154-162` — `instance_open_dir` creates phantom directory before verifying instance existence
- **Файл:строка**:
  - `src-tauri/src/commands/mod.rs:154-162`
  - `docs/UI-CONTRACT.md`
- **Сценарий**:
  ```rust
  154: pub async fn instance_open_dir(state: State<'_, AppState>, id: String) -> Result<OkMsg> {
  155:     // Без валидации id `..\..\Windows` открывал бы произвольный каталог/файл
  156:     // через opener (A23).
  157:     instances::valid_id(&id)?;
  158:     let dir = instances::instance_dir(&state.paths, &id);
  159:     std::fs::create_dir_all(crate::util::fs::long_path(&dir))?;
  160:     opener::open(dir).map_err(|e| LauncherError::Internal(format!("открытие папки: {e}")))?;
  161:     Ok(OkMsg { ok: true })
  162: }
  ```
  `instance_open_dir` validates the syntax of `id` (`[a-zA-Z0-9_-]+`), but does NOT check if the instance exists (`instances::load(&state.paths, &id)` or `instance.json` existence). Instead, it calls `std::fs::create_dir_all`.
- **Ожидание vs факт**:
  - *Ожидание*: In accordance with `docs/UI-CONTRACT.md`, calling `instance_open_dir` with a non-existent ID returns `Err(LauncherError::NotFound)`.
  - *Факт*: Invoking `instance_open_dir` with an arbitrary non-existent ID creates an empty ghost directory under `%LOCALAPPDATA%\mc-launcher-v2\instances\<id>`, opens Windows File Explorer pointing to the empty directory, and returns `Ok`.
- **Рекомендация**: Check `instances::load(&state.paths, &id)?` before calling `create_dir_all` and `opener::open`.

---

### B11. [P2] `src/state/instances.ts:257-264`, `src/ui/pages/HomePage.tsx:214,330`, `src/ui/pages/InstancesPage.tsx:83-100` — Systemic bypass of `apiErrorText` in launch and instance action error handlers
- **Файл:строка**:
  - `src/state/instances.ts:261`
  - `src/ui/pages/HomePage.tsx:214-231, 330`
  - `src/ui/pages/InstancesPage.tsx:83-100`
  - `src/i18n/index.ts:125` & `src/i18n/errors.json`
- **Сценарий**:
  D30 and D31 introduced `apiErrorText(e)` to translate backend IPC errors (such as `java_not_found`, `version_not_found`, `network`, `auth`, `busy`) into the active UI language (en, uk, ru) using `errors.json`.
  While `InstancesPage.tsx:109` and `HomePage.tsx:240` use `apiErrorText` for `onIconSet`, all primary actions bypass it:
  1. In `src/state/instances.ts:261`:
     ```typescript
     return { status, error: String((e as { message?: string })?.message ?? e) };
     ```
  2. In `src/ui/pages/HomePage.tsx:214`:
     ```typescript
     onLaunch: () => void launch(inst.id, ...).catch((e) => setError(String(e)))
     ```
  3. In `src/ui/pages/HomePage.tsx:330`:
     ```typescript
     void launch(hero.id, ...).catch((e) => setError(String(e)))
     ```
  4. In `src/ui/pages/InstancesPage.tsx:83-100`:
     `onLaunch`, `onStop`, `onOptimize`, `onFolder`, `onDuplicate`, `onBackup`, `onExport` all catch errors with `.catch((e) => setError(String(e)))`.
- **Ожидание vs факт**:
  - *Ожидание*: All user-facing IPC errors are passed through `apiErrorText(e)` so that users on English or Ukrainian locales see localized error explanations.
  - *Факт*: Errors are formatted with `String(e)`, displaying unlocalized Russian error strings from the Rust core directly to English and Ukrainian users.
- **Рекомендация**: Replace all instances of `setError(String(e))` with `setError(apiErrorText(e))` in `HomePage.tsx`, `InstancesPage.tsx`, and `instances.ts`.

---

### B12. [P3] `src/ui/pages/SettingsPage.tsx:19, 206-208` — Hardcoded untranslated Russian string constant in accent reset button
- **Файл:строка**:
  - `src/ui/pages/SettingsPage.tsx:19, 206-208`
- **Сценарий**:
  In `SettingsPage.tsx`:
  ```typescript
  18: /** Имя системы тем — подпись пункта «вернуть как в теме» (бренд, не переводится). */
  19: const SLATE_LABEL = "Сланец";
  ...
  206: <button onClick={() => commit({ uiAccent: "" })} className="btn-ghost btn-sm">
  207:   {SLATE_LABEL}
  208: </button>
  ```
  The comment asserts that "Сланец" is an untranslatable brand name. In reality, "Сланец" is simply the Russian translation of the default Slate theme accent.
- **Ожидание vs факт**:
  - *Ожидание*: The reset button displays a localized label matching the active language (e.g., "Slate" or "Default" in English, "Сланець" in Ukrainian, "Сланец" in Russian) via `t("settings.accent.reset")`.
  - *Факт*: The Cyrillic word "Сланец" is permanently rendered on the Settings screen regardless of whether the user selected English or Ukrainian.
- **Рекомендация**: Add `settings.accent.reset` keys to `ru.json`, `en.json`, and `uk.json`, and replace `{SLATE_LABEL}` with `{t("settings.accent.reset")}`.

---

## Verification Matrix

| Bug ID | Severity | Module / Subsystem | Root Cause Category | Primary Invalidation / Test Condition |
|---|---|---|---|---|
| **B1** | P1 | `scripts/ui_checks.cjs`, `scripts/e2e_features.cjs` | Test Contract / Selector Drift | Run `node scripts/ui_checks.cjs` with 1+ instances; inspect selector match count |
| **B2** | P1 | `scripts/m4_acceptance.cjs`, `scripts/m5_acceptance.cjs` | Test Setup / Locale Assumption | Run on clean data root (`%LOCALAPPDATA%\mc-launcher-v2`); observe timeout on English strings |
| **B3** | P1 | `src-tauri/src/auth`, `src/state/accounts.ts` | State Lifecycle / Dangling Pointer | Delete active account from UI, launch instance; check launch player name |
| **B4** | P1 | `src-tauri/src/commands/mod.rs` | Resource Management / Cache Leak | Launch instance, update proxy settings; verify whether subsequent launch uses new proxy |
| **B5** | P1 | `src-tauri/src/commands/mod.rs` | Concurrency / TOCTOU Race Condition | Issue `instance_kill` immediately after `prepare()` finishes before `launch_detached` |
| **B6** | P1 | `commands/mod.rs`, `InstanceSettingsModal.tsx` | Atomicity / State Overwrite | Open settings modal during gameplay, exit game, save settings modal; observe `playSeconds` |
| **B7** | P1 | `src/ui/components/CreateInstanceDialog.tsx` | Error Handling / Partial State | Trigger network error on `loaderInstall`; check filesystem for orphaned instance directory |
| **B8** | P2 | `src-tauri/src/instances/content.rs` | Boolean Logic Inversion | Call `content_toggle` on an enabled mod whose source disabled file is absent |
| **B9** | P2 | `commands/mod.rs`, `InstanceDetailPage.tsx` | OS Concurrency / File Locking | Attempt to toggle or delete an installed mod while Minecraft process is executing |
| **B10** | P2 | `src-tauri/src/commands/mod.rs` | API Contract / Traversal Guard | Invoke `instance_open_dir("phantom-12345")`; check if directory was created |
| **B11** | P2 | `src/ui/pages/HomePage.tsx`, `InstancesPage.tsx` | Internationalization / UX Quality | Trigger launch error on English locale; inspect language of presented error alert |
| **B12** | P3 | `src/ui/pages/SettingsPage.tsx` | Internationalization / Hardcoded String | Switch language to English; inspect theme accent reset button label |
