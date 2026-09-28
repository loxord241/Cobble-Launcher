# Handoff Report: Static Bug Hunt & Reliability Audit

**Auditor**: Teamwork Explorer (`explorer_bughunt`)  
**Parent Task ID**: `77618110-4f34-4be7-94e0-5e525096032d`  
**Report Artifact**: `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_bughunt\findings.md`

---

## 1. Observation

Direct code observations from read-only static analysis across the repository:

1. **Selector Regression in Acceptance Tests**:
   - `scripts/ui_checks.cjs:35,41`: `await page.locator(".group.grid").count()`
   - `scripts/e2e_features.cjs:20`: `await page.locator(".group.grid").count()`
   - `src/ui/components/InstanceCard.tsx:44`: `<div className="group flex h-full flex-col ...">`
   - `docs/DECISIONS.md:457`: `селекторы скриптов content/wave2 переведены со строк (.group.grid) на карточки`
2. **Acceptance Test Locale Mismatch**:
   - `docs/DECISIONS.md:339`: `Settings::default language = "en"`
   - `scripts/m4_acceptance.cjs:17`: `await page.waitForSelector("text=Добро пожаловать", { timeout: 15000 });`
   - `scripts/m5_acceptance.cjs:13,18`: `await page.waitForSelector("text=Главная")`, `text=Поиск модов на Modrinth…`
   - `scripts/kill_acceptance.cjs:10`: `page.getByRole("navigation").getByRole("button", { name: /^Инстансы/ })`
   - Unlike `ui_checks.cjs:29`, `content_features.cjs:15`, `wave2_features.cjs:15`, these scripts do not initialize `localStorage.setItem("lang", "ru")`.
3. **Active Account Deletion Fallback**:
   - `src-tauri/src/auth/mod.rs:80-92`: `auth::remove(paths, id)` removes account from `accounts.json` and keyring, but does not modify `settings.accounts_active_id`.
   - `src-tauri/src/auth/mod.rs:153-156`: `launch_identity` calls `active(paths).ok_or_else(...)`.
   - `src-tauri/src/commands/mod.rs:218-228`: Error caught, falls back to `crate::auth::offline::identity(&player)` where `player` is `"Player"`.
4. **Download Engine Registration Leak**:
   - `src-tauri/src/commands/mod.rs:63-65`: `engine_for` documentation: *«Работа с группой завершена → engine_done(group), чтобы не держать старый HttpClient»*.
   - Called with cleanup in `content_install:357`, `content_update_all:478`, `mrpack_install:519`, `modpack_install:545`, `instance_optimize:561`.
   - In `instance_launch` (`commands/mod.rs:214`): `let engine = state.engine_for(&group).await;` is called, but `state.engine_done(&group)` is never called anywhere in the function.
5. **Launch/Kill TOCTOU Race Condition**:
   - `src-tauri/src/commands/mod.rs:262-265`: `let cancelled = { let launching = state.launching.lock().await; !launching.contains(&inst_id_clone) };` (mutex released at line 265).
   - `src-tauri/src/commands/mod.rs:293`: `let spawn_res = instances::run::launch_detached(&state.paths, &instance_id, prepared, &state.bus);`
   - `src-tauri/src/commands/mod.rs:572-588`: `instance_kill` removes ID from `launching`, checks `running_pid` (which is `None` before line 293), emits `Exited`, and returns `Ok`. Then line 293 spawns Java process.
6. **Instance Settings Overwrite Race**:
   - `src/ui/components/InstanceSettingsModal.tsx:51-66`: Fetches `fresh` instance, merges RAM/JVM flags, and sends full `updated: Instance` to `instanceSettingsSet`.
   - `src-tauri/src/commands/mod.rs:181-184`: `instances::save(&paths, &instance)` completely overwrites `instance.json`.
   - `src-tauri/src/instances/run.rs:812-816`: Background process supervisor updates `play_seconds` and `launch_count` via atomic `instances::update`. If settings are saved after/during gameplay, supervisor counters are overwritten by stale client counters.
