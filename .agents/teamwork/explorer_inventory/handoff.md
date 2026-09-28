# Handoff Report — Codebase Inventory Specialist (R1)

**Agent:** `explorer_inventory`  
**Working Directory:** `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_inventory\`  
**Target Artifact:** `findings.md`  
**Date:** 2026-09-28  
**Handoff Type:** Hard (Task complete)

---

## 1. Observation

1. **IPC Commands Discrepancy:**
   - `src-tauri/src/lib.rs:141-192` registers **50 commands** inside `tauri::generate_handler!`.
   - `docs/UI-CONTRACT.md:20` claims: *"Все 47 команд invoke_handler (src-tauri/src/lib.rs:104)"*.
   - 3 commands are present in `commands/mod.rs` and bound in `src/api/client.ts`, but completely absent from `docs/UI-CONTRACT.md`:
     * `modrinth_projects`: `src-tauri/src/commands/mod.rs:327`, `src/api/client.ts:76`
     * `content_backfill_projects`: `src-tauri/src/commands/mod.rs:459`, `src/api/client.ts:88`
     * `update_check`: `src-tauri/src/commands/mod.rs:1141`, `src/api/client.ts:117`
2. **Forge Omission in UI / `loader_versions`:**
   - `PARITY_AUDIT_PROMPT.md:65-66` claims: *"загрузчики fabric/forge/neoforge/quilt"*.
   - `src-tauri/src/commands/mod.rs:879-904` (`loader_versions`): matches only `"fabric"`, `"quilt"`, and `"neoforge"`. Passing `"forge"` returns `Err(LauncherError::InvalidInput("неизвестный загрузчик forge"))`.
   - `src/ui/components/CreateInstanceDialog.tsx:9-14`: `LOADERS` constant only contains `none`, `fabric`, `quilt`, `neoforge`.
3. **Dead Event (`dl_queue_state`):**
   - `docs/UI-CONTRACT.md:133` and `src/api/types.ts:139-142` document event `dl_queue_state`.
   - `src-tauri/src/events.rs:54` defines `LauncherEvent::DlQueueState(DlQueueState)`.
   - In `src-tauri/src/net/download.rs:381`, `queue_state()` builds `DlQueueState`, but no code ever emits `LauncherEvent::DlQueueState` to the `EventBus`.
4. **Orphaned IPC Command (`instance_import`):**
   - `src-tauri/src/commands/mod.rs:801-830` implements archive import (CF zip, MultiMC/Prism, Cobble zip).
   - `src/api/client.ts:49` exposes `api.instanceImport`.
   - `git grep "instanceImport" src/` finds zero occurrences across all components in `src/ui/`.
5. **Decisions D01–D35 Status:**
   - All 35 architectural decisions verified in actual code (e.g. D14 debug port removed from `tauri.conf.json`, D25 camelCase, D26 content toggle/backup/export, D28 engine groups, D31 security validations, D34 skins, D35 CF-style cards).
6. **Settings Struct Discrepancy:**
   - `src-tauri/src/settings.rs:57-60` contains `ui_font: String` and `ui_accent: String` (added in D32).
   - `src/api/types.ts:66-70` includes `uiFont` and `uiAccent`.
   - `docs/UI-CONTRACT.md:99-105` omits both fields.

---

## 2. Logic Chain

1. From `src-tauri/src/lib.rs:141-192`, counting each command in `generate_handler!` gives 50 commands. Comparing this list against `docs/UI-CONTRACT.md` reveals that `modrinth_projects`, `content_backfill_projects`, and `update_check` were added in later waves (D32, D33) without updating the contract document.
2. Tracing `loader_versions` in `src-tauri/src/commands/mod.rs:879-904` reveals that `"forge"` was never integrated into the version query handler. Because `CreateInstanceDialog.tsx:54` relies on `api.loaderVersions` to populate the loader version dropdown, Forge could not be supported in the dialog without that arm, leading to its complete exclusion from the UI `LOADERS` array.
3. Searching the entire codebase for `LauncherEvent::DlQueueState` showed that while the struct and enum variant exist and are handled in `emit_event`, the download engine only queries `queue_state` synchronously and never sends the event across `EventBus`, making it a dead contract.
4. Searching `src/ui/` for calls to `api.instanceImport` confirms that while `.mrpack` installation is wired to drag-and-drop and the Mods page, full archive import was implemented as a backend command (`commands/mod.rs:801`) and exposed in `client.ts:49`, but never given a UI button or modal.
5. All findings were correlated with the 11 functional domains to build the Master Map of Extension Points in `findings.md`.

---

## 3. Caveats

1. **Product Code Read-Only Constraint:** In accordance with audit rules, no code in `src/` or `src-tauri/` was modified.
2. **Offline Mode for OAuth:** Mojang MSA login could not be tested live due to known Mojang 403 API self-registration restrictions documented in §1.4.
3. **SmartScreen / Auto-Updater:** As documented in D32 and §1.4, auto-updater endpoints and code signing are awaiting owner keys (`scripts/gen-keys.ps1`).

---

## 4. Conclusion

The codebase is in a highly modular, clean state with strict adherence to architectural decisions D01–D35. The IPC surface consists of 50 commands and 5 events. The documentation (`docs/UI-CONTRACT.md`) lags behind actual implementation by 3 commands and 2 types. The master extension points map across the 11 domains provides ready recipes for feature expansion (instance groups, world management, datapacks, custom Yggdrasil auth, download speed limits, and UI log sharing).

Full technical details and file:line citations are recorded in:
`c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_inventory\findings.md`

---

## 5. Verification Method

To verify the findings independently:
1. Count commands in `src-tauri/src/lib.rs:141-192`:
   `git grep -n "commands::" src-tauri/src/lib.rs` (exactly 50 commands).
2. Check missing commands in `docs/UI-CONTRACT.md`:
   `git grep "modrinth_projects" docs/UI-CONTRACT.md` (no matches).
3. Check Forge exclusion:
   `git grep -n "forge" src-tauri/src/commands/mod.rs` (lines 874-905 show only fabric, quilt, neoforge).
   `git grep -n "LOADERS" src/ui/components/CreateInstanceDialog.tsx` (lines 9-14 omit forge).
4. Check dead event:
   `git grep -n "LauncherEvent::DlQueueState" src-tauri/src/` (only appears in `commands/mod.rs:998`).
5. Check orphaned import command:
   `git grep -n "instanceImport" src/` (only appears in `src/api/client.ts:49`).
