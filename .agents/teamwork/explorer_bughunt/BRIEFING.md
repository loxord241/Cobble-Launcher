# BRIEFING — 2026-09-28T13:38:00Z

## Mission
Static Bug Hunter & Reliability Auditor: Conduct deep static analysis of src/ and src-tauri/ to identify 5-25 real, confirmed bugs (B1..Bn) across race conditions, error handling, i18n, a11y/CDP contract, and edge cases.

## 🔒 My Identity
- Archetype: explorer
- Roles: investigator, auditor, synthesizer
- Working directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_bughunt
- Original parent: 77618110-4f34-4be7-94e0-5e525096032d
- Milestone: static_bug_hunt_and_reliability_audit

## 🔒 Key Constraints
- Read-only investigation — do NOT implement or modify product files in src/ or src-tauri/
- Write ONLY inside c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_bughunt\
- DO NOT run cargo build, cargo test, npm run dev, npm run build, or launch launcher/scripts
- DO NOT reopen known ceilings and backlog items from §1.4 (MSA 403, unsigned auto-updater, A45 data-URL, A17, A39, A44, A46, A48, A60, A16-tail, Installed tab file names, Versions/Changelog tabs, Modrinth filter/sort, saves/worlds, MC version change, mod copy, instance grouping, MSA in-game skins)
- Identify ONLY confirmed, real bugs verified directly by reading code with eyes (lesson D15)
- Format bugs as: B<n>. [P?] файл:строка — сценарий → ожидание vs факт

## Current Parent
- Conversation ID: 77618110-4f34-4be7-94e0-5e525096032d
- Updated: 2026-09-28T13:38:00Z

## Investigation State
- **Explored paths**:
  - `src-tauri/src/commands/mod.rs` (instance lifecycle, engine registry, concurrency, error mapping)
  - `src-tauri/src/instances/mod.rs`, `run.rs`, `content.rs` (content toggling, lock management, process supervision)
  - `src-tauri/src/auth/mod.rs` (account deletion, launch identity resolution)
  - `src/ui/components/InstanceCard.tsx`, `InstanceSettingsModal.tsx`, `CreateInstanceDialog.tsx`, `InstanceKebabMenu.tsx`
  - `src/ui/pages/HomePage.tsx`, `InstancesPage.tsx`, `InstanceDetailPage.tsx`, `SettingsPage.tsx`
  - `src/state/instances.ts`, `accounts.ts`, `settings.ts`
  - `scripts/*.cjs` (all 10 acceptance and test driver scripts)
  - `docs/DECISIONS.md`, `docs/UI-CONTRACT.md`, `AUDIT.md`, `PARITY_AUDIT_PROMPT.md`
- **Key findings**:
  - Identified 12 real, verified bugs (B1–B12): 7 P1, 4 P2, 1 P3.
  - Concurrency/race conditions (B5 TOCTOU in launch/kill, B6 non-atomic instance save).
  - Resource leaks & dangling pointers (B4 engine leak in launch, B3 dangling active account).
  - Test contract drifts (B1 selector breakage in ui_checks/e2e_features, B2 clean install locale mismatch in m4/m5/kill).
  - OS-specific failure modes (B9 Windows file locks on active JARs, B10 phantom folders).
  - Logic/i18n defects (B8 inverted condition in toggle, B11 apiErrorText bypass, B12 hardcoded Russian constant).
- **Unexplored areas**:
  - None within the assigned audit scope.

## Key Decisions Made
- Confirmed bugs only through verbatim source code verification (no speculative findings).
- Avoided all known ceiling / backlog items (§1.4).
- Compiled full report into `findings.md` and 5-component summary into `handoff.md`.

## Artifact Index
- `DISPATCH.md` — incoming dispatch instructions and prompt history
- `BRIEFING.md` — persistent working memory and identity
- `progress.md` — liveness heartbeat
- `findings.md` — full detailed bug reports B1–B12
- `handoff.md` — formal 5-component handoff report
