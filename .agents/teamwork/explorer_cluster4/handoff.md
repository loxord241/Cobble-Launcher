# Handoff Report — Explorer Cluster 4: Directions 7 & 8

**Date**: 2026-09-27  
**Mission**: Direction 7 (Security & Isolation) and Direction 8 (Test Coverage & Reliability)  
**Detailed Report**: `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster4\report.md`  

---

## 1. Observation

Direct code observations from static analysis, pattern grep, and test execution:

1. **RCE via IPC JVM Flags**:
   - `src-tauri/src/commands/mod.rs:155-161`: `instance_settings_set` accepts `Instance` directly from IPC and saves it via `instances::save`.
   - `src-tauri/src/instances/run.rs:260-262`: `jvm_extra_flags.extend(inst.jvm_flags.iter().cloned());`
   - `src-tauri/src/mojang/launch.rs:164-180`: `values.jvm_extra` are appended to the process command arguments without any whitelist or validation.
2. **Arbitrary Directory Deletion via Traversal in `instance_delete`**:
   - `src-tauri/src/commands/mod.rs:133-136`: `instance_delete(state, id, wipe)` invokes `instances::delete(&state.paths, &id, wipe)`.
   - `src-tauri/src/instances/mod.rs:291-314`: Unlike `load` (line 193) and `save` (line 203), `delete` **does not call `valid_id(id)`**.
   - `src-tauri/src/instances/mod.rs:309`: If `wipe` is true, calls `std::fs::remove_dir_all(long_path(&dir))`. For `id: "."` or `id: ".."`, this targets the parent `instances/` directory or root launcher data directory.
3. **Token Leakage Outside `launcher.log`**:
   - `src-tauri/src/lib.rs:23-56, 67`: `RedactingWriter` is configured exclusively for `tracing_subscriber` writing to `launcher.log`.
   - `src-tauri/src/instances/run.rs:476, 686`: Game logs are written to `latest.log` via `writeln!(log_file, "{line}")` without calling `util::redact::redact()`.
   - `src-tauri/src/instances/run.rs:477-481, 688-692`: `LauncherEvent::GameLogLine` emits the unredacted `line` string directly to the event bus and WebView.
   - `src-tauri/src/auth/msa.rs:276-280, 405-408`: Raw HTTP error response bodies are injected into `LauncherError::Network`, exposing them to WebView.
4. **Local File Read Oracle in `instance_icon_set`**:
   - `src-tauri/src/commands/mod.rs:373-384`: Accepts `path: String` directly from IPC.
   - `src-tauri/src/instances/content.rs:240-258`: Reads arbitrary files up to 300 KB (`std::fs::read(p)`), checks magic bytes (`sniff_image`), and base64 encodes them into `instance.json`.
5. **SSRF and Symlink Inconsistencies in Drag & Drop**:
   - `src-tauri/src/modrinth/mrpack.rs:148-168`: Downloads files specified in `modrinth.index.json` (`f.downloads[0]`) without validating host domain.
   - `src-tauri/src/util/zip.rs:140-146`: `extract_zip_stripping` does not skip symlinks, unlike `extract_zip_filtered` (lines 75-78).
6. **CSP Configuration Omissions**:
   - `src-tauri/tauri.conf.json:24-26`: Production CSP specifies `default-src`, `connect-src`, `img-src`, `style-src`, `font-src`, but lacks `base-uri`, `object-src`, and `form-action`.
7. **Test Coverage & Flakiness**:
   - `cargo test`: 102 passed, 0 failed, 5 ignored in library; 10 passed in `download_engine`.
   - Core launch command `instance_launch` has **0 tests** in Rust test suites.
   - `src-tauri/tests/download_engine.rs:287, 304` and `src-tauri/src/modrinth/mrpack.rs:465` use fixed `tokio::time::sleep(400ms / 500ms)`.
   - `src-tauri/src/util/redact.rs:8-18`: `registry()` uses a static `OnceLock<Mutex<HashSet<String>>>` that retains registered secrets across parallel test runs.
   - `src-tauri/src/auth/mod.rs:226-235`: `keyring_roundtrip` is marked `#[ignore]` and mutates the OS Windows Credential Manager under fixed service `"mc-launcher-v2"`.

---

