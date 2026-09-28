# Comprehensive Codebase Inventory & Extension Points Map
**Project:** mc-launcher-v2 (Cobble Launcher)  
**Revision:** `dac1cab19771b1bb1a8cd7c998acba774e5cb400`  
**Date:** 2026-09-28  
**Author:** Codebase Inventory Specialist (`explorer_inventory`)  
**Scope:** Verification of "as-is" features against `docs/DECISIONS.md` (D01–D35), `docs/UI-CONTRACT.md`, and `AUDIT.md` (§6, §7); two-way discrepancy analysis; master map of extension points across 11 functional domains.

---

## 1. Executive Summary

This inventory provides a ground-truth technical audit of the `mc-launcher-v2` codebase across the Rust backend (`src-tauri/src/`) and React 19/TypeScript frontend (`src/`).

### Key Quantitative Discoveries:
1. **IPC Commands Count:** `src-tauri/src/lib.rs:141-192` registers **50 commands** via `generate_handler!`, all 50 are bound in `src/api/client.ts`. However, `docs/UI-CONTRACT.md:20` claims "47 команд" and omits 3 critical commands (`modrinth_projects`, `content_backfill_projects`, `update_check`).
2. **Events Count:** `src-tauri/src/events.rs:52-70` defines 5 events (`dl_progress`, `dl_queue_state`, `launch_state`, `game_log_line`, `account_refresh_failed`). However, `dl_queue_state` is **never emitted** by any backend code (dead event).
3. **Orphaned / Unreachable Features in UI:**
   - `instance_import` (`src-tauri/src/commands/mod.rs:801`, `src/api/client.ts:49`): implemented in core and bound in API client, but **has zero UI invocation points** (no button/dialog/menu).
   - `account_add_msa_start` / `account_add_msa_poll` (`src-tauri/src/commands/mod.rs:697, 717`, `src/api/client.ts:125, 133`): dead legacy OAuth device-code flow, not exposed in `AccountsModal.tsx`.
   - `Forge` loader: `loaders/forge.rs` exists, but `commands/mod.rs:879-904` (`loader_versions`) omits `"forge"` from its match arms (returns `invalid_input`), and `CreateInstanceDialog.tsx:9-14` excludes Forge from `LOADERS`. Forge cannot be queried or created via the UI.
4. **Decisions D01–D35:** All 35 architectural decisions have been verified against actual code implementations.

---

## 2. Functional Groups Verification (§1.2 Ground Truth)

Every feature group listed in §1.2 was verified against physical file locations and line numbers:

