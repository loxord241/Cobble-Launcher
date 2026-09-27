# BRIEFING — 2026-09-27T17:41:20Z

## Mission
Investigate Direction 4 (Frontend State & Leaks), Direction 5 (Frontend a11y & Navigation), and Direction 6 (i18n Quality & Completeness) for mc-launcher-v2.

## 🔒 My Identity
- Archetype: explorer
- Roles: frontend investigator, code auditor, i18n & a11y auditor
- Working directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster3
- Original parent: 99709a5d-49a1-4bfb-89f4-5a051b9f4dcb
- Milestone: Cluster 3 Frontend & i18n Investigation

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Product code in `src/` and `src-tauri/src/` MUST NOT be modified!
- No real game launches
- Strictly mask any tokens, secrets, or personal data
- Every claim and finding MUST cite exact `file:line` references

## Current Parent
- Conversation ID: 99709a5d-49a1-4bfb-89f4-5a051b9f4dcb
- Updated: 2026-09-27T17:41:20Z

## Investigation State
- **Explored paths**:
  - `src/state/instances.ts`, `src/state/settings.ts`, `src/state/accounts.ts`
  - `src/api/client.ts`, `src/api/types.ts`
  - `src/ui/layout/Shell.tsx`, `src/App.tsx`
  - `src/ui/pages/HomePage.tsx`, `src/ui/pages/InstancesPage.tsx`, `src/ui/pages/ModsPage.tsx`, `src/ui/pages/Onboarding.tsx`, `src/ui/pages/SettingsPage.tsx`
  - `src/ui/components/*.tsx` (all modals, rows, theme cards)
  - `src/i18n/{en,ru,uk}.json`, `src/i18n/errors.json`, `src/i18n/index.ts`
  - `scripts/*.cjs` (all E2E and acceptance suites)
  - `src-tauri/src/events.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/instances/run.rs`, `src-tauri/src/errors.rs`
- **Key findings**:
  - C3-1: `Shell.tsx:58` unselected `useInstances()` causes whole-tree re-render every 200 ms during game run.
  - C3-2: Race condition between `LaunchState::Exited` and `GameLogLine` causes lost tail lines in `crashAnalyze`.
  - C3-3: `HomePage.tsx` lacks D7 deletion dialog (still uses `window.confirm` with `wipe: false`) and kebab actions (backup, export, icon).
  - C3-4: `scripts/*.cjs` use hardcoded Russian selectors and break on default `en` clean installs.
  - C3-5: `LogViewerModal.tsx` falls back to `launcher.log` instead of Minecraft `latest.log`.
  - C3-6: `subscribeEvents` does not retain `unlisten`, leaking Tauri listeners permanently.
  - C3-7: All 7 modals lack focus trap and focus restoration; `CreateInstanceDialog` lacks Escape and overlay close.
  - C3-8: `SettingsPage.tsx` Toggle lacks accessible name (`aria-label`).
  - C3-9: `src/i18n/uk.json` misses `accounts.title` (225 vs 226 keys, refuting D30 parity claim).
  - C3-10: `uk.json` contains typos ("можно", "Тем"); hardcoded `"ru"` locale in `HomePage.tsx:155` and `ModsPage.tsx:464`.
  - C3-11: No pluralization engine; mixed-language error hints (`English.. Russian hint`).
- **Unexplored areas**: None within Cluster 3 scope.

## Key Decisions Made
- Fully documented all 15 discrete findings with code references, repro logic, and verification scripts in report.md and handoff.md.

## Artifact Index
- `DISPATCH.md` — Initial task dispatch
- `BRIEFING.md` — Persistent memory and identity
- `progress.md` — Liveness and task completion tracking
- `report.md` — Detailed comprehensive report for Directions 4, 5, 6
- `handoff.md` — 5-component handoff report for the orchestrator
