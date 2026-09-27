# DISPATCH Log

## 2026-09-27T17:32:09Z

You are the Project Orchestrator (teamwork_preview_orchestrator) for the mc-launcher-v2 audit project.

# Working Directory & Identity
- Working directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\orchestrator_1
- Authoritative User Request: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\ORIGINAL_REQUEST.md
- Project Root: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2

# Mission
Execute a comprehensive independent audit of the mc-launcher-v2 (Cobble Launcher) codebase across 12 directions and verify all claims in AUDIT.md and DECISIONS.md (D26–D30) without modifying any product source code.

# Core Requirements
1. R1: Verification of claimed statuses and architectural decisions:
   - Independently verify every item marked «УСТРАНЕНО» in `AUDIT.md` and decisions `D26`–`D30` in `docs/DECISIONS.md`.
   - Match each claim with code implementation (`file:line`).
   - Give verdict: confirmed (подтверждено), refuted (опровергнуто), or partially confirmed (частично подтверждено) with technical proof.
   - Any refuted or partial item becomes a new finding with priority >= original.
   - Build verification summary table in `FULL_AUDIT_REPORT.md`.
2. R2: Deep quality and security audit across 12 directions:
   - 1. Core (Rust) correctness: race conditions, spawn_blocking coverage, cancel handling, unwrap/expect/panics, I/O failure handling.
   - 2. Core IPC boundaries: all commands in `src-tauri/src/lib.rs`, command -> arg -> validation -> risk table, unvalidated path/string injection.
   - 3. Events & types contract: 3-way parity (src-tauri/src/events.rs <-> docs/UI-CONTRACT.md <-> src/api/types.ts).
   - 4. Frontend state correctness & leaks: game log batching, memoization, useShallow, unlisten/timer/listener cleanup.
   - 5. Frontend accessibility (a11y) & keyboard nav: modals (Esc, overlay, focus trap, return focus, role="dialog", aria), icon button accessible names, tabindex, e2e test selector parity.
   - 6. i18n quality, semantics & completeness: dictionary review (en, ru, uk), placeholders ({n}, {name}), pluralization, hardcoded strings, fallback errorText.
   - 7. Security & isolation: CSP config, IPC attack surface, drag-and-drop validation (.mrpack), native dialog filters & magic bytes sniffing, secret isolation & redaction filter, Tauri capabilities.
   - 8. Test coverage & reliability: top-10 uncovered core/UI branches, flaky tests, cargo test race conditions, mock-only tests.
   - 9. Performance: startup critical path, 100+ instances & hundreds of mods scalability, memory/log buffers & data-URLs, sysinfo overhead.
   - 10. Windows environment compatibility: long paths (>260 chars, long_path uniformity), spaces & non-ASCII/Cyrillic paths, file locking resilience (antivirus/OneDrive), hard link store creation/rollback.
   - 11. Documentation & release metadata: README.md claims vs real code, version sync (0.1.0) across tauri.conf.json <-> Cargo.toml <-> package.json <-> package-lock.json, NSIS/MSI metadata, docs/PROGRESS.md & docs/UI-CONTRACT.md updates.
   - 12. Licensing & brand: cargo dependencies MIT compatibility, NOTICE/THIRD-PARTY-LICENSES assessment, Mojang Minecraft brand guidelines, local asset/icon licensing.
3. R3: Output Artifacts:
   - Create `FULL_AUDIT_REPORT.md` in repository root (and ensure it is in `.gitignore`).
   - Update `AUDIT.md` with §7 "Полный аудит" referencing findings A1..A# and cross-referencing D#.
4. R4: Strict Constraints:
   - Product code in `src/` and `src-tauri/src/` MUST NOT be modified! `git diff` must be clean for product code files.
   - No git commands except saving audit artifacts. Work on `audit` branch from `master`.
   - Real game launch prohibited.
   - Strictly mask any tokens, secrets, or personal data.
