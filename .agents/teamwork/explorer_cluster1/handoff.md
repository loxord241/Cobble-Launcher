# Handoff Report — Explorer Cluster 1 (R1 Verification & Direction 11)

**Working Directory:** `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster1`  
**Report Artifact:** `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster1\report.md` (50 KB detailed report)  
**Parent Agent:** `99709a5d-49a1-4bfb-89f4-5a051b9f4dcb` (parent)  
**Date:** 2026-09-27  

---

## 1. Observation

### 1.1 Tool Execution & Test Suite Results
1. **Rust Test Suite (`cargo test`):**
   - Command: `cargo test` in `src-tauri`
   - Output: 
     - Unit tests (`src-tauri/src/lib.rs`): 102 passed, 0 failed, 5 ignored (network/slow tests: `test_diagnose_msa_error_flow`, `test_curseforge_manifest_parsing_from_network`, `test_modrinth_manifest_parsing_from_network`, `test_msa_full_device_code_flow_simulated`, `test_launch_offline_pipeline`).
     - Integration tests (`tests/engine_tests.rs`): 10 passed, 0 failed, 0 ignored.
     - Total: **112 tests passed, 0 failed**.
2. **Clippy Static Analysis (`cargo clippy -- -D warnings`):**
   - Command: `cargo clippy --all-targets --all-features -- -D warnings` in `src-tauri`
   - Output: `Finished dev [unoptimized + debuginfo] target(s) in 2.37s`. **0 warnings, 0 errors**.
3. **Frontend Build (`npm run build`):**
   - Command: `npm run build` (tsc && vite build)
   - Output: `dist/assets/index-Bf6tG8iW.js 379.83 kB │ gzip: 104.97 kB`. **0 TypeScript errors, 0 build errors**.

---

### 1.2 R1 Verification (§1, §3, §5, §6 and DECISIONS.md D26–D30)
Across AUDIT.md and DECISIONS.md, 48 distinct items were verified with exact code references:

1. **Refuted Claims (Опровергнуто):**
   - **AUDIT.md §6 D15 & DECISIONS.md D29 («shutdown OAuth-listener по отмене»):**
     - Claim: A cancellation mechanism or shutdown triggers termination of the local HTTP server on port 58428.
     - Observation in Code (`src-tauri/src/commands/mod.rs:602-619` & `src-tauri/src/auth/msa.rs:322-362`):
       - `TcpListener::bind` is passed to `tokio::task::spawn_blocking(move || { for stream in listener.incoming() { ... } })`.
       - The blocking thread loops synchronously on `listener.incoming()`.
       - While `tokio::time::timeout(Duration::from_secs(300), rx)` handles frontend cancellation and timeout, it merely drops the receiver. The OS thread inside `spawn_blocking` remains blocked indefinitely on `accept()` until an incoming socket connection or process termination. No cancellation token or shutdown signal is passed to the listener.
       - *Verdict:* **ОПРОВЕРГНУТО (REFUTED)** -> Logged as **FINDING-C1-01 (P2)**.

2. **Partially Confirmed Claims (Частично подтверждено):**
   - **AUDIT.md §1 п. 14 & §6 D7 («Модалка подтверждения удаления с чекбоксом wipe»):**
     - Claim: Delete instance prompt was replaced everywhere with a full confirmation modal with wipe checkbox.
     - Observation in Code:
       - Implemented in `src/pages/InstancesPage.tsx:58-60, 200-245` (`DeleteInstanceModal` with `wipeDirectory` checkbox).
       - In `src/pages/HomePage.tsx:120-124`, `onDelete` still calls `window.confirm(t('instances.confirmDelete', { name }))` and hardcodes `api.instanceDelete(id, false)`. The modal is bypassed, and user has no option to wipe directory.
       - *Verdict:* **ЧАСТИЧНО ПОДТВЕРЖДЕНО (PARTIALLY CONFIRMED)** -> Logged as **FINDING-C1-02 (P2)**.
   - **DECISIONS.md D30 («src/i18n/{ru,en,uk}.json, 226 ключей в каждом, полный паритет»):**
     - Observation: Key counting script verified: `ru.json` has 226 keys, `en.json` has 226 keys, but `uk.json` has **225 keys**.
     - Missing key in `src/i18n/uk.json`: `"accounts.title": "Керування акаунтами"`.
     - *Verdict:* **ЧАСТИЧНО ПОДТВЕРЖДЕНО (PARTIALLY CONFIRMED)** -> Logged as **FINDING-C1-07 (P3)**.
   - **AUDIT.md §6 D13 (apiErrorText fallback):** Confirmed as partial as documented (8 catch blocks use `apiErrorText`, remaining use raw `err`).
   - **AUDIT.md §6 D14 (Heavy IO in spawn_blocking):** Confirmed as partial as documented (major launch/archive/read operations wrapped, minor directory sync remains synchronous).
   - **AUDIT.md §6 D18 (dl_progress owner):** Confirmed as partial as documented (progress guarded by download state, but payload lacks `instanceId`).

