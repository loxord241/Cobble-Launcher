# Audit Plan: mc-launcher-v2 (Cobble Launcher)

## Overview & Objectives
Execute an exhaustive, independent, zero-code-modification audit of `mc-launcher-v2` covering:
1. R1: Full verification of all items marked «УСТРАНЕНО» in `AUDIT.md` and architectural decisions `D26`–`D30` in `docs/DECISIONS.md` with exact `file:line` references and verdicts (confirmed / refuted / partially confirmed).
2. R2: Deep quality and security audit across 12 directions without duplicating discovery backlog D1-D29.
3. R3: Creation of `FULL_AUDIT_REPORT.md` (in repository root, added to `.gitignore`) and update of `AUDIT.md` §7.
4. R4: Absolute compliance with constraints: zero product code changes in `src/` and `src-tauri/src/`, no real game launches, full token/secret masking.

## Investigation Breakdown (5 Parallel Explorer Clusters)

### Cluster 1: R1 Verification & Direction 11 (Documentation & Release Metadata)
- **Explorer 1** (`explorer_cluster1`):
  - Audit every item marked «УСТРАНЕНО» in `AUDIT.md` (e.g. in §1, §2, §3, §4, §5, etc.).
  - Audit architectural decisions `D26` through `D30` in `docs/DECISIONS.md`.
  - Direction 11: Validate all `README.md` claims against real code.
  - Direction 11: Version synchronization (0.1.0) across `tauri.conf.json`, `Cargo.toml`, `package.json`, `package-lock.json`.
  - Direction 11: NSIS/MSI installer metadata, application name "Cobble Launcher", identifiers.
  - Direction 11: Currency of `docs/PROGRESS.md` and `docs/UI-CONTRACT.md`.
  - Deliverable: Detailed verification report with `file:line` proof for each item, plus Direction 11 audit findings.

### Cluster 2: Directions 1, 2, 3 (Rust Core Correctness, IPC Boundaries, Events/Types Contract)
- **Explorer 2** (`explorer_cluster2`):
  - Direction 1: Race conditions (lock/save order, concurrent instance.json writes across threads/commands).
  - Direction 1: spawn_blocking coverage for blocking disk/network calls.
  - Direction 1: Cancellation correctness (residual files, integrity checks on abort during download/launch).
  - Direction 1: Unwraps/expects/panics in production code paths.
  - Direction 1: I/O failure handling (disk full, OneDrive/antivirus file locks).
  - Direction 2: Exhaustive audit of all Tauri commands in `src-tauri/src/lib.rs`.
  - Direction 2: Complete mapping table: `Command -> Argument -> Validation -> Risk`.
  - Direction 2: Unvalidated user strings passed to file ops (path traversal, shell injection, lack of normalization, focusing on `instance_id`, `path`, `file`, `name`, compared against `resolve_content_file` in `instances/content.rs`).
  - Direction 3: 3-way parity contract: `src-tauri/src/events.rs` <-> `docs/UI-CONTRACT.md` <-> `src/api/types.ts`.
  - Direction 3: Field naming (camelCase/snake_case), optionality/nullability, schema discrepancies across all events.
  - Deliverable: Comprehensive technical report on Directions 1, 2, 3.

### Cluster 3: Directions 4, 5, 6 (Frontend State, a11y & Navigation, i18n Quality)
- **Explorer 3** (`explorer_cluster3`):
  - Direction 4: Game log batching (tail loss prevention on Exited, flush timer race conditions).
  - Direction 4: Reactivity & memoization (`memo(InstanceRow)`, unnecessary re-renders).
  - Direction 4: Zustand `useShallow` usage and direct access to unselected mutating state.
  - Direction 4: Resource cleanup (Tauri `unlisten()` on unmount, `clearTimeout`/`clearInterval`, global focus/keyboard listeners).
  - Direction 5: Modal dialog audit (Escape key, overlay click, focus trap, return focus to trigger, `role="dialog"`, `aria-label`/`aria-labelledby`).
  - Direction 5: Accessible names (`aria-label`) on all icon-only buttons.
  - Direction 5: Tab navigation order (`tabindex`).
  - Direction 5: E2E test selector parity (`scripts/*.cjs` vs actual component DOM structures).
  - Direction 6: i18n dictionary proofreading (en, ru, uk) for semantic consistency and correctness.
  - Direction 6: Positional placeholders (`{n}`, `{name}`).
  - Direction 6: Pluralization and Russian/Ukrainian numeral inflections.
  - Direction 6: Unregistered hardcoded strings in TSX/JSX bypassing `t()`.
  - Direction 6: Language switching resilience and fallback behavior for `errorText`.
  - Deliverable: Comprehensive technical report on Directions 4, 5, 6.

