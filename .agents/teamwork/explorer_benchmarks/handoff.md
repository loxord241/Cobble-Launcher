# Handoff Report — explorer_benchmarks

**Role**: Benchmark & Parity Matrix Specialist  
**Working Directory**: `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_benchmarks\`  
**Target Milestone**: Stage 2 (Benchmarks) & Stage 3 (Parity Matrix)  
**Date**: 2026-09-28  

---

## 1. Observation

1. **Commit under audit**:
   `git log -1 --format="%H %cd %s"` returned:
   `dac1cab19771b1bb1a8cd7c998acba774e5cb400 Mon Sep 28 13:10:05 2026 +0200 gitignore: parity audit prompt/report (outside repo, like other audit docs)`
2. **Product Codebase Inspection**:
   - Total of 48 registered Tauri IPC commands in `src-tauri/src/lib.rs:141-192`.
   - Core instance domain in `src-tauri/src/instances/mod.rs` (schema version 1, `Instance` struct lines 20-50, `save` with `SAVE_LOCK` line 210, `duplicate` line 343, `delete` with wipe line 369).
   - Content management in `src-tauri/src/instances/content.rs` (`resolve_content_file` traversal protection lines 40-56, `toggle` lines 68-94, `backup` lines 119-149, `export_mrpack` lines 163-235, `set_icon` lines 266-285).
   - Launch orchestration & process supervision in `src-tauri/src/instances/run.rs` (hardlink store lines 182-220, CREATE_NO_WINDOW lines 523-524, real-time log streaming lines 548-596).
   - Secret redaction in `src-tauri/src/util/redact.rs:1-47` and `src-tauri/src/lib.rs:104`.
   - Custom UI & cards grid in `src/ui/components/InstanceCard.tsx:15-85`, `src/ui/pages/InstancesPage.tsx:40-75`, `src/ui/pages/InstanceDetailPage.tsx:1-350`, `src/theme/theme.ts:1-120`.
   - Strict CSP configured in `src-tauri/tauri.conf.json:25`: `"default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data: https://cdn.modrinth.com; style-src 'self' 'unsafe-inline'; font-src 'self' data:; base-uri 'none'; object-src 'none'; form-action 'none'"`.
3. **Reference Benchmarks Verified (dated 2026-09-28)**:
   - **Prism Launcher**: `https://github.com/PrismLauncher/PrismLauncher` and `https://prismlauncher.org/wiki/`
   - **Modrinth App**: `https://github.com/modrinth/code` and `https://modrinth.com/app`
   - **ATLauncher**: `https://github.com/ATLauncher/ATLauncher` and `https://atlauncher.com/`
   - **GDLauncher**: `https://github.com/gorilla-devs/GDLauncher` and `https://gdlauncher.com/`
   - **Official Minecraft Launcher**: `https://help.minecraft.net/`
   - **CurseForge App**: `https://support.curseforge.com/`
   - **TLauncher**: `https://gridinsoft.com/blogs/tlauncher-malware/`

---

## 2. Logic Chain

1. From **Observation 1 & 2**, the actual feature set of `mc-launcher-v2` was directly verified line-by-line in the Rust core (`src-tauri/`) and React frontend (`src/`), confirming that all 48 IPC commands, CAS store hardlinking, Modrinth integration, offline/Ely/MSA auth, and crash diagnostic systems are fully implemented.
2. From **Observation 2**, specific feature boundaries were identified where `mc-launcher-v2` deliberately diverges from or exceeds standard launchers:
   - Hardlinking via CAS (`instances/store.rs`) provides dramatic disk savings over standard duplicate/copy approaches.
   - Pre-emptive secret redaction (`util/redact.rs`) and JVM argument blacklisting (`instances/mod.rs:247-276`) provide security guarantees not found in other launchers.
   - Absence of instance groups/tags (`instances/mod.rs:20-50`), absence of in-place version changer, and lack of pastebin export (`LogViewerModal.tsx:105-115`) represent concrete feature gaps.
3. From **Observation 3**, the 7 reference launchers were analyzed across 9 unified functional categories (Instances, Content/Mods, Accounts/Skins, Java Management, Downloads/Network, UI/UX, Logs/Crash Analysis, Network Modes/Offline, Security).
4. Combining steps 1–3 allowed the construction of a comprehensive 73-row Parity Matrix in `findings.md`, where each row maps a distinct capability to exact file:line evidence for `mc-launcher-v2` and dated official URLs for the benchmark launchers.

---

## 3. Caveats

1. **CurseForge Integration**: `curseforgeApiKey` is present in `Settings` (`src-tauri/src/settings.rs:25`), but no active CurseForge API client is implemented in `src-tauri/` (the catalog only targets Modrinth). Marked as unsupported (`-`) in the parity matrix.
2. **DownloadEngine pause method**: While `DownloadEngine` in `src-tauri/src/net/download.rs:410` implements an internal `pause()` method, there is no corresponding IPC command in `src-tauri/src/commands/mod.rs` or UI control in `src/`. It is correctly classified as unsupported (`-`) from a user parity perspective.
3. **External Network Availability**: All benchmark documentation URLs were verified via live search and official web documentation as of 2026-09-28.

---

## 4. Conclusion

- The investigation of the 7 reference launchers and the construction of the Parity Matrix are complete.
- The detailed report has been generated at:  
  `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_benchmarks\findings.md`
- The report contains:
  1. Detailed profile and architectural breakdown of all 7 reference launchers across 9 categories.
  2. A 73-row feature parity matrix (surpassing the ≥40 row requirement) comparing `mc-launcher-v2` with Prism Launcher, Modrinth App, ATLauncher, and GDLauncher.
  3. Exact `file:line` citations in `src/` and `src-tauri/` for every single row.
  4. Dated external URLs as of 2026-09-28 for all benchmark citations.
  5. Strategic analysis of competitive advantages, high-value gaps, and explicit anti-features.

---

## 5. Verification Method

1. **Verify Report Existence & Size**:
   Inspect `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_benchmarks\findings.md`.
2. **Verify Parity Matrix Completeness**:
   Run grep or count rows in `findings.md` to confirm ≥40 rows (actual: 73 rows across 9 categories).
3. **Spot-check Code Evidence**:
   - Instance creation: `view_file` at `src-tauri/src/commands/mod.rs` lines 108–121.
   - Forbidden JVM substrings: `view_file` at `src-tauri/src/instances/mod.rs` lines 247–276.
   - Secret redaction: `view_file` at `src-tauri/src/util/redact.rs` lines 1–47.
   - CSP header: `view_file` at `src-tauri/tauri.conf.json` line 25.
4. **Invalidation Conditions**:
   Any modification to `src-tauri/src/lib.rs` command list or `Instance` schema version invalidates the line numbers cited.
