# Project: mc-launcher-v2 Feature Parity Audit & Static Bug Hunt

## Architecture & Scope
Independent, read-only feature parity audit and static bug hunt for `mc-launcher-v2` (Tauri 2 / Rust / React 19).
Benchmark against:
- Prism Launcher (Primary functional benchmark, GPLv3)
- Modrinth App (Tauri stack peer, UX & modpack catalog)
- ATLauncher (Instance management, templates, servers)
- GDLauncher (Loader UX, mod discovery)
- Official Minecraft Launcher (Baseline features, skins, news)
- CurseForge App (UX reference, grid & CF-style cards)
- TLauncher (Anti-pattern reference: opaque bundles, adware)

Deliverable:
`c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_REPORT.md`

## Feature Inventory & Requirement Mapping
| # | Requirement | Description | Assigned Agent | Status |
|---|-------------|-------------|----------------|--------|
| 1 | R1 (Inventory & Extension Points) | Codebase vs DECISIONS.md (D01–D35), UI-CONTRACT.md, AUDIT.md (§6, §7); discrepancies & extension points | explorer_inventory | DONE |
| 2 | R2 (Benchmark Investigation) | Research 7 launchers across 9 categories with dated URLs | explorer_benchmarks | DONE |
| 3 | R3 (Parity Matrix) | >=40 rows matrix with +, ~, -, ? and proofs (file:line & URLs) | explorer_benchmarks | DONE |
| 4 | R4 (Recommendations & Anti-features) | 15–30 cards F1..Fn, Anti-features, Top-10 | explorer_recommendations | DONE |
| 5 | R5 (Static Bug Hunt) | 5–25 confirmed bugs B1..Bn with file:line, race conditions, i18n, a11y, error handling | explorer_bughunt | DONE |
| 6 | R6 & Section 0–6 Assembly | Assemble comprehensive PARITY_AUDIT_REPORT.md conforming to structure | worker_report_writer | DONE |
| 7 | Verification & Gate | Reviewer and forensic auditor verification | reviewer_parity_1, reviewer_parity_2, auditor_parity_1 | DONE |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Deep Exploratory Research | Run 4 parallel explorers for R1, R2/R3, R4, R5 | none | DONE |
| M2 | Report Synthesis & Writing | Worker writes PARITY_AUDIT_REPORT.md at repo root | M1 | DONE |
| M3 | Review & Forensic Audit | 2 Reviewers + Forensic Auditor verify report integrity and constraints | M2 | DONE (PASS) |
| M4 | Final Delivery & Parent Handoff | Final reporting to parent agent | M3 | IN_PROGRESS |

## Code Layout & Read-Only Boundaries
- `src-tauri/src/` — Rust core (READ-ONLY)
- `src/` — React frontend (READ-ONLY)
- `docs/` — Documentation & Architecture decisions (READ-ONLY)
- `scripts/` — Acceptance scripts & test specifications (READ-ONLY)
- `.agents/teamwork/` — Agent coordination artifacts ONLY
- `PARITY_AUDIT_REPORT.md` — The ONLY permitted output file at repo root (CREATED & VERIFIED)
