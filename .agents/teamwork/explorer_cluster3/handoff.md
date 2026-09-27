# Handoff Report — Explorer Cluster 3

**Target:** Orchestrator (Conversation ID: `99709a5d-49a1-4bfb-89f4-5a051b9f4dcb`)  
**Mission:** Direction 4 (Frontend State & Leaks), Direction 5 (Frontend a11y & Navigation), Direction 6 (i18n Quality & Completeness)  
**Detailed Report:** `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster3\report.md`  

---

## 1. Observation
1. **Zustand unselected subscription in Shell:**
   - File `src/ui/layout/Shell.tsx:58`:
     `const { load, subscribeEvents, list, crashReport, clearCrashReport } = useInstances();`
     Calls `useInstances` with zero arguments/selectors, subscribing the root Shell to all mutations in `InstancesState` (including `logs` flushed every 200 ms and `dl_progress`).
2. **Crash analyze tail log race condition:**
   - File `src/state/instances.ts:147-156`:
     Synchronous `flushLogs()` on `exited` followed immediately by `api.crashAnalyze(logText)`.
   - File `src-tauri/src/instances/run.rs:694-710` & `src-tauri/src/commands/mod.rs:922-923`:
     `LaunchState::Exited` and `GameLogLine` are emitted on separate Tauri event channels. Reader threads can deliver late crash dump lines after `exited`, which are delayed by the 200 ms buffer timer and missed by `crashAnalyze`.
3. **LogViewerModal log source discrepancy:**
   - File `src/ui/components/LogViewerModal.tsx:28-31`: calls `api.logsTail(150)` when session logs are empty.
   - File `src-tauri/src/commands/mod.rs:938`: `logs_tail` reads `state.paths.logs_dir().join("launcher.log")` (the launcher's internal log, not `instances/<id>/logs/latest.log`).
4. **HomePage delete & kebab menu parity gap:**
   - File `src/ui/pages/HomePage.tsx:120-124`: still invokes `window.confirm(t("instances.confirmDelete", ...))` with `remove(inst.id, false)`.
   - File `src/ui/pages/HomePage.tsx:107-128`: does not supply `onBackup`, `onExport`, `onIconSet`, `onIconClear` callbacks to `InstanceRow`.
5. **Modal accessibility gaps:**
   - File `src/ui/components/CreateInstanceDialog.tsx:75-165`: has no `Escape` key listener, no backdrop click-to-close handler, no focus trap, no focus restoration.
   - All 7 modals (`AccountsModal`, `CrashReportModal`, `CreateInstanceDialog`, `InstanceSettingsModal`, `LogViewerModal`, `RenameInstanceModal`, `InstancesPage` delete modal): completely lack focus trap (`Tab` cycles out of dialog into background DOM) and focus restoration to trigger element.
   - File `src/ui/pages/SettingsPage.tsx:379-394`: `<Toggle>` (`role="switch"`) has no `aria-label` or `aria-labelledby`.
6. **i18n missing key & semantic defects:**
   - File `src/i18n/uk.json:120-125`: key `accounts.title` is completely missing (225 keys in `uk.json` vs 226 in `ru.json` and `en.json`).
   - File `src/i18n/uk.json:224`: `"instances.delete.trash": "Видалити в кошик (можно відновити)"` uses Russian word `"можно"`.
   - File `src/i18n/uk.json:147`: `"themes.title": "Тем"` uses genitive plural instead of nominative `"Теми"`.
   - File `src/ui/pages/HomePage.tsx:155` & `src/ui/pages/ModsPage.tsx:464`: hardcoded `.toLocaleString("ru", ...)` regardless of selected language.
   - File `scripts/*.cjs`: all Playwright E2E suites use hardcoded Russian regex/strings for role selectors, failing on default clean `en` installs.

---

## 2. Logic Chain
1. *From Obs 1:* `Shell.tsx:58` subscribes to the entire `useInstances` store -> `flushLogs()` calls `setState({ logs })` every 200 ms while a game is running -> `Shell` re-renders every 200 ms -> child page components (`HomePage`, `InstancesPage`, etc.) are not memoized, so the entire DOM tree re-renders every 200 ms. This invalidates the claim in AUDIT D8 / DECISIONS D29 that whole-tree re-renders were eliminated.
2. *From Obs 2:* In Tauri IPC, events on channel `game_log_line` and `launch_state` are handled asynchronously -> `LaunchState::Exited` triggers `flushLogs()` and `crashAnalyze` synchronously -> tail lines arriving in `queueLogLine` after `exited` are delayed by `LOG_FLUSH_MS` (200 ms) -> crash analysis runs without the final stack trace.
3. *From Obs 3:* `LogViewerModal` falls back to `api.logsTail` on empty memory buffer -> `api.logsTail` reads `launcher.log` instead of the instance's Minecraft `latest.log` -> users inspect launcher logs thinking they are debugging game crash logs.
4. *From Obs 4:* Decision D7/D28 implemented a 3-choice modal (`trash / forever / cancel`) and kebab actions (`backup / export / icon`) in `InstancesPage.tsx`, but failed to update `HomePage.tsx` -> users deleting instances from the Home page are forced into `wipe: false` with standard browser prompt.
5. *From Obs 5:* Web accessibility standards (WCAG 2.1) require modal dialogs to trap focus, close on Escape/overlay, and restore focus to trigger -> none of the modals implement focus trap or focus restoration, and `CreateInstanceDialog` ignores Escape.
6. *From Obs 6:* Decision D30 claimed complete parity across 226 keys in `ru`, `en`, `uk` -> `uk.json` has only 225 keys, missing `accounts.title`, causing fallback to English in Ukrainian UI. Furthermore, all E2E test scripts break on clean installs due to hardcoded Russian selectors.

---

## 3. Caveats
- No runtime execution or modifications of source code were performed (read-only audit per constraints).
- Production build CSP enforcement in Tauri release packages was not tested with live WebView binaries in this cluster.
- Pluralization analysis focused on Slavic (ru, uk) and English rules; other language expansions would require formal gettext or ICU MessageFormat tooling.

---

## 4. Conclusion
The frontend implementation exhibits significant state inefficiencies (whole-tree re-render in `Shell.tsx`), race conditions during game exit and crash analysis, accessibility non-compliance across all modal dialogs, and dictionary/test defects in i18n. Prior audit claims regarding D8 (whole-tree re-render eliminated) and D30 (complete 226-key dictionary parity) are partially refuted.

---

## 5. Verification Method
1. Verify missing `accounts.title` in `uk.json`:
   `node -e "const uk = require('./src/i18n/uk.json'); console.assert(uk['accounts.title'] === undefined);"`
2. Verify unselected Zustand hook in `Shell.tsx`:
   Inspect `src/ui/layout/Shell.tsx` line 58.
3. Verify `window.confirm` in `HomePage.tsx`:
   Inspect `src/ui/pages/HomePage.tsx` lines 120–124.
4. Verify hardcoded Russian locale in `HomePage.tsx`:
   Inspect `src/ui/pages/HomePage.tsx` line 155.
5. Invalidation condition:
   If `Shell.tsx:58` is wrapped in `useShallow`, `uk.json` contains `accounts.title`, and `CreateInstanceDialog.tsx` handles Escape, these respective findings are resolved.