3. **Confirmed Claims (Подтверждено):**
   - 43 items confirmed with exact `file:line` proof in `report.md` (e.g. rate limiters in `modrinth.rs:16` and `curseforge.rs:12`; path traversal checks in `validate_instance_id`, `fs_extra.rs:18-36`, `zip.rs:59-78`; SHA-1 verification in `downloader.rs:136-166`; lockfile cleanup in `launch.rs:252-257`, `295-298`; log masking in `redact.rs:6-33`; `open_in_browser` security filter in `commands/mod.rs:649-659`; etc.).

---

### 1.3 Direction 11: Documentation & Release Metadata Observations
1. **License Conflict:**
   - Root `LICENSE`: MIT License (Copyright (c) 2026 mc-launcher-v2 contributors).
   - `src-tauri/Cargo.toml:6`: `license = "MIT"`.
   - `package.json:11`: `"license": "MIT"`.
   - `docs/README.md:69`: `## Лицензия\n\nGPL-3.0 (как у Prism Launcher).`
   - `docs/PROGRESS.md:14`: `GPL-3.0`
   - *Verdict:* Direct contradiction between root MIT license and docs GPL-3.0 -> **FINDING-C1-03 (P1)**.

2. **UI-CONTRACT Out of Sync:**
   - `docs/UI-CONTRACT.md` documents only 24 Tauri commands.
   - `src-tauri/src/lib.rs:77-124` registers **46 Tauri commands**.
   - `src/api/client.ts` implements **46 frontend wrapper functions**.
   - Missing 22 commands in contract: `instance_kill`, `instance_backup`, `instance_backups_list`, `instance_backup_restore`, `instance_export`, `instance_export_mrpack`, `instance_icon_set`, `instance_icon_clear`, `account_add_msa_browser`, `news_feed`, `system_diagnose`, `launch_guide_ram`, `open_folder`, `open_file`, `app_version`, `crash_history`, `game_versions_cached`, `assets_verify`, `clean_orphaned_assets`, `runtime_check`, etc.
   - Missing types: `uiScale` in `Settings`, `url` in `NewsItem`, and entire data models (`Account`, `ArchiveResult`, `Diagnosis`, `RamGuide`, `ServerPingResult`).
   - *Verdict:* Stale API documentation -> **FINDING-C1-04 (P1)**.

3. **Installer Metadata Deficiencies:**
   - `src-tauri/tauri.conf.json:5-30`:
     - Missing `publisher` string.
     - Missing `copyright` string.
     - `tauri.bundle.identifier`: `"dev.mclauncher.v2"` (development placeholder).
     - Missing `bundle.windows.nsis` and `bundle.windows.wix` configuration blocks (custom install directory, desktop shortcuts, start menu, uninstall cleanup).
   - *Verdict:* Incomplete production installer specification -> **FINDING-C1-05 (P2)**.

4. **Version Synchronization:**
   - `src-tauri/tauri.conf.json:5`: `"version": "0.1.0"`
   - `src-tauri/Cargo.toml:3`: `version = "0.1.0"`
   - `package.json:4`: `"version": "0.1.0"`
   - `package-lock.json:3, 9`: `"version": "0.1.0"`
   - *Verdict:* **100% SYNCHRONIZED** at version `0.1.0`.