7. **Create Instance Rollback Absence**:
   - `src/ui/components/CreateInstanceDialog.tsx:66-72`: `api.instanceCreate` creates directory and `instance.json`. If `api.loaderInstall` fails, error is caught and displayed; no deletion or rollback of the created directory is performed.
8. **Inverted Boolean Condition**:
   - `src-tauri/src/instances/content.rs:82-84`:
     ```rust
     } else if !new_enabled {
         // Включаем, но активного файла нет: может лежать выключенным.
         if !crate::util::fs::long_path(&to).exists() {
             return Err(LauncherError::NotFound(from.to_string_lossy().into()));
         }
     ```
     `new_enabled` is `true` when enabling; therefore `!new_enabled` is `false`. The branch intended for enabling is skipped and drops to `else { return Err(NotFound) }`.
9. **Mod Operations During Active Gameplay**:
   - `src-tauri/src/commands/mod.rs:371-397`: `content_toggle` and `content_remove` lack `running_pid` verification.
   - `src/ui/pages/InstanceDetailPage.tsx:657,672`: Buttons disable only on `contentBusy`, ignoring `busy` (game running).
   - On Windows, modifying locked JAR files during gameplay raises OS error 32.
10. **Phantom Directory Creation**:
    - `src-tauri/src/commands/mod.rs:154-162`: `instance_open_dir` runs `std::fs::create_dir_all` before verifying that `instance.json` exists for that ID.
11. **Systemic Bypass of `apiErrorText`**:
    - `src/state/instances.ts:261`: `return { status, error: String((e as { message?: string })?.message ?? e) };`
    - `src/ui/pages/HomePage.tsx:214, 330`: `.catch((e) => setError(String(e)))`
    - `src/ui/pages/InstancesPage.tsx:83-100`: `.catch((e) => setError(String(e)))` across all row callbacks.
12. **Hardcoded Russian Constant**:
    - `src/ui/pages/SettingsPage.tsx:19, 207`: `const SLATE_LABEL = "Сланец";` rendered directly inside accent reset button.

---

## 2. Logic Chain

1. **Observation 1** shows `InstanceCard` changed the DOM hierarchy to `flex`, while `ui_checks.cjs` and `e2e_features.cjs` assert `.group.grid`. In JavaScript, `.count()` on a non-matching locator returns `0`. `before === after` (0 === 0) evaluates to true, hiding potential search breakages and continuously spawning unwanted test instances.
2. **Observation 2** shows that default settings have `language: "en"`. Acceptance scripts that do not prime localStorage with `"ru"` will fail on clean installs because the web view renders English text while Playwright awaits Russian string selectors.
3. **Observation 3** reveals that account removal leaves `settings.accounts_active_id` dangling. When launching, the launcher attempts to load the deleted active account, fails, and triggers fallback to the unconfigured default `"Player"`, wiping out custom skins, local worlds/stats associations, and server identities.
4. **Observation 4** indicates that while other content-handling functions unregister their engines via `state.engine_done(&group)`, `instance_launch` omits this step. Consequently, the download engine and its embedded HTTP client are retained indefinitely in memory, preventing proxy setting changes from taking effect on subsequent launches.
5. **Observation 5** establishes a critical concurrency window: `instance_launch` releases the `launching` mutex after `prepare()`. If `instance_kill` runs during this gap, it finds no active PID, emits `Exited` to the UI, and returns. `instance_launch` then proceeds to spawn the Java process, leaving it orphaned in the background.
6. **Observation 6** shows that saving instance settings from the modal overwrites the whole `Instance` struct using `instances::save`, whereas the supervisor updates `play_seconds` incrementally using `instances::update`. Opening the modal before or during a game session causes the modal to hold a stale snapshot, which overwrites and wipes the newly recorded playtime upon saving.
7. **Observation 7** shows that `api.instanceCreate` and `api.loaderInstall` are two separate unmanaged steps in `CreateInstanceDialog.tsx`. A network failure during loader installation leaves a broken instance directory on disk without rollback.
8. **Observation 8** confirms a logical contradiction: the code comment specifies enabling behavior, but the code evaluates `!new_enabled`. When enabling, `new_enabled == true`, so `!new_enabled` evaluates to false, immediately triggering a `NotFound` error.
9. **Observation 9** demonstrates that file-level operations (`rename`, `remove`) on JARs in `mods/` are permitted while the JVM process is alive, which Windows rejects with OS sharing violation error 32.
10. **Observation 10** proves that `instance_open_dir` creates folders unconditionally without validating instance existence, leaving orphan directories on disk when queried with invalid IDs.
11. **Observation 11** demonstrates that UI catch blocks format IPC errors using `String(e)` instead of `apiErrorText(e)`, leaking raw Russian backend strings to English and Ukrainian users.
12. **Observation 12** shows a hardcoded Cyrillic string in `SettingsPage.tsx` that bypasses the i18n translation system.