| §1.2 Functional Area | Feature & Scope | Backend Implementation (File:Line) | Frontend Implementation (File:Line) | Status / Verdict |
|---|---|---|---|---|
| **1. Instances** | Instance Creation | `commands/mod.rs:108-121`<br>`instances/mod.rs:146-170`<br>`util/names.rs:23-55`<br>`mojang/manifest.rs:74-120` | `CreateInstanceDialog.tsx:1-174`<br>`api/client.ts:36-37` | **Confirmed** (sanitizes name, checks manifest) |
| | Loaders: Fabric | `loaders/fabric.rs:1-110`<br>`instances/run.rs:658` | `CreateInstanceDialog.tsx:11` | **Confirmed** |
| | Loaders: Quilt | `loaders/quilt.rs:1-75`<br>`instances/run.rs:661` | `CreateInstanceDialog.tsx:12` | **Confirmed** |
| | Loaders: NeoForge | `loaders/neoforge.rs:1-250`<br>`instances/run.rs:677`<br>`mojang/launch.rs:90-115` | `CreateInstanceDialog.tsx:13` | **Confirmed** (headless installer, classpath dedup) |
| | Loaders: Forge | `loaders/forge.rs:1-160`<br>`instances/run.rs:664` | `CreateInstanceDialog.tsx:9-14` (**MISSING**) | **Partially Confirmed**: Backend supports install, but `loader_versions` rejects `"forge"` and UI dialog excludes it |
| | Duplication | `commands/mod.rs:137-143`<br>`instances/mod.rs:269-291`<br>`instances/store.rs:77-124` | `InstanceKebabMenu.tsx:80-87`<br>`api/client.ts:40` | **Confirmed** (hardlinks store assets, copies unique saves/mods) |
| | Renaming | `commands/mod.rs:124-135`<br>`instances/mod.rs:253-267` | `RenameInstanceModal.tsx:1-98`<br>`api/client.ts:38-39` | **Confirmed** (blocks if instance is running) |
| | Deletion | `commands/mod.rs:145-152`<br>`instances/mod.rs:293-345` | `DeleteInstanceDialog.tsx:1-75`<br>`api/client.ts:41-42` | **Confirmed** (`wipe: false` -> recycle bin via PowerShell `ps_quote`; `wipe: true` -> `remove_dir_all`) |
| | Settings (RAM, JVM flags, Java path, resolution) | `commands/mod.rs:165-191`<br>`commands/mod.rs:1040-1080` (`validate_instance_settings`) | `InstanceSettingsModal.tsx:1-250`<br>`api/client.ts:44-46` | **Confirmed** (RAM clamp 256..=65536, blacklist on `-javaagent`/`-agentlib`/`-XX:OnError=`, path check) |
| | RAM Optimization (`ram_guide`, `instance_optimize`) | `commands/mod.rs:552-569`<br>`optimize.rs:15-105` | `InstanceSettingsModal.tsx:110-130`<br>`api/client.ts:100, 104` | **Confirmed** (sysinfo byte/MB fix from D28, sodium/lithium/ferritecore 1-click install) |
| | Icons (custom PNG/JPEG/WebP <=300KB) | `commands/mod.rs:413-435`<br>`instances/content.rs:240-275` | `InstanceKebabMenu.tsx:120-140`<br>`pickFile.ts:1-25` | **Confirmed** (magic bytes sniffing, data-URL in `instance.json`) |
| | Notes | `instances/mod.rs:38` (`pub notes`) | `InstanceDetailPage.tsx:145-165`<br>`InstanceSettingsModal.tsx:195-210` | **Confirmed** |
| | Statistics (launches, playtime, last played) | `instances/mod.rs:34-37`<br>`instances/run.rs:702-720` | `InstanceDetailPage.tsx:170-195`<br>`InstanceSettingsModal.tsx:65-80` | **Confirmed** (persisted on game exit via `instances::update`) |
| | Name Search | N/A (client-side) | `Shell.tsx:75-100` (Ctrl+K)<br>`InstancesPage.tsx:73-75`<br>`HomePage.tsx:70-75` | **Confirmed** |
| | Drag & Drop .mrpack | N/A (Tauri window event) | `Shell.tsx:140-180` (`onDragDropEvent` -> `mrpackInstall`) | **Confirmed** |
| | Import Directory / Archive | `commands/mod.rs:801-830`<br>`import/mod.rs:1-450` | `api/client.ts:49`<br>UI Trigger: **NONE** | **Partially Confirmed**: Backend supports CF zip, MultiMC/Prism, Cobble zip; UI has no button to trigger |
| **2. CF-Style Cards (D35)** | Grid & 4:3 Art & Badges | N/A | `InstanceCard.tsx:1-125`<br>`InstancesPage.tsx:140-180` | **Confirmed** (`aspect-[4/3]`, version badge, hover Play overlay, running Stop button) |
| | Procedural Pixel Art Landscape | N/A | `defaultArt.ts:1-110` (`defaultInstanceArt`) | **Confirmed** (deterministic SVG data-URL, 6 color themes from hash) |
| | Instance Detail Page (Overview / Mods / Logs) | N/A | `InstanceDetailPage.tsx:1-800` | **Confirmed** (header hero, 3 tabs, state-reactive) |
| **3. Content (Modrinth)** | Catalog Search & Type Tabs | `commands/mod.rs:302-325`<br>`modrinth/api.rs:1-100` | `ModsPage.tsx:1-550` | **Confirmed** (mods, shaders, resourcepacks, modpacks tabs, search text) |
| | 1-Click Install & Dependencies | `commands/mod.rs:338-360`<br>`modrinth/install.rs:1-350` | `ModsPage.tsx:320-360`<br>`api/client.ts:77-78` | **Confirmed** (recursive dependency resolution, SHA1 hash check, content-manifest) |
| | Content Toggles (`.disabled`) | `commands/mod.rs:371-384`<br>`instances/content.rs:68-94` | `InstanceDetailPage.tsx:340-365`<br>`api/client.ts:81-82` | **Confirmed** (Prism pattern: renames `mods/x.jar` <-> `mods/x.jar.disabled`, toggles manifest `enabled`) |
| | Content Deletion | `commands/mod.rs:386-399`<br>`instances/content.rs:97-115` | `InstanceDetailPage.tsx:370-390`<br>`api/client.ts:83-84` | **Confirmed** (deletes both enabled/disabled filenames, cleans manifest) |
| | Update Check & Update All | `commands/mod.rs:448-457, 469-488`<br>`modrinth/updates.rs:1-500` | `InstanceDetailPage.tsx:280-320`<br>`api/client.ts:85-87` | **Confirmed** (batch `/v2/version_files/update`, old jars moved to `.trash/`) |
| | Batch Metadata (`/v2/projects?ids=`) | `commands/mod.rs:327-336`<br>`modrinth/api.rs:105-135` | `InstanceDetailPage.tsx:230-260`<br>`api/client.ts:76` | **Confirmed** (`modrinth_projects`, chunks of 100) |
| | Self-Healing Manifest Backfill | `commands/mod.rs:459-467`<br>`modrinth/updates.rs:80-140` | `InstanceDetailPage.tsx:220-228`<br>`api/client.ts:88-89` | **Confirmed** (`content_backfill_projects`, hashes -> projectId lookup) |
| | Modpacks (.mrpack & CDN) | `commands/mod.rs:490-550`<br>`modrinth/mrpack.rs:1-750` | `Shell.tsx:102`<br>`ModsPage.tsx:220` | **Confirmed** (progress tracking on `mrpack:<name>`, atomic rollback on error/cancel) |
| | Backup Zip (with saves) | `commands/mod.rs:401-411`<br>`instances/content.rs:119-164` | `InstanceKebabMenu.tsx:90-100`<br>`api/client.ts:95` | **Confirmed** (stores zip in `%LOCALAPPDATA%\mc-launcher-v2\backups\`) |
| | Export .mrpack (without saves) | `commands/mod.rs:437-446`<br>`instances/content.rs:166-238` | `InstanceKebabMenu.tsx:102-112`<br>`api/client.ts:96` | **Confirmed** (stores `.mrpack` in `exports\`, untracked files -> overrides) |
| **4. Accounts / Auth** | Offline Profile | `commands/mod.rs:623-640`<br>`auth/offline.rs:1-50` | `AccountsModal.tsx:80-120`<br>`Onboarding.tsx:90-110` | **Confirmed** (UUID from MD5, clearly labeled "нелицензионный") |
| | Microsoft Browser Auth-Code | `commands/mod.rs:642-695`<br>`auth/msa.rs:250-400` | `AccountsModal.tsx:125-155`<br>`api/client.ts:124` | **Confirmed** (localhost:52134 listener, 300s timeout, keyring storage) |
| | Microsoft Device-Code (Legacy) | `commands/mod.rs:697-748`<br>`auth/msa.rs:50-130` | `api/client.ts:125-134`<br>UI: **MISSING** | **Partially Confirmed**: Backend commands exist, but UI exclusively uses browser flow |
| | Ely.by + authlib-injector | `commands/mod.rs:750-770`<br>`auth/ely.rs:1-170`<br>`instances/run.rs:370-395` | `AccountsModal.tsx:160-190`<br>`api/client.ts:135-136` | **Confirmed** (authlib-injector downloaded with GitHub digest SHA256 check) |
| | Active Profile & Switching | `commands/mod.rs:618, 778`<br>`auth/mod.rs:50-120` | `Shell.tsx:50-70`<br>`AccountsModal.tsx:195-260` | **Confirmed** |
| | Keyring Token Storage | `auth/mod.rs:140-230` | N/A (backend only) | **Confirmed** (Windows Credential Manager via `keyring` crate) |
| **5. Skins (D34)** | Ely.by Skins | `auth/skins.rs:60-80` (`skinsystem.ely.by`) | `SkinViews.tsx:1-250` | **Confirmed** |
| | Mojang MSA Skins | `auth/skins.rs:85-150` (`sessionserver.mojang.com`) | `SkinViews.tsx:1-250` | **Confirmed** |
| | Offline Default Procedural Skin | `auth/skins.rs:50-58` (returns None)<br>UI: `SkinViews.tsx:110-180` | `SkinViews.tsx:110-180` | **Confirmed** (no external network leaks for offline usernames) |
| | Head Avatar & Body Canvas | N/A | `SkinViews.tsx:15-50` (`SkinHead`)<br>`SkinViews.tsx:55-105` (`SkinBody`) | **Confirmed** (8x8 face + hat overlay; 16x32 canvas body) |
| | Skin Cache & Validation | `auth/skins.rs:15-45, 175-195` | `state/skins.ts:1-60` | **Confirmed** (1-hour disk cache in `cache/skins/`, 8-byte PNG magic header, allowlist hosts) |
| **6. Java** | Detection | `commands/mod.rs:931-935`<br>`java/detect.rs:1-250` | `SettingsPage.tsx:200-280`<br>`InstanceSettingsModal.tsx:140-190` | **Confirmed** (PATH, JAVA_HOME, Program Files, ~/.jdks, launcher runtime, extra paths) |
| | Adoptium Auto-Install | `commands/mod.rs:937-948`<br>`java/adoptium.rs:1-200` | `SettingsPage.tsx:240-270` (Java 17 & 21) | **Confirmed** (validates SHA256 from Adoptium API, extracts to `runtime/jdk{major}`) |
| **7. Process & Logs** | Live Stream & Batching | `instances/run.rs:475-485, 685-695` | `state/instances.ts:130-160`<br>`LogViewerModal.tsx:1-160` | **Confirmed** (200ms batching, 500 lines buffer, token redaction in output) |
| | Crash Analysis | `commands/mod.rs:832-848`<br>`process/crash.rs:1-120` | `CrashReportModal.tsx:1-150` | **Confirmed** (rule-based matching for OOM, GPU driver, Java version, Fabric mods) |
| | Process Lifecycle & Kill | `commands/mod.rs:571-605`<br>`instances/run.rs:400-750` | `InstancesPage.tsx`, `HomePage.tsx`<br>`InstanceDetailPage.tsx` | **Confirmed** (CREATE_NO_WINDOW, `.lock` tracking, supervisor thread, process tree kill) |
| **8. Launcher Update** | Update Check | `commands/mod.rs:1141-1155` | `UpdatesSection.tsx:1-160`<br>`SettingsPage.tsx:320-350` | **Confirmed** (checks GitHub API releases/latest; returns `UpdateInfo` with `not_found` code for private repo) |
| | Auto-Updater Bundler | `tauri.conf.json:30-40` | N/A | **Confirmed** (configured in tauri.conf, awaiting pubkeys) |
| **9. UI / Platform** | Palettes ×5 | `tokens.css:1-300`<br>`main.tsx:1-30` | `ThemeCards.tsx:1-150` | **Confirmed** (`dark`, `light`, `cb-dark`, `cb-light`, `mc`) |
| | Customization (Font, Accent, Zoom) | `settings.rs:56-60` | `state/settings.ts:40-70`<br>`SettingsPage.tsx:140-195` | **Confirmed** (`uiScale` 80–150%, `uiFont`, `uiAccent` hex) |
| | i18n (en, ru, uk) | N/A | `i18n/index.ts:1-141`<br>`i18n/{en,ru,uk}.json` (226 keys)<br>`i18n/errors.json` (14 codes) | **Confirmed** (pluralization, fallback current -> en -> key) |
| | a11y & Focus Traps | N/A | `useModalA11y.ts:1-90` | **Confirmed** (applied across all 7 modals) |
| | CSP | `tauri.conf.json:24-26` | N/A | **Confirmed** (connect-src ipc:, img-src cdn.modrinth.com, object-src 'none') |
| | Native Dialogs | N/A | `pickFile.ts:1-25` (`@tauri-apps/plugin-dialog`) | **Confirmed** |

---

## 3. Two-Way Discrepancy Register

We performed a strict bidirectional comparison:
- **Direction A:** Documentation claims a feature/contract exists, but code omits or violates it.
- **Direction B:** Code implements a feature/contract, but documentation omits it.

### Direction A: Documentation Claims It Exists, But Code Doesn't

| ID | Documentation Source | Claimed in Documentation | Actual Code Status | Evidence (File:Line) | Impact / Severity |
|---|---|---|---|---|---|
| **DISC-A01** | `PARITY_AUDIT_PROMPT.md:65-66` | "загрузчики fabric/forge/neoforge/quilt" available for instance creation | `loader_versions` rejects `"forge"`. `CreateInstanceDialog` does NOT contain Forge in its `LOADERS` constant | `src-tauri/src/commands/mod.rs:879-904`<br>`src/ui/components/CreateInstanceDialog.tsx:9-14` | **P1 (Functional Gap)**: Users cannot select or query Forge versions when creating an instance via the UI. |
| **DISC-A02** | `PARITY_AUDIT_PROMPT.md:70`<br>`docs/UI-CONTRACT.md:58` | "Инстансы: ... импорт каталога"<br>`instance_import` command available | `instance_import` is compiled in backend and in `api/client.ts:49`, but **has no UI caller anywhere in the frontend**. | `src/ui/pages/InstancesPage.tsx`<br>`src/ui/pages/HomePage.tsx`<br>`src/api/client.ts:49` | **P2 (Orphaned API)**: Archive/directory import cannot be triggered from the interface. |
| **DISC-A03** | `docs/UI-CONTRACT.md:20` | "Все 47 команд invoke_handler (src-tauri/src/lib.rs:104)" | There are **50** commands registered at line 141 (not 104). 3 commands are missing from UI-CONTRACT.md table. | `src-tauri/src/lib.rs:141-192`<br>`docs/UI-CONTRACT.md:20` | **P2 (Documentation Stale)** |
| **DISC-A04** | `docs/UI-CONTRACT.md:133`<br>`src/api/types.ts:139-142` | `dl_queue_state` event emitted to UI | `LauncherEvent::DlQueueState` is **never emitted** anywhere in `src-tauri/src/` | `src-tauri/src/events.rs:54`<br>`src-tauri/src/commands/mod.rs:998` | **P2 (Dead Event Contract)**: Queue state monitoring in UI receives no events. |
| **DISC-A05** | `docs/UI-CONTRACT.md:138-139` | "spawn_event_bridge, src-tauri/src/commands/mod.rs:899" | `spawn_event_bridge` is located at line 976, not 899. | `src-tauri/src/commands/mod.rs:976`<br>`docs/UI-CONTRACT.md:139` | **P3 (Line Stale)** |
| **DISC-A06** | `src-tauri/src/commands/mod.rs:975` | Docstring: "Запустить пересылку событий ядра в окно (единый канал core)" | Emits to 5 distinct channels (`dl_progress`, `dl_queue_state`, `launch_state`, `game_log_line`, `account_refresh_failed`), not `core`. | `src-tauri/src/commands/mod.rs:995-1004` | **P3 (Internal Doc Drift)** |

### Direction B: Code Has It, But Documentation Omits It

| ID | Code Location (File:Line) | Feature in Code | Omission in Documentation | Impact / Severity |
|---|---|---|---|---|
| **DISC-B01** | `src-tauri/src/commands/mod.rs:327-336`<br>`src/api/client.ts:76` | `modrinth_projects(ids: Vec<String>) -> Result<Vec<ProjectMeta>>`: Batch project metadata fetch (icons/titles) | Completely absent from `docs/UI-CONTRACT.md` command table and types table. | **P2 (Missing Contract)** |
| **DISC-B02** | `src-tauri/src/commands/mod.rs:459-467`<br>`src/api/client.ts:88` | `content_backfill_projects(instance_id: String) -> Result<usize>`: Self-healing manifest backfill for old modpacks | Completely absent from `docs/UI-CONTRACT.md` command table. | **P2 (Missing Contract)** |
| **DISC-B03** | `src-tauri/src/commands/mod.rs:1141-1155`<br>`src/api/client.ts:117` | `update_check() -> Result<UpdateInfo>`: GitHub releases check for launcher updates | Completely absent from `docs/UI-CONTRACT.md` command table. | **P2 (Missing Contract)** |
| **DISC-B04** | `src-tauri/src/settings.rs:57-60`<br>`src/api/types.ts:66-70` | `ui_font: String` and `ui_accent: String` in `Settings` struct | `docs/UI-CONTRACT.md:99-105` lists all `Settings` fields but omits `uiFont` and `uiAccent`. | **P3 (Type Definition Lag)** |
| **DISC-B05** | `src/api/types.ts:176-181, 228-241` | `ProjectMeta` and `UpdateInfo` TypeScript interfaces | `docs/UI-CONTRACT.md` types section omits `ProjectMeta` and `UpdateInfo`. | **P3 (Type Definition Lag)** |
| **DISC-B06** | `src-tauri/src/auth/skins.rs:1-350`<br>`src/api/types.ts:17-20` | `SkinInfo` interface `{ source, dataUrl }` and `SkinSource` (`"ely" \| "mojang"`) | `docs/UI-CONTRACT.md:93-122` types section does not declare `SkinInfo` or `SkinSource` as standalone types (only mentions inline in command table). | **P3 (Minor Doc Lag)** |

---

## 4. Complete IPC Commands Master Inventory (All 50 Commands)

Below is the definitive verification table of all 50 commands registered in `src-tauri/src/lib.rs:141-192`:

| # | Command Name (`lib.rs`) | Rust Handler (File:Line) | Input Parameters | Return Type | UI-CONTRACT.md Status | `client.ts` Binding | Invoked in UI (`src/ui/`) | Notes & Audit Status |
|---|---|---|---|---|---|---|---|---|
| 1 | `instance_list` | `commands/mod.rs:98` | — | `Vec<Instance>` | Listed (p. 27) | `api.instanceList:35` | `state/instances.ts:107` | Async `spawn_blocking` (A20/D14) |
| 2 | `instance_create` | `commands/mod.rs:108` | `mc_version: String, name: String` | `Instance` | Listed (p. 28) | `api.instanceCreate:36` | `CreateInstanceDialog.tsx:66` | Sanitizes name (`names.rs`) |
| 3 | `instance_rename` | `commands/mod.rs:124` | `id: String, name: String` | `Instance` | Listed (p. 29) | `api.instanceRename:38` | `RenameInstanceModal.tsx:43` | Async `spawn_blocking` (A27) |
| 4 | `instance_duplicate` | `commands/mod.rs:137` | `id: String` | `Instance` | Listed (p. 30) | `api.instanceDuplicate:40` | `InstancesPage.tsx:87`, `HomePage.tsx` | Hardlinks shared content |
| 5 | `instance_delete` | `commands/mod.rs:145` | `id: String, wipe: bool` | `OkMsg` | Listed (p. 31) | `api.instanceDelete:41` | `DeleteInstanceDialog.tsx:38` | `valid_id` checked (A1/A23) |
| 6 | `instance_open_dir` | `commands/mod.rs:154` | `id: String` | `OkMsg` | Listed (p. 32) | `api.instanceOpenDir:43` | `InstanceKebabMenu.tsx:50` | `valid_id` checked |
| 7 | `instance_settings_get` | `commands/mod.rs:165` | `id: String` | `Instance` | Listed (p. 33) | `api.instanceSettingsGet:44` | `InstanceSettingsModal.tsx:48` | Reads fresh snapshot (D19) |
| 8 | `instance_settings_set` | `commands/mod.rs:173` | `instance: Instance` | `Instance` | Listed (p. 34) | `api.instanceSettingsSet:45` | `InstanceSettingsModal.tsx:90` | Validates JVM flags (A3) |
| 9 | `instance_launch` | `commands/mod.rs:193` | `id: String, player: String` | `OkMsg` | Listed (p. 35) | `api.instanceLaunch:47` | `state/instances.ts:38` | Concurrency lock + supervisor |
| 10 | `manifest_versions` | `commands/mod.rs:850` | `show_snapshots: bool, show_old: bool` | `Vec<VersionEntry>` | Listed (p. 59) | `api.manifestVersions:52` | `CreateInstanceDialog.tsx:40` | Stale cache fallback (D06/D28) |
| 11 | `loader_versions` | `commands/mod.rs:874` | `loader: String` | `Vec<LoaderVersionEntry>` | Listed (p. 60) | `api.loaderVersions:54` | `CreateInstanceDialog.tsx:54` | **Rejects "forge"** (DISC-A01) |
| 12 | `loader_install` | `commands/mod.rs:908` | `id: String, loader: String, loader_version: Option<String>` | `Instance` | Listed (p. 61) | `api.loaderInstall:56` | `CreateInstanceDialog.tsx:68` | Installs into instance |
| 13 | `modrinth_search` | `commands/mod.rs:302` | `query, mc_version?, loader?, project_type?, limit?, offset?` | `SearchResult` | Listed (p. 45) | `api.modrinthSearch:59` | `ModsPage.tsx:95` | Modrinth API search |
| 14 | `modrinth_projects` | `commands/mod.rs:327` | `ids: Vec<String>` | `Vec<ProjectMeta>` | **MISSING** (DISC-B01) | `api.modrinthProjects:76` | `InstanceDetailPage.tsx:238` | Added in D33 (batch metadata) |
| 15 | `content_install` | `commands/mod.rs:338` | `instance_id, project_id, version_id?` | `Vec<ContentEntry>` | Listed (p. 46) | `api.contentInstall:77` | `ModsPage.tsx:327` | Recursive dependencies |
| 16 | `content_installed` | `commands/mod.rs:362` | `instance_id: String` | `Vec<ContentEntry>` | Listed (p. 47) | `api.contentInstalled:79` | `InstanceDetailPage.tsx:206` | Reads `content-manifest.json` |
| 17 | `content_toggle` | `commands/mod.rs:371` | `instance_id: String, file: String` | `ContentEntry` | Listed (p. 48) | `api.contentToggle:81` | `InstanceDetailPage.tsx:348` | Renames `.jar` <-> `.disabled` |
| 18 | `content_remove` | `commands/mod.rs:386` | `instance_id: String, file: String` | `()` (null) | Listed (p. 49) | `api.contentRemove:83` | `InstanceDetailPage.tsx:378` | Deletes file & cleans manifest |
| 19 | `content_update_check` | `commands/mod.rs:448` | `instance_id: String` | `Vec<UpdateCheck>` | Listed (p. 50) | `api.contentUpdateCheck:85` | `InstanceDetailPage.tsx:288` | Batch check `/version_files` |
| 20 | `content_update_all` | `commands/mod.rs:469` | `instance_id: String` | `usize` | Listed (p. 51) | `api.contentUpdateAll:87` | `InstanceDetailPage.tsx:307` | Moves old jars to `.trash/` |
| 21 | `content_backfill_projects`| `commands/mod.rs:459` | `instance_id: String` | `usize` | **MISSING** (DISC-B02) | `api.contentBackfillProjects:88` | `InstanceDetailPage.tsx:223` | Self-healing manifest backfill |
| 22 | `mrpack_install` | `commands/mod.rs:490` | `path: String, name: Option<String>` | `Instance` | Listed (p. 52) | `api.mrpackInstall:90` | `Shell.tsx:102`, `ModsPage.tsx:220` | Unpacks `.mrpack`, rollback |
| 23 | `modpack_install` | `commands/mod.rs:526` | `project_id: String, name: Option<String>` | `Instance` | Listed (p. 53) | `api.modpackInstall:92` | `HomePage.tsx:135`, `ModsPage.tsx:327` | Downloads latest .mrpack |
| 24 | `instance_backup` | `commands/mod.rs:401` | `id: String` | `ArchiveResult` | Listed (p. 54) | `api.instanceBackup:95` | `InstanceKebabMenu.tsx:93` | Creates zip with saves |
| 25 | `instance_export` | `commands/mod.rs:437` | `id: String` | `ArchiveResult` | Listed (p. 55) | `api.instanceExport:96` | `InstanceKebabMenu.tsx:105` | Creates .mrpack without saves |
| 26 | `instance_icon_set` | `commands/mod.rs:413` | `id: String, path: String` | `Instance` | Listed (p. 56) | `api.instanceIconSet:97` | `InstanceKebabMenu.tsx:126` | Validates image magic bytes |
| 27 | `instance_icon_remove` | `commands/mod.rs:428` | `id: String` | `Instance` | Listed (p. 57) | `api.instanceIconRemove:98` | `InstanceKebabMenu.tsx:136` | Removes dataUrl icon |
| 28 | `instance_optimize` | `commands/mod.rs:552` | `instance_id: String` | `Vec<String>` | Listed (p. 38) | `api.instanceOptimize:100` | `InstancesPage.tsx:85`, `HomePage.tsx` | Sodium/Lithium/FerriteCore |
| 29 | `ram_guide` | `commands/mod.rs:566` | — | `RamGuide` | Listed (p. 39) | `api.ramGuide:104` | `InstanceSettingsModal.tsx:60` | Sysinfo memory recommendation |
| 30 | `instance_kill` | `commands/mod.rs:571` | `instance_id: String` | `OkMsg` | Listed (p. 36) | `api.instanceKill:102` | `InstancesPage.tsx:84`, `HomePage.tsx` | Kills process tree, clears lock |
| 31 | `process_status` | `commands/mod.rs:607` | — | `Vec<(String, u32)>` | Listed (p. 37) | `api.processStatus:103` | `state/instances.ts:110` | Restores running PID state |
| 32 | `account_list` | `commands/mod.rs:618` | — | `Vec<Account>` | Listed (p. 67) | `api.accountList:122` | `state/accounts.ts:25` | Reads `accounts.json` |
| 33 | `account_add_offline` | `commands/mod.rs:623` | `name: String` | `Account` | Listed (p. 68) | `api.accountAddOffline:123` | `AccountsModal.tsx:108`, `Onboarding.tsx` | Offline MD5 UUID |
| 34 | `account_add_msa_browser`| `commands/mod.rs:642` | — | `Account` | Listed (p. 71) | `api.accountAddMsaBrowser:124` | `AccountsModal.tsx:136` | Localhost OAuth server |
| 35 | `account_add_msa_start` | `commands/mod.rs:697` | — | `DeviceCodeStart` | Listed (p. 69) | `api.accountAddMsaStart:125` | **Unused in UI** | Legacy device-code start |
| 36 | `account_add_msa_poll` | `commands/mod.rs:717` | `device_code: String` | `Option<Account>` | Listed (p. 70) | `api.accountAddMsaPoll:133` | **Unused in UI** | Legacy device-code poll |
| 37 | `account_add_ely` | `commands/mod.rs:750` | `username: String, password: String` | `Account` | Listed (p. 72) | `api.accountAddEly:135` | `AccountsModal.tsx:173` | Authenticates against ely.by |
| 38 | `account_remove` | `commands/mod.rs:772` | `id: String` | `OkMsg` | Listed (p. 73) | `api.accountRemove:137` | `AccountsModal.tsx:234` | Cleans keyring token |
| 39 | `account_active_set` | `commands/mod.rs:778` | `id: String` | `OkMsg` | Listed (p. 74) | `api.accountActiveSet:138` | `AccountsModal.tsx:220` | Sets `accountsActiveId` |
| 40 | `account_skin` | `commands/mod.rs:786` | `account_id: String` | `Option<SkinInfo>` | Listed (p. 75) | `api.accountSkin:140` | `state/skins.ts:25` | Ely / Mojang / null (D34) |
| 41 | `instance_import` | `commands/mod.rs:801` | `path: String, name: Option<String>` | `Instance` | Listed (p. 58) | `api.instanceImport:49` | **Unused in UI** (DISC-A02) | CF zip / MultiMC / Cobble zip |
| 42 | `crash_analyze` | `commands/mod.rs:832` | `log_text: String` | `Vec<Diagnosis>` | Listed (p. 81) | `api.crashAnalyze:119` | `state/instances.ts:167` | Rule-based diagnosis |
| 43 | `java_list` | `commands/mod.rs:931` | — | `Vec<JavaInstall>` | Listed (p. 82) | `api.javaList:106` | `SettingsPage.tsx:210`, `InstanceSettingsModal.tsx` | Scans PATH & Program Files |
| 44 | `java_install` | `commands/mod.rs:937` | `major: u32` | `JavaInstall` | Listed (p. 83) | `api.javaInstall:107` | `SettingsPage.tsx:247` | Adoptium JRE auto-download |
| 45 | `settings_get` | `commands/mod.rs:950` | — | `Settings` | Listed (p. 84) | `api.settingsGet:109` | `state/settings.ts:22` | Reads `settings.json` |
| 46 | `settings_set` | `commands/mod.rs:955` | `settings: Settings` | `OkMsg` | Listed (p. 85) | `api.settingsSet:110` | `state/settings.ts:35` | Hot-swaps proxy HttpClient |
| 47 | `data_dir` | `commands/mod.rs:1008` | — | `String` | Listed (p. 86) | `api.dataDir:112` | `SettingsPage.tsx:75`, `Onboarding.tsx` | `%LOCALAPPDATA%\mc-launcher-v2` |
| 48 | `logs_tail` | `commands/mod.rs:1014` | `lines: u32` | `String` | Listed (p. 87) | `api.logsTail:113` | `InstanceDetailPage.tsx:520`, `LogViewerModal.tsx` | Reads `launcher.log` tail |
| 49 | `dir_open` | `commands/mod.rs:1026` | — | `OkMsg` | Listed (p. 88) | `api.dirOpen:114` | `SettingsPage.tsx:82` | Opens explorer on data_dir |
| 50 | `update_check` | `commands/mod.rs:1141` | — | `UpdateInfo` | **MISSING** (DISC-B03) | `api.updateCheck:117` | `UpdatesSection.tsx:35` | GitHub releases/latest |

---

## 5. Architectural Decisions D01–D35 Verification Matrix

| Decision | Title / Intent | Verification Location in Code | Status | Technical Evidence & Verification Note |
|---|---|---|---|---|
| **D01** | Rust toolchain installed for M0 | Environment / build scripts | **Confirmed** | Rust 1.98.1 stable x86_64-pc-windows-msvc configured |
| **D02** | Tauri CLI: npm + cargo | `package.json:25` | **Confirmed** | `@tauri-apps/cli ^2` in devDependencies |
| **D03** | README in docs/ + pointer in root | `README.md:1`, `docs/README.md:1` | **Confirmed** | Root README points to `docs/README.md` |
| **D04** | CLI binary `mcl` (M1 proof) | `src-tauri/src/bin/mcl.rs:1-120` | **Confirmed** | Standalone launcher CLI binary exists |
| **D05** | Mojang yearly versioning scheme | `src-tauri/src/java/detect.rs:165`<br>`src-tauri/src/mojang/manifest.rs` | **Confirmed** | Fallback Java mapping handles `major > 1 -> Java 21` |
| **D06** | Rule action `disallow` in old versions | `src-tauri/src/mojang/rules.rs:25` | **Confirmed** | `RuleAction::Deny` has `#[serde(alias = "disallow")]` |
| **D07** | Log4j config substitution `${path}` | `src-tauri/src/mojang/launch.rs:125` | **Confirmed** | Substitution map replaces `${path}` for `-Dlog4j.configurationFile=` |
| **D08** | Adoptium API singular `binary` | `src-tauri/src/java/adoptium.rs:32` | **Confirmed** | `struct AdoptiumEntry { binary: AdoptiumBinary }` models singular field |
| **D09** | Trash store subtrees directly | `src-tauri/src/instances/mod.rs:305-325` | **Confirmed** | Shared store folders removed directly; only unique saves/mods/config sent to OS recycle bin |
| **D10** | Launch downloads scoped to launch plan | `src-tauri/src/instances/run.rs:130` | **Confirmed** | Downloads queue scoped per launch; abandoned tasks don't block new runs |
| **D11** | NeoForge minimal `launcher_profiles.json` | `src-tauri/src/loaders/neoforge.rs:145` | **Confirmed** | Creates `{"profiles": {}, "settings": {}, "version": 3}` before headless installer |
| **D12** | `\\?\` prefix and `/` to `\` normalization | `src-tauri/src/util/fs.rs:20-35` | **Confirmed** | `long_path` converts slashes to backslashes before adding `\\?\` |
| **D13** | NeoForge classpath & argument fixes | `src-tauri/src/mojang/launch.rs:90-115`<br>`loaders/neoforge.rs:200` | **Confirmed** | Strips `@jar`, sets `-DlibraryDirectory`, adds `--mavenRoot` and `--mods` |
| **D14** | Remove CDP debug port for release | `src-tauri/tauri.conf.json:14-23` | **Confirmed** | `additionalBrowserArgs` removed from tauri.conf.json |
| **D15** | JSON case rules | `src-tauri/src/modrinth/api.rs`<br>`src-tauri/src/settings.rs:18` | **Confirmed** | Modrinth snake_case, settings/instances camelCase |
| **D16** | Prefer Fabric mod build for vanilla | `src-tauri/src/modrinth/install.rs:135` | **Confirmed** | Priority: fabric -> first compatible loader |
| **D17** | Optimization stack real project IDs | `src-tauri/src/optimize.rs:18-35` | **Confirmed** | Valid slugs/IDs for Sodium, Lithium, FerriteCore; non-compatible skipped |
| **D18** | Forge maven MC-prefix | `src-tauri/src/loaders/forge.rs:48` | **Confirmed** | Uses `{mc}-{version}` coordinate for forge installer |
| **D19** | Shared headless installer for Forge family | `src-tauri/src/loaders/mod.rs:15-40` | **Confirmed** | `run_headless_installer` shared between Forge and NeoForge |
| **D20** | Redaction secret registry | `src-tauri/src/util/redact.rs:1-120`<br>`src-tauri/src/lib.rs:80-108` | **Confirmed** | `RedactingWriter` masks registered access/refresh tokens in logs |
| **D21** | Ely.by authlib GitHub API digest | `src-tauri/src/instances/run.rs:380` | **Confirmed** | Compares `assets[].digest` SHA256 before executing authlib-injector |
| **D22** | Slate UI theme via `data-theme` | `src/theme/tokens.css:1-300`<br>`src/main.tsx:10-25` | **Confirmed** | `dark`, `light`, `cb-dark`, `cb-light`, `mc` via CSS custom properties |
| **D23** | Custom titlebar & permissions | `src-tauri/tauri.conf.json:21`<br>`src-tauri/capabilities/default.json` | **Confirmed** | `decorations: false`, window control permissions granted |
| **D24** | Offline system fonts | `src/theme/tokens.css:15-25` | **Confirmed** | "Segoe UI", "Cascadia Mono", no Google Fonts or external CDN fonts |
| **D25** | CamelCase serialization on both ends | `src-tauri/src/instances/mod.rs:18`<br>`src-tauri/src/events.rs:26,38,51` | **Confirmed** | `#[serde(rename_all = "camelCase")]` across all structs and events |
| **D26** | Content toggle, remove, backup, export | `src-tauri/src/instances/content.rs:68-238` | **Confirmed** | `content_toggle` (.disabled), `content_remove`, zip `backup` with saves, `.mrpack` `export` without saves |
| **D27** | Custom icons, uiScale, statistics | `src-tauri/src/instances/content.rs:240`<br>`src/state/settings.ts:50` | **Confirmed** | Data-URL icon <=300KB, `uiScale` 80-150% font-size zoom, stat chips |
| **D28** | Discovery fixes (engines, ram_guide, zip bomb, MSA refresh) | `src-tauri/src/commands/mod.rs:40-88`<br>`src-tauri/src/util/zip.rs:140-160` | **Confirmed** | `AppState.engines` registry, sysinfo MB fix, zip bomb limits (200k / 8GB), MSA refresh paths fix |
| **D29** | Wave of 5 agents (release, perf, mrpack, etc.) | `src-tauri/tauri.conf.json:3`<br>`src-tauri/src/modrinth/mrpack.rs:200`<br>`src/ui/components/InstanceCard.tsx` | **Confirmed** | "Cobble Launcher", rollback on mrpack fail, `memo(InstanceCard)`, `sysinfo` pid_alive |
| **D30** | 3 languages: en (default), ru, uk | `src/i18n/{en,ru,uk}.json`<br>`src/i18n/index.ts:1-141` | **Confirmed** | 226 keys in each dictionary, full parity, 3-language `errors.json` |
| **D31** | Audit remediation (A1–A58) | `src-tauri/src/commands/mod.rs:1040`<br>`src-tauri/src/instances/mod.rs:230`<br>`src/ui/hooks/useModalA11y.ts` | **Confirmed** | `validate_instance_settings`, `SAVE_LOCK` atomic update, `useModalA11y` focus traps, `NOTICE` + licenses |
| **D32** | Feedback wave (CREATE_NO_WINDOW, CF-style detail, fonts, update_check) | `src-tauri/src/instances/run.rs:405`<br>`src/ui/pages/InstanceDetailPage.tsx`<br>`src-tauri/src/commands/mod.rs:1141` | **Confirmed** | `CREATE_NO_WINDOW` flag, `InstanceDetailPage`, `uiFont`/`uiAccent`, `update_check` command |
| **D33** | Visual wave (CF catalog cards, batch projects, self-healing) | `src/ui/pages/ModsPage.tsx`<br>`src-tauri/src/commands/mod.rs:327, 459`<br>`src-tauri/src/modrinth/updates.rs:80` | **Confirmed** | Batch metadata `/v2/projects?ids=`, `content_backfill_projects`, popular cards on Home |
| **D34** | Profile skins | `src-tauri/src/auth/skins.rs:1-350`<br>`src/ui/components/SkinViews.tsx:1-250` | **Confirmed** | Ely skinsystem, Mojang sessionserver, offline procedural texture, 1h disk cache, canvas body |
| **D35** | CF-style instance cards | `src/ui/components/InstanceCard.tsx:1-125`<br>`src/ui/defaultArt.ts:1-110` | **Confirmed** | 4:3 art, version badge, Play overlay, procedural pixel landscapes, `InstanceKebabMenu` |

---

## 6. Master Map of Extension Points (11 Functional Domains)

This section maps exact file paths, structs, functions, data flows, and concrete implementation recipes for extending `mc-launcher-v2`.

### 6.1 Domain 1: Instances (`instances`)

#### Key Artifacts:
- **Structs:**
  - `Instance` (`src-tauri/src/instances/mod.rs:19-38`): Main entity.
  - `AppState` (`src-tauri/src/commands/mod.rs:40-88`): In-memory runtime state (`paths`, `bus`, `settings`, `http`, `launching`, `engines`).
  - `InstanceLock` (`src-tauri/src/instances/mod.rs:400-440`): File lock guardian.
- **Functions:**
  - `instances::list`, `instances::load`, `instances::save`, `instances::update` (`src-tauri/src/instances/mod.rs:172-240`).
  - `instances::duplicate`, `instances::rename`, `instances::delete` (`src-tauri/src/instances/mod.rs:253-345`).
  - `store::copy_tree_mixed` (`src-tauri/src/instances/store.rs:77-124`).
- **Frontend Hooks & Components:**
  - `useInstances` (`src/state/instances.ts:1-180`): Zustand store with `list`, `status`, `load`, `launch`, `stop`, `optimize`, `remove`, `duplicate`.
  - `InstanceCard.tsx`, `InstanceDetailPage.tsx`, `CreateInstanceDialog.tsx`, `InstanceSettingsModal.tsx`, `InstanceKebabMenu.tsx`.

#### Extension Hook Points & Recipes:
1. **Instance Groups / Tags:**
   - *Backend Hook:* Add `pub groups: Vec<String>` (with `#[serde(default)]`) to `struct Instance` in `src-tauri/src/instances/mod.rs:37`.
   - *Frontend Hook:* Add `groups?: string[]` to `interface Instance` in `src/api/types.ts:39`. In `InstancesPage.tsx`, add a group tab/filter bar above the instance grid.
2. **Favorite / Pinned Instances:**
   - *Backend Hook:* Add `pub favorite: bool` (default `false`) to `Instance` in `instances/mod.rs`.
   - *Frontend Hook:* Add star toggle button to `InstanceCard.tsx` and sort favorites first in `useInstances`.
3. **World / Saves Manager:**
   - *Backend Hook:* In `src-tauri/src/instances/mod.rs`, add functions `list_worlds(&Paths, id) -> Result<Vec<WorldInfo>>`, `backup_world(...)`, `delete_world(...)`. Worlds are located in `instance_dir.join("minecraft/saves")`. Expose new IPC commands `instance_worlds_list`, `instance_world_backup`, `instance_world_delete` in `commands/mod.rs` and `lib.rs`.
   - *Frontend Hook:* Add a new tab `"worlds"` in `InstanceDetailPage.tsx:100` alongside Overview, Mods, and Logs.
4. **Screenshots Gallery:**
   - *Backend Hook:* In `src-tauri/src/instances/mod.rs`, add `list_screenshots(&Paths, id) -> Result<Vec<ScreenshotInfo>>` scanning `instance_dir.join("minecraft/screenshots")`. Expose IPC command `instance_screenshots_list`.
   - *Frontend Hook:* Add a `"screenshots"` tab in `InstanceDetailPage.tsx`.
5. **Archive / Directory Import Integration:**
   - *Fix Hook:* In `InstancesPage.tsx:110` and `HomePage.tsx:50`, add an "Импортировать архив" button invoking `pickFilePath` (`.zip`, `.mrpack`) and calling `api.instanceImport(path)`.

---

### 6.2 Domain 2: Content & Modrinth (`content`, `modrinth`, `loaders`)

#### Key Artifacts:
- **Structs:**
  - `ContentEntry` (`src-tauri/src/instances/mod.rs:60-75`): `kind`, `file`, `source`, `project_id`, `version_id`, `sha1`, `url`, `enabled`.
  - `SearchHit`, `SearchResult`, `ProjectMeta` (`src-tauri/src/modrinth/api.rs:15-50`).
  - `UpdateCheck` (`src-tauri/src/modrinth/updates.rs:25-35`).
- **Functions:**
  - `modrinth::api::search` (`src-tauri/src/modrinth/api.rs:65-100`).
  - `modrinth::install::install_content` (`src-tauri/src/modrinth/install.rs:20-180`).
  - `instances::content::toggle`, `remove`, `backup`, `export_mrpack` (`src-tauri/src/instances/content.rs:68-238`).
  - `modrinth::updates::check_updates`, `update_all`, `backfill_projects` (`src-tauri/src/modrinth/updates.rs:35-150`).
- **Frontend Components:**
  - `ModsPage.tsx`, `InstanceDetailPage.tsx` (Mods tab).

#### Extension Hook Points & Recipes:
1. **Catalog Filters (Categories & Sorting):**
   - *Backend Hook:* In `src-tauri/src/modrinth/api.rs:65-100`, `search` already constructs `facets` query parameter. Extend `modrinth_search` command parameters in `commands/mod.rs:302` to accept `categories: Option<Vec<String>>` and `sort: Option<String>` (`"relevance"`, `"downloads"`, `"follows"`, `"newest"`, `"updated"`).
   - *Frontend Hook:* In `src/ui/pages/ModsPage.tsx:60-90`, add category filter chips and a sort dropdown.
2. **Datapacks Support:**
   - *Backend Hook:* In `src-tauri/src/instances/content.rs:21`, extend `CONTENT_DIRS` to `["mods", "resourcepacks", "shaderpacks", "datapacks"]`. In `ContentKind`, add `Datapack`. Datapack files live under `minecraft/saves/<world>/datapacks` or template folder.
   - *Frontend Hook:* Add `"datapacks"` to tabs in `ModsPage.tsx:35`.
3. **Forge Loader UI Restoration:**
   - *Backend Hook:* In `src-tauri/src/commands/mod.rs:879-904` (`loader_versions`), add match arm for `"forge"`:
     ```rust
     "forge" => Ok(crate::loaders::forge::list_versions(&http, &mc_version).await?...)
     ```
   - *Frontend Hook:* In `src/ui/components/CreateInstanceDialog.tsx:14`, add `{ id: "forge", label: "Forge" }` to `LOADERS`.
4. **Mod Dependency Tree / Optional Dependencies Dialog:**
   - *Backend Hook:* In `src-tauri/src/modrinth/install.rs:60-120`, inspect `dependencies[]` where `dependency_type == "optional"`. Return `(Vec<ContentEntry>, Vec<OptionalDep>)`.
   - *Frontend Hook:* Modal prompting user whether to install optional dependencies.

---

### 6.3 Domain 3: Accounts & Auth (`auth`)

#### Key Artifacts:
- **Structs:**
  - `Account` (`src-tauri/src/auth/mod.rs:20-35`): `id`, `kind`, `name`, `uuid`, `refresh_ref`.
  - `AccountKind` (`Offline`, `Msa`, `Ely`).
  - `AccountsFile` (`src-tauri/src/auth/mod.rs:45-120`).
- **Functions:**
  - `auth::offline::create_offline_account` (`src-tauri/src/auth/offline.rs:15-30`).
  - `auth::msa::auth_code_flow_browser`, `refresh_session`, `launch_identity` (`src-tauri/src/auth/msa.rs:140-350`).
  - `auth::ely::authenticate`, `authlib_injector` (`src-tauri/src/auth/ely.rs:20-150`).
  - `auth::store_token`, `get_token`, `delete_token` via Windows Credential Manager (`src-tauri/src/auth/mod.rs:140-230`).
- **Frontend Components:**
  - `AccountsModal.tsx`, `useAccounts` (`src/state/accounts.ts`), `Shell.tsx` (topbar profile chip).

#### Extension Hook Points & Recipes:
1. **Generic Custom Authlib / Yggdrasil Servers (LittleSkin, Bleess, Self-hosted):**
   - *Backend Hook:* Generalize `src-tauri/src/auth/ely.rs` into a generic authlib handler in `src-tauri/src/auth/yggdrasil.rs`. Add `AccountKind::CustomYggdrasil { auth_url: String }`. When launching in `src-tauri/src/instances/run.rs:370-395`, inject `-javaagent:authlib-injector.jar=<auth_url>`.
   - *Frontend Hook:* Add a 4th tab "Кастомный сервер" in `AccountsModal.tsx` asking for Server Auth URL, username, and password.
2. **Per-Instance Account Binding:**
   - *Backend Hook:* In `struct Instance` (`instances/mod.rs:37`), add `pub account_id: Option<String>`. In `instances/run.rs:70`, resolve player identity using `instance.account_id` if set, otherwise active account.
   - *Frontend Hook:* In `InstanceSettingsModal.tsx`, add an "Аккаунт для запуска" selector.

---

### 6.4 Domain 4: Skins (`skins`)

#### Key Artifacts:
- **Structs:**
  - `SkinInfo` (`src-tauri/src/auth/skins.rs:25-30`): `source: SkinSource`, `data_url: String`.
  - `SkinSource` (`Ely`, `Mojang`).
- **Functions:**
  - `auth::skins::fetch_skin` (`src-tauri/src/auth/skins.rs:40-160`): fetches, validates, and caches skins.
  - `account_skin` command (`src-tauri/src/commands/mod.rs:786-799`).
- **Frontend Components:**
  - `SkinViews.tsx` (`SkinHead`, `SkinBody`, `ProceduralSkinTexture`, `CharacterSection`).
  - `useSkins` (`src/state/skins.ts`).

#### Extension Hook Points & Recipes:
1. **Local Custom Skin File for Offline Profiles:**
   - *Backend Hook:* In `src-tauri/src/auth/skins.rs:50-58`, check if a local skin file exists at `paths.cache_dir().join("skins/custom_{account_id}.png")`. If present, return it with `SkinSource::Local`. Add IPC command `account_skin_upload(account_id, image_path)`.
   - *Frontend Hook:* In `AccountsModal.tsx` CharacterSection, show "Загрузить скин (файл)" for offline profiles.
2. **Capes (Плащи):**
   - *Backend Hook:* Add `struct CapeInfo { data_url: String }` and `pub cape: Option<CapeInfo>` in `SkinInfo`. Parse cape texture from Mojang sessionserver response and Ely.by skinsystem (`skinsystem.ely.by/capes/{username}.png`).
   - *Frontend Hook:* Add cape render toggle in `SkinBody` canvas preview.

---

### 6.5 Domain 5: Java Management (`java`)

#### Key Artifacts:
- **Structs:**
  - `JavaInstall` (`src-tauri/src/java/detect.rs:9-17`): `java_exe`, `major`, `arch_bits`, `origin`.
- **Functions:**
  - `java::detect::scan_system_javas` (`src-tauri/src/java/detect.rs:30-140`).
  - `java::detect::select_best_java` (`src-tauri/src/java/detect.rs:150-180`).
  - `java::adoptium::install_adoptium_jre` (`src-tauri/src/java/adoptium.rs:20-130`).
  - Commands: `java_list`, `java_install` (`src-tauri/src/commands/mod.rs:931-948`).
- **Frontend Components:**
  - `SettingsPage.tsx` (Java section), `InstanceSettingsModal.tsx` (Java dropdown).

#### Extension Hook Points & Recipes:
1. **Auto-Install Missing Java During Launch:**
   - *Backend Hook:* In `src-tauri/src/instances/run.rs:90-120`, if `select_best_java` finds no suitable Java runtime, automatically call `install_adoptium_jre(http, paths.runtime_dir(), required_major).await` with `launch_state: downloading` event.
2. **Support Additional Java Vendors / Majors (Java 8 & Java 25):**
   - *Backend Hook:* In `src-tauri/src/java/adoptium.rs:35`, Adoptium API supports majors `8`, `17`, `21`, `25`.
   - *Frontend Hook:* In `SettingsPage.tsx:240-270`, add 1-click buttons for Java 8 (for 1.12.2 legacy instances) and Java 25.
3. **Uninstall / Remove Adoptium Runtime:**
   - *Backend Hook:* Add command `java_uninstall(major: u32)` that deletes `paths.runtime_dir().join(format!("jdk{major}"))`.

---

### 6.6 Domain 6: Download Engine & Network (`net`)

#### Key Artifacts:
- **Structs:**
  - `HttpClient` (`src-tauri/src/net/http.rs:10-50`): Thread-safe `reqwest::Client` wrapper.
  - `DownloadEngine` (`src-tauri/src/net/download.rs:40-250`): High-throughput queue.
  - `DownloadItem` (`src-tauri/src/net/download.rs:15-30`): URL, destination, SHA1 hash, priority.
  - `DlProgress`, `DlQueueState` (`src-tauri/src/events.rs:24-47`).
- **Functions:**
  - `DownloadEngine::new`, `submit`, `cancel_group`, `progress`, `queue_state`.
  - `AppState::engine_for`, `cancel_engine_group`, `engine_done` (`src-tauri/src/commands/mod.rs:65-88`).

#### Extension Hook Points & Recipes:
1. **Bandwidth Speed Limiter:**
   - *Backend Hook:* In `src-tauri/src/settings.rs:40`, add `pub max_download_speed_kbps: Option<u32>`. In `src-tauri/src/net/download.rs:260-310`, wrap file streaming with token-bucket rate limiter.
   - *Frontend Hook:* Add speed limiter slider/input in `SettingsPage.tsx` under Downloads & Network.
2. **Emit `dl_queue_state` Events (Fixing DISC-A04):**
   - *Backend Hook:* In `src-tauri/src/net/download.rs:220` (worker task completion) and `progress_emitter`, emit `LauncherEvent::DlQueueState(self.queue_state())` on the EventBus.
   - *Frontend Hook:* Bind queue state to global download progress tray or drawer.
3. **Offline Mode Toggle ("Work Offline"):**
   - *Backend Hook:* Add `pub work_offline: bool` in `Settings`. In `HttpClient::get`, immediately return `Err(LauncherError::Network("Режим офлайн активен"))` when true.
   - *Frontend Hook:* Add "Работать автономно" toggle in `SettingsPage.tsx` and titlebar banner.

---

### 6.7 Domain 7: UI & Layout (`ui/layout`, `theme`)

#### Key Artifacts:
- **Files:**
  - `src/theme/tokens.css`: Core design system variables (`--bg`, `--card`, `--accent`, `--border`, etc.).
  - `src/main.tsx`: Synchronous theme initialization.
  - `src/state/settings.ts`: UI scale, font, and accent CSS variable overrides.
  - `src/ui/layout/Shell.tsx`: Topbar, left navigation sidebar, notifications, drag-and-drop.
  - `src/ui/hooks/useModalA11y.ts`: Universal modal focus/escape hook.

#### Extension Hook Points & Recipes:
1. **Global Shortcut / Quick Play:**
   - *Frontend Hook:* In `Shell.tsx:120`, register global keyboard listener for `Ctrl+Enter` to launch the active/last-played instance.
2. **Download Drawer / Tray Component:**
   - *Frontend Hook:* In `Shell.tsx:200`, add a slide-up floating drawer in the bottom-right corner showing active downloads, current speed, and files remaining when `dl_progress` is active.
3. **Notification System (Toast Notifications):**
   - *Frontend Hook:* In `Shell.tsx`, replace single `notice` state banner with a stacked toast notifications provider for backup completion, export success, and background errors.

---

### 6.8 Domain 8: Process Supervision & Crash Analysis (`process`, `run`)

#### Key Artifacts:
- **Structs:**
  - `Diagnosis` (`src-tauri/src/process/crash.rs:10-20`): `rule_id`, `title`, `advice`.
  - `LaunchPhase` (`src-tauri/src/events.rs:9-15`): `preparing`, `downloading`, `launching`, `running`, `exited`.
- **Functions:**
  - `instances::run::launch_detached` (`src-tauri/src/instances/run.rs:400-750`).
  - `process::crash::analyze` (`src-tauri/src/process/crash.rs:25-90`).
  - Commands: `instance_launch`, `instance_kill`, `process_status`, `logs_tail`, `crash_analyze`.
- **Frontend Components:**
  - `LogViewerModal.tsx`, `CrashReportModal.tsx`, `InstanceDetailPage.tsx` (Logs tab).

#### Extension Hook Points & Recipes:
1. **Share Log via Paste Service (mclo.gs / bytebin):**
   - *Backend Hook:* In `src-tauri/src/commands/mod.rs`, add command `log_share(instance_id: String) -> Result<String>`. Read instance's `logs/latest.log`, redact tokens, send HTTP POST to `https://api.mclo.gs/1/log`, return public URL.
   - *Frontend Hook:* Add "Поделиться логом" button in `LogViewerModal.tsx` and `CrashReportModal.tsx`.
2. **Expandable Crash Diagnostics JSON:**
   - *Backend Hook:* In `src-tauri/src/process/crash.rs:30`, load custom rule patterns from `paths.data_dir().join("crash_rules.json")` before falling back to built-in rules.
3. **Discord Rich Presence (RPC):**
   - *Backend Hook:* In `src-tauri/src/instances/run.rs:700` (supervisor loop), integrate `discord-rich-presence` crate to publish game status ("Playing Minecraft 1.21.1", elapsed time, instance name).

---

### 6.9 Domain 9: Settings & Configuration (`settings`)

#### Key Artifacts:
- **Structs:**
  - `Settings` (`src-tauri/src/settings.rs:35-61`).
- **Functions:**
  - `Settings::load`, `Settings::save`, `Settings::migrate` (`src-tauri/src/settings.rs:90-150`).
  - Commands: `settings_get`, `settings_set` (`src-tauri/src/commands/mod.rs:950-971`).
- **Frontend Components:**
  - `useSettings` (`src/state/settings.ts`), `SettingsPage.tsx`.

#### Extension Hook Points & Recipes:
1. **Close to Tray / Minimize to Tray on Game Launch:**
   - *Backend Hook:* Add `pub close_to_tray: bool` in `Settings`. In `src-tauri/src/instances/run.rs:415`, if enabled, call `app_handle.get_webview_window("main").unwrap().hide()`.
   - *Frontend Hook:* Add toggle in `SettingsPage.tsx` under General.
2. **Reset Launcher Data / Export All Settings:**
   - *Backend Hook:* Add command `settings_export() -> Result<String>` and `settings_import(json: String)`.

---

### 6.10 Domain 10: Internationalization (`i18n`)

#### Key Artifacts:
- **Files:**
  - `src/i18n/index.ts`: `t`, `plural`, `errorText`, `apiErrorText`, `setLanguage`, `currentLanguage`.
  - `src/i18n/en.json`, `ru.json`, `uk.json` (226 keys each).
  - `src/i18n/errors.json` (14 error codes).

#### Extension Hook Points & Recipes:
1. **Eliminate Bilingual Error Hints (A39 Solution):**
   - *Backend Hook:* In `src-tauri/src/errors.rs:96-111`, instead of returning hardcoded Russian strings in `hint()`, return translation keys (e.g. `hint: Some("errors.hints.hash_mismatch")`).
   - *Frontend Hook:* In `src/i18n/index.ts:117` (`errorText`), translate `e.hint` via `t(e.hint)` so the hint matches the active UI language.
2. **Adding a New Language (e.g. `de.json`):**
   - *Frontend Hook:* Create `src/i18n/de.json` with 226 keys, import in `src/i18n/index.ts`, add `"de"` to `dicts` map and `SettingsPage.tsx` language dropdown.

---

### 6.11 Domain 11: Auto-Update (`update`)

#### Key Artifacts:
- **Backend:**
  - `src-tauri/src/commands/mod.rs:1141-1155` (`update_check`).
  - `src-tauri/tauri.conf.json:30-40` (`bundle.windows.updater`).
- **Frontend:**
  - `src/ui/components/UpdatesSection.tsx`, `SettingsPage.tsx`.

#### Extension Hook Points & Recipes:
1. **Enable Official Tauri In-App Updater:**
   - *Setup:* Run `scripts/gen-keys.ps1` to produce public/private signing keypair. Paste public key into `tauri.conf.json:35` under `plugins.updater.pubkey`.
   - *Backend Hook:* Expose `tauri-plugin-updater` command `app_update_install`.
   - *Frontend Hook:* In `UpdatesSection.tsx`, when `updateAvailable` is true, show "Установить обновление и перезапустить" button.

---

## 7. Verification Method

To independently verify the facts established in this inventory:
1. **IPC Commands Check:**
   - Inspect `src-tauri/src/lib.rs:141-192` (`generate_handler!`). Count is exactly 50 commands.
   - Inspect `docs/UI-CONTRACT.md:20`. Count claimed is 47; search for `modrinth_projects`, `content_backfill_projects`, `update_check` — confirm they are absent.
2. **Forge Omission Check:**
   - Inspect `src-tauri/src/commands/mod.rs:879-904`. Confirm match arms only cover `"fabric"`, `"quilt"`, and `"neoforge"`.
   - Inspect `src/ui/components/CreateInstanceDialog.tsx:9-14`. Confirm `LOADERS` contains only `none`, `fabric`, `quilt`, `neoforge`.
3. **Dead Event Check:**
   - Run `git grep "LauncherEvent::DlQueueState" src-tauri/src/`. Confirm match appears ONLY in `commands/mod.rs:998` (inside `emit_event` match), but nowhere in `download.rs` or any emit site.
4. **Orphaned Import Command Check:**
   - Run `git grep "instanceImport" src/`. Confirm it appears only in `src/api/client.ts:49` and nowhere in `src/ui/`.