5. **Stale Documentation Counters & Progress:**
   - `docs/PROGRESS.md` status table halts at Milestone 9 (2026-09-26), misses D26-D30, and lists 90 tests.
   - `README.md:86` lists `96 unit-тестов` (actual: 112 passed).
   - `README.md:97` claims `i18n (ru + en)`, omitting `uk`.
   - `docs/README.md:65` claims `21 архитектурных решений` (actual: 30).
   - *Verdict:* Minor doc drifts -> **FINDING-C1-06 (P2)**, **FINDING-C1-08 (P3)**.

---

## 2. Logic Chain

1. **Premise 1:** The team relied on AUDIT.md §6 D15 and DECISIONS.md D29 assuming the OAuth callback HTTP server stops listening if a user cancels the Microsoft login dialog.
2. **Observation Reference:** In `commands/mod.rs:602-619` and `auth/msa.rs:322-362`, the underlying thread executes `listener.incoming()` in a blocking loop without a shutdown handle. The `tokio::time::timeout` only abandons the receiver channel.
3. **Inference 1:** The OS TCP socket on port 58428 remains open and the thread remains blocked until either an external HTTP request hits the port or the entire app terminates. A subsequent OAuth flow within the timeout window may encounter port collision or unpredictable state.
4. **Premise 2:** AUDIT.md §1 п. 14 and §6 D7 declared that instance deletion prompts were unified into a modal with a directory wipe option to prevent accidental data loss.
5. **Observation Reference:** While `InstancesPage.tsx:200-245` implements `DeleteInstanceModal`, `HomePage.tsx:120-124` bypassed this modal and kept `window.confirm()` with hardcoded `wipe: false`.
6. **Inference 2:** A user deleting an instance from the Home screen cannot wipe orphaned files from disk, creating an inconsistent UX and accumulating unmanaged disk clutter.
7. **Premise 3:** `docs/README.md` and `docs/PROGRESS.md` state that the project is licensed under GPL-3.0, while the root repository contains an MIT `LICENSE` file and all crate/package manifests specify MIT.
8. **Inference 3:** Distributing or publishing the project under contradictory licenses exposes the project to legal ambiguity and potential licensing non-compliance.
9. **Premise 4:** `docs/UI-CONTRACT.md` serves as the authoritative interface contract between frontend and backend.
10. **Inference 4:** Documenting only 24 out of 46 commands (47.8% undocumented) prevents frontend and backend teams from having an accurate specification for error handling, return types, and command behaviors.

---

## 3. Caveats

1. **Read-Only Constraint:** Per instructions, no production source files in `src/` or `src-tauri/src/` were modified.
2. **No Real Game Launches:** Minecraft launch was verified through unit/integration tests and static code inspection of command arguments, environment variables, classpath construction, and process isolation. No actual game client process or Mojang/Microsoft network connection was initiated.
3. **Ignored Integration Tests:** 5 tests in `src-tauri` (`test_diagnose_msa_error_flow`, `test_curseforge_manifest_parsing_from_network`, `test_modrinth_manifest_parsing_from_network`, `test_msa_full_device_code_flow_simulated`, `test_launch_offline_pipeline`) are marked `#[ignore]` in code because they require external network mocks or specific local directory setups.
4. **Operating System:** Testing and clippy verification were conducted under Windows 11 x64; Linux/macOS specific behavior was analyzed via static code inspection.

---

## 4. Conclusion

- **R1 Audit Status:** The previous remediation round (R1) achieved a very high quality bar: 43 out of 48 items are fully confirmed and verified in code. However, 1 claim is completely refuted (FINDING-C1-01: OAuth listener leak on cancel) and 1 UI safety claim is partially implemented (FINDING-C1-02: `HomePage.tsx` bypasses wipe confirmation modal).
- **Direction 11 Status:** Significant documentation drift was uncovered:
  - Critical P1 License collision (MIT vs GPL-3.0) must be resolved immediately.
  - Critical P1 UI-CONTRACT deficit (22 missing commands and missing types) requires an update to match the 46 active Tauri commands.
  - Release packaging requires NSIS/MSI installer customization and real publisher metadata.
  - Version synchronization across package files is flawless (`0.1.0`).