---

## 3. Caveats

1. **No Source Modification**: In accordance with the strict read-only audit mandate, no product code files in `src/` or `src-tauri/` were modified.
2. **No Live Execution**: Tests (`cargo test`, `npm run dev`) and launchers were not executed to avoid side effects in the host workspace.
3. **Excluded Ceilings**: Pre-established architectural ceilings and backlog items from §1.4 (MSA 403, unsigned auto-updater, A45 data-URL in instance.json, A17, A39, A44, A46, A48, A60, etc.) were deliberately excluded from this audit.

---

## 4. Conclusion

The audit identified 12 distinct, code-grounded bugs (7 P1, 4 P2, 1 P3). These bugs represent real reliability hazards (orphaned game processes, wiped playtime statistics, stale proxy leak, broken acceptance test contracts, and OS error 32 crashes on Windows). All findings are documented in detail with code citations and recommended remediations in `findings.md`.

---

## 5. Verification Method

To independently verify these findings:

1. **B1 (Selector Regression)**:
   - Inspect `scripts/ui_checks.cjs:35` and `src/ui/components/InstanceCard.tsx:44`. Verify that `InstanceCard` lacks the class `.grid`.
2. **B2 (Locale Acceptance)**:
   - Check `src-tauri/src/settings.rs` default language (`"en"`). Compare `scripts/m4_acceptance.cjs:17` with `scripts/ui_checks.cjs:29` to observe missing `localStorage.setItem("lang", "ru")`.
3. **B3 (Active Account Deletion)**:
   - Trace `src-tauri/src/auth/mod.rs:80` (`auth::remove`) and note the absence of any update to `settings.accounts_active_id`.
4. **B4 (Engine Leak)**:
   - Grep `engine_done` in `src-tauri/src/commands/mod.rs`. Verify that `instance_launch` (lines 210–297) does not call `engine_done`.
5. **B5 (TOCTOU Race)**:
   - Inspect `src-tauri/src/commands/mod.rs:262-296` and `571-589`. Trace the interleaving where `instance_kill` executes between lines 265 and 293.
6. **B6 (Playtime Overwrite)**:
   - Inspect `InstanceSettingsModal.tsx:51-66` and `src-tauri/src/commands/mod.rs:181-184`. Compare with `src-tauri/src/instances/run.rs:812-816`.
7. **B7 (Rollback Absence)**:
   - Inspect `CreateInstanceDialog.tsx:66-73` and confirm that `api.loaderInstall` error does not trigger `api.instanceDelete`.
8. **B8 (Inverted Condition)**:
   - Inspect `src-tauri/src/instances/content.rs:82` and compare condition `!new_enabled` with line 75 `let new_enabled = !entry.enabled`.
9. **B9 (Mod Locked JARs)**:
   - Inspect `src-tauri/src/commands/mod.rs:371-397` and confirm absence of `instances::running_pid(&dir)`.
10. **B10 (Phantom Directory)**:
    - Inspect `src-tauri/src/commands/mod.rs:154-162` and confirm `std::fs::create_dir_all` precedes instance validation.
11. **B11 (i18n Error Bypass)**:
    - Inspect `src/state/instances.ts:261` and `src/ui/pages/HomePage.tsx:214, 330` for `setError(String(e))`.
12. **B12 (Hardcoded Cyrillic)**:
    - Inspect `src/ui/pages/SettingsPage.tsx:19, 207`.
