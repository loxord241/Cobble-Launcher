# BRIEFING — 2026-09-27T17:40:00Z

## Mission
Audit Direction 1 (Rust Core Correctness), Direction 2 (Core IPC Boundaries), and Direction 3 (Events & Types Contract) for mc-launcher-v2.

## 🔒 My Identity
- Archetype: explorer
- Roles: Rust Core Correctness, Core IPC Boundaries, Events & Types Contract
- Working directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster2
- Original parent: 99709a5d-49a1-4bfb-89f4-5a051b9f4dcb
- Milestone: Audit of Directions 1, 2, and 3

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Product code in `src/` and `src-tauri/src/` MUST NOT be modified! Read-only investigation.
- No real game launches.
- Strictly mask any tokens, secrets, or personal data.
- Every claim and finding MUST cite exact `file:line` references.

## Current Parent
- Conversation ID: 99709a5d-49a1-4bfb-89f4-5a051b9f4dcb
- Updated: 2026-09-27T17:40:00Z

## Investigation State
- **Explored paths**:
  - `src-tauri/src/lib.rs` (invoke_handler, logging writer)
  - `src-tauri/src/commands/mod.rs` (46 command handlers, AppState)
  - `src-tauri/src/instances/mod.rs`, `instances/content.rs`, `instances/run.rs`, `instances/store.rs`
  - `src-tauri/src/net/download.rs`, `net/http.rs`
  - `src-tauri/src/modrinth/install.rs`, `modrinth/mrpack.rs`, `modrinth/updates.rs`, `modrinth/api.rs`
  - `src-tauri/src/java/detect.rs`, `java/adoptium.rs`
  - `src-tauri/src/events.rs`
  - `src-tauri/src/settings.rs`
  - `src-tauri/src/errors.rs`
  - `src-tauri/src/loaders/forge.rs`, `loaders/neoforge.rs`
  - `docs/UI-CONTRACT.md`
  - `src/api/types.ts`, `src/api/client.ts`
  - `src/ui/components/InstanceSettingsModal.tsx`, `src/ui/pages/SettingsPage.tsx`
- **Key findings**:
  - A-C2-01 (P0): Path Traversal & Arbitrary Deletion in `instance_delete` (`instances/mod.rs:291-314`).
  - A-C2-02 (P0): Command Injection in PowerShell in `recycle_dir` (`instances/mod.rs:320-327`).
  - A-C2-03 (P1): Path Traversal & Arbitrary Execution in `instance_open_dir` (`commands/mod.rs:139-144`).
  - A-C2-04 (P1): Logger panic on `.unwrap()` in fallback (`lib.rs:51-53`).
  - A-C2-05 (P1): Ghost mod installation in manifest on cancelled download (`modrinth/install.rs:183-193`).
  - A-C2-06 (P2): Incomplete rollback on mrpack install failure (`modrinth/mrpack.rs:193-211`).
  - A-C2-07 (P1): Corrupt file cache poisoning in store `link_or_copy` (`instances/store.rs:14-19`).
  - A-C2-08 (P2): Missing `spawn_blocking` on heavy IO (`duplicate`, `delete`, `list`, `rename`).
  - A-C2-09 (P2): Race condition between `instance_settings_set` and process supervisor on `instance.json`.
  - A-C2-10 (P1): `JavaInstall` snake_case schema breaks UI Java dropdown (`java/detect.rs:9-17`).
  - A-C2-11 (P2): `VersionEntry` snake_case schema (`commands/mod.rs:764-770`).
  - A-C2-12 (P2): Missing `dl_queue_state` listener and type in frontend.
  - A-C2-13 (P2): 21 missing commands in `docs/UI-CONTRACT.md`.
- **Unexplored areas**: None for Directions 1, 2, 3 (exhaustively investigated).

## Key Decisions Made
- Performed rigorous static and semantic investigation of all 46 commands, all events, and core concurrency/IO models.
- Documented complete findings in `report.md` and concise 5-component report in `handoff.md`.

## Artifact Index
- `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster2\report.md` — Complete detailed audit report
- `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster2\handoff.md` — 5-component handoff report
- `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster2\progress.md` — Heartbeat
- `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster2\DISPATCH.md` — Dispatch record