### Summary of New Findings Produced
| ID | Priority | Description | Target File(s) |
|---|---|---|---|
| **FINDING-C1-01** | P2 | OAuth TcpListener loop thread leak on login cancellation | `src-tauri/src/commands/mod.rs:602`, `src-tauri/src/auth/msa.rs:322` |
| **FINDING-C1-02** | P2 | Incomplete deletion modal migration (`HomePage.tsx` uses window.confirm) | `src/pages/HomePage.tsx:120-124` |
| **FINDING-C1-03** | P1 | License collision: docs claim GPL-3.0, manifests & root claim MIT | `docs/README.md:69`, `docs/PROGRESS.md:14` vs `LICENSE`, `Cargo.toml:6` |
| **FINDING-C1-04** | P1 | UI-CONTRACT missing 22 Tauri commands (46 in code vs 24 in docs) | `docs/UI-CONTRACT.md:20-60` vs `src-tauri/src/lib.rs:77-124` |
| **FINDING-C1-05** | P2 | Production installer metadata missing (publisher, copyright, NSIS bundle) | `src-tauri/tauri.conf.json:5-30` |
| **FINDING-C1-06** | P2 | `docs/PROGRESS.md` stale (frozen at M9, outdated test counts, missing D26-D30) | `docs/PROGRESS.md:1-120` |
| **FINDING-C1-07** | P3 | Ukrainian i18n missing `"accounts.title"` (225 vs 226 keys) | `src/i18n/uk.json:1-230` |
| **FINDING-C1-08** | P3 | Outdated documentation counters (tests: 96 vs 112, decisions: 21 vs 30) | `README.md:86,97`, `docs/README.md:65` |

---

## 5. Verification Method

To independently reproduce and verify every finding in this report:

1. **Verify Test Suite & Clippy:**
   ```powershell
   cd c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\src-tauri
   cargo test
   cargo clippy --all-targets --all-features -- -D warnings
   ```
   *Expected:* 112 passed tests (102 lib, 10 engine), 0 clippy warnings.

2. **Verify Frontend Build & Typecheck:**
   ```powershell
   cd c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2
   npm run build
   ```
   *Expected:* Clean compilation, zero TypeScript errors.

3. **Verify License Discrepancy (FINDING-C1-03):**
   ```powershell
   Select-String -Path "docs/README.md", "docs/PROGRESS.md" -Pattern "GPL-3.0"
   Get-Content "LICENSE" -TotalCount 3
   Select-String -Path "src-tauri/Cargo.toml", "package.json" -Pattern "license"
   ```
   *Expected:* `docs/README.md` and `docs/PROGRESS.md` match `GPL-3.0`; `LICENSE`, `Cargo.toml`, and `package.json` match `MIT`.

4. **Verify Command Count Disparity (FINDING-C1-04):**
   ```powershell
   # Count registered commands in Tauri lib.rs:
   (Select-String -Path "src-tauri/src/lib.rs" -Pattern "commands::[a-z0-9_]+").Matches.Count
   # Count documented commands in UI-CONTRACT.md:
   (Select-String -Path "docs/UI-CONTRACT.md" -Pattern "^\| `[a-z0-9_]+`").Matches.Count
   ```
   *Expected:* Code yields 46 commands; `docs/UI-CONTRACT.md` yields 24 commands.

5. **Verify i18n Key Discrepancy (FINDING-C1-07):**
   ```powershell
   node -e 'const ru = Object.keys(require("./src/i18n/ru.json")); const uk = Object.keys(require("./src/i18n/uk.json")); console.log("ru count:", ru.length, "uk count:", uk.length, "missing in uk:", ru.filter(k => !uk.includes(k)));'
   ```
   *Expected:* `ru count: 226 uk count: 225 missing in uk: [ 'accounts.title' ]`.

6. **Verify OAuth Listener Thread Behavior (FINDING-C1-01):**
   Inspect `src-tauri/src/auth/msa.rs:322-362` and `src-tauri/src/commands/mod.rs:602-619`. Notice that `listener.incoming()` loop is synchronous and lacks any mechanism to break or close the listener on timeout.