### Cluster 4: Directions 7, 8 (Security & Isolation, Test Coverage & Reliability)
- **Explorer 4** (`explorer_cluster4`):
  - Direction 7: CSP (Content Security Policy) configuration in production Tauri build.
  - Direction 7: IPC attack surface analysis assuming compromised WebView.
  - Direction 7: Drag & Drop security (input format validation, structure verification of `.mrpack`, rejection of malicious files).
  - Direction 7: Native dialog filters & magic bytes sniffing (signature verification for icons/uploads).
  - Direction 7: Secret isolation & keyring service security, redaction filter verification on token-bearing test strings.
  - Direction 7: Principle of least privilege for Tauri capabilities configuration.
  - Direction 8: Identification of Top 10 critical uncovered core & UI commands/branches with concrete test scenarios.
  - Direction 8: Flaky test analysis (timing dependencies, live network dependencies, order dependence).
  - Direction 8: Potential race conditions in parallel Cargo test execution.
  - Direction 8: Mock-only tests checking mock setup instead of real system invariants.
  - Deliverable: Comprehensive technical report on Directions 7, 8.

### Cluster 5: Directions 9, 10, 12 (Performance, Windows Compatibility, Licensing & Brand)
- **Explorer 5** (`explorer_cluster5`):
  - Direction 9: Launch critical path analysis (startup bottlenecks, operations blocking initial window render).
  - Direction 9: Scalability under load (100+ instances, hundreds of mods).
  - Direction 9: Memory footprint (game log buffers, data-URL icon storage overhead in `instance.json`).
  - Direction 9: `sysinfo` periodic snapshot frequency and overhead.
  - Direction 10: Windows long path handling (>260 chars, uniformity of `long_path`).
  - Direction 10: Spaces and non-ASCII / Cyrillic characters in data paths (`%LOCALAPPDATA%`), instance names, binary name (`Cobble Launcher.exe`).
  - Direction 10: File locking resilience (antivirus, Windows Search indexer, OneDrive synchronization).
  - Direction 10: Hard link content store creation and rollback integrity.
  - Direction 12: Cargo dependency tree license compatibility with MIT.
  - Direction 12: Requirement evaluation for `NOTICE` / `THIRD-PARTY-LICENSES`.
  - Direction 12: Mojang Minecraft brand guidelines compliance (avoiding false official endorsement).
  - Direction 12: Local assets, icons, and mock graphics licensing verification.
  - Deliverable: Comprehensive technical report on Directions 9, 10, 12.

## Synthesis & Artifact Writing (Worker 1)
- Once all 5 clusters complete, aggregate and reconcile all findings into a unified registry:
  - Findings A1..A# categorized with priority P0-P3, effort, direction, file:line proof, and mitigation proposal.
  - Top 20 findings table.
  - Section-by-section verdict for all 12 directions.
  - Executive summary (5 key takeaways for the launcher owner).
  - Verification summary table for AUDIT.md and D26-D30.
- Worker 1 will create `FULL_AUDIT_REPORT.md`, update `.gitignore` (to ensure `FULL_AUDIT_REPORT.md` is ignored), and update `AUDIT.md` with §7.

## Review & Verification
- Reviewer checks report completeness, evidence chains, no hallucinated line numbers, exact formatting match with specifications.
- Forensic Auditor performs integrity verification ensuring zero product source code changes were made (`git status`/`git diff` check).

## Timeline & Milestones
- M1: Dispatch 5 Explorer clusters [IN PROGRESS]
- M2: Aggregate cluster findings & build findings matrix [PENDING]
- M3: Author `FULL_AUDIT_REPORT.md`, update `.gitignore`, update `AUDIT.md` §7 via Worker [PENDING]
- M4: Review & Forensic Audit [PENDING]
- M5: Final Delivery to Parent [PENDING]
