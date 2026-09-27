# BRIEFING — 2026-09-27T17:40:00Z

## Mission
Audit Direction 7 (Security & Isolation) and Direction 8 (Test Coverage & Reliability) for mc-launcher-v2.

## 🔒 My Identity
- Archetype: explorer
- Roles: Security & Isolation auditor, Test Coverage & Reliability auditor
- Working directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster4
- Original parent: 99709a5d-49a1-4bfb-89f4-5a051b9f4dcb
- Milestone: Security & Testing Audit

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Product code in `src/` and `src-tauri/src/` MUST NOT be modified!
- No real game launches.
- Strictly mask any tokens, secrets, or personal data.
- Every claim and finding MUST cite exact `file:line` references.

## Current Parent
- Conversation ID: 99709a5d-49a1-4bfb-89f4-5a051b9f4dcb
- Updated: 2026-09-27T17:40:00Z

## Investigation State
- **Explored paths**:
  - `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`
  - `src-tauri/src/lib.rs`, `src-tauri/src/commands/mod.rs`
  - `src-tauri/src/instances/mod.rs`, `instances/content.rs`, `instances/run.rs`
  - `src-tauri/src/modrinth/mrpack.rs`, `src-tauri/src/util/zip.rs`, `util/redact.rs`
  - `src-tauri/src/auth/mod.rs`, `auth/msa.rs`, `auth/ely.rs`
  - `src-tauri/src/mojang/launch.rs`, `mojang/manifest.rs`
  - `src/ui/layout/Shell.tsx`, `src/ui/components/pickFile.ts`, `src/ui/pages/ModsPage.tsx`
  - `src-tauri/tests/download_engine.rs`, `scripts/*.cjs`
- **Key findings**:
  - SEC-1 (P0): RCE via `instance_settings_set` + `instance_launch` without JVM flag sanitization.
  - SEC-2 (P0): Arbitrary directory deletion / DoS via missing `valid_id` check in `instance_delete`.
  - SEC-3 (P1): Token leaks in game logs and IPC events due to `RedactingWriter` being scoped only to `launcher.log`.
  - SEC-4 (P1): Local file read oracle up to 300KB in `instance_icon_set`.
  - SEC-5 (P1): Missing host validation for `.mrpack` downloads (SSRF risk) and symlink check omission in stripping zip extraction.
  - SEC-6 (P2): Missing `base-uri`, `object-src`, `form-action` in production CSP.
  - TEST-1 (P1): Core `instance_launch` has 0 tests in Rust test suite.
  - TEST-2 (P2): Flaky tests with `sleep` in `download_engine.rs` and `mrpack.rs`.
  - TEST-3 (P2): Parallel test race conditions with static `registry()` in `util/redact.rs` and Windows Credential Manager.
  - TEST-4 (P2): Mock-only tests checking only serde JSON deserialization.
- **Unexplored areas**: None within the scope of Direction 7 & 8.

## Key Decisions Made
- Completed systematic audit and created comprehensive `report.md`.
- Ready to emit `handoff.md` and message orchestrator.

## Artifact Index
- DISPATCH.md — Recorded dispatch instructions
- BRIEFING.md — Working memory & identity
- progress.md — Heartbeat and status
- report.md — Comprehensive findings for Direction 7 and 8
- handoff.md — 5-component handoff report