## 2. Logic Chain

1. **WebView Compromise to RCE**:
   - WebView can be compromised via XSS or dependency injection.
   - A compromised WebView can call `invoke('instance_settings_set', { instance: { ...inst, jvmFlags: ['-XX:OnError=calc.exe'] } })`.
   - The backend persists this without validation in `instance.json`.
   - Calling `invoke('instance_launch', { id: inst.id, player: 'attacker' })` passes these flags straight to `Command::new(java)`.
   - Result: Arbitrary host code execution outside WebView sandbox.
2. **Missing Validation to Data Loss**:
   - `instances::delete` does not call `valid_id(id)`.
   - Supplying `id: ".."` results in `paths.instances_dir().join("..")` which is `paths.root()`.
   - Setting `wipe: true` triggers `std::fs::remove_dir_all`, which deletes all user accounts, settings, and instances.
3. **Redaction Filter Bypass to Token Exposure**:
   - Redaction is strictly implemented in `RedactingWriter` on the tracing subscriber.
   - Game process stdout/stderr lines are written to `latest.log` and dispatched as `GameLogLine` events bypassing tracing.
   - If JVM flags or tokens are printed in process output or crash dumps, tokens are stored in plaintext on disk and sent to WebView.
4. **Timing Sleep to CI Flakiness**:
   - Tests assert that events occur during an artificial `sleep` window (e.g. 500ms in `mrpack.rs:465`).
   - On heavily loaded CI nodes or single-core VMs, background execution can take longer, resulting in 0 events received and intermittent test failures.

---

## 3. Caveats

- In dev mode (`npm run tauri dev`), Vite serves the frontend without production CSP constraints; findings on CSP strictly apply to the production release configuration (`src-tauri/tauri.conf.json`).
- `instance_delete` with `wipe: false` relies on PowerShell `Microsoft.VisualBasic.FileIO.FileSystem::DeleteDirectory` on Windows. String escaping prevents command execution if the path does not exist on disk, but still represents bad architectural practice.
- The `keyring` crate uses the Windows Credential Manager under the executing Windows user account. Tests run with `-- --ignored` require interactive user session permissions on Windows.

---

## 4. Conclusion

- **Direction 7 (Security & Isolation)**: **REQUIRES WORK (6 findings: 2 P0, 3 P1, 1 P2)**. The application has strong baseline defenses (strict `connect-src` CSP, zip-slip protection, Windows Credential Manager integration), but exposes severe IPC vulnerabilities: RCE through unvalidated JVM flags (`SEC-1`), arbitrary directory deletion through unvalidated `instance_delete` IDs (`SEC-2`), local file disclosure oracle in `instance_icon_set` (`SEC-4`), and token leakage in game logs / IPC events (`SEC-3`).
- **Direction 8 (Test Coverage & Reliability)**: **REQUIRES WORK (4 findings: 1 P1, 3 P2)**. While static assertions on fixtures pass (102 tests), critical runtime paths (`instance_launch`, OAuth browser exchange, Ely.by login, full mod updates) have 0 tests. Several tests are flaky due to hardcoded `sleep` timers, and parallel test execution suffers from shared mutable state in `util::redact::registry`.

---

## 5. Verification Method

To independently verify the observations:

1. **Verify Unit Tests & Ignored Tests**:
   ```powershell
   cd c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\src-tauri
   cargo test
   # Notice: 102 passed, 5 ignored (keyring and network tests ignored)
   ```
2. **Inspect Missing ID Validation in `instance_delete`**:
   - View `src-tauri/src/instances/mod.rs:291-295`. Compare with lines 193 and 203 where `valid_id(id)?` is called.
3. **Inspect JVM Flags Pass-through in `instance_launch`**:
   - View `src-tauri/src/instances/run.rs:260-262` and `src-tauri/src/mojang/launch.rs:164-180`. Note absence of flag filtering or sanitization.
4. **Inspect Unredacted Game Logs & Events**:
   - View `src-tauri/src/instances/run.rs:476-481` and lines `685-692`. Note direct writes and emissions without `util::redact::redact`.
5. **Inspect Flaky Sleeps in Tests**:
   - View `src-tauri/tests/download_engine.rs:287, 304` and `src-tauri/src/modrinth/mrpack.rs:465`.
