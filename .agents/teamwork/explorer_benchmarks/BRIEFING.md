# BRIEFING — 2026-09-28T11:30:20Z

## Mission
Conduct comprehensive investigation of 7 reference launchers across 9 categories and construct a verified >=40-row feature parity matrix with exact mc-launcher-v2 code locations (file:line) and dated benchmark URLs.

## 🔒 My Identity
- Archetype: explorer
- Roles: Benchmark & Parity Matrix Specialist
- Working directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_benchmarks
- Original parent: 77618110-4f34-4be7-94e0-5e525096032d
- Milestone: Stage 2 (Benchmarks) & Stage 3 (Parity Matrix)

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Strict read-only on product codebase (no edits in src/ or src-tauri/, tests, configs)
- DO NOT run cargo build, cargo test, npm run dev/build, or launch scripts
- Write ONLY in assigned working directory
- DO NOT copy GPLv3 code from benchmarks (Prism/MultiMC); describe in own words
- All benchmark URLs must be dated. If inaccessible, mark "не проверено (источник недоступен)"
- Parity Matrix with at least 40 rows comparing mc-launcher-v2, Prism, Modrinth App, ATLauncher, GDLauncher (+, ~, -, ?)
- Provide concrete evidence (file:line for mc-launcher-v2, dated URL for benchmarks)
- Output findings.md and handoff.md, notify caller

## Current Parent
- Conversation ID: 77618110-4f34-4be7-94e0-5e525096032d
- Updated: 2026-09-28T11:30:20Z

## Investigation State
- **Explored paths**:
  - `src-tauri/src/lib.rs` (48 IPC commands, logging, CSP)
  - `src-tauri/src/commands/mod.rs` (all command handlers, AppState)
  - `src-tauri/src/instances/mod.rs` (Instance struct, list, rename, duplicate, delete)
  - `src-tauri/src/instances/store.rs` (CAS hardlink store)
  - `src-tauri/src/instances/run.rs` (launching, CREATE_NO_WINDOW, supervision, stats)
  - `src-tauri/src/instances/content.rs` (content manifest, toggle, remove, export_mrpack, backup_zip, icons)
  - `src-tauri/src/auth/` (offline, msa, ely, skins)
  - `src-tauri/src/net/` (download engine, http, proxy)
  - `src-tauri/src/java/` (detect, adoptium)
  - `src-tauri/src/process/` (crash diagnosis rules, process supervision)
  - `src-tauri/src/util/` (redaction, fs, names)
  - `src/ui/`, `src/theme/`, `src/state/`, `src/i18n/`
- **Key findings**:
  - Complete architectural mapping of 7 benchmarks: Prism Launcher, Modrinth App, ATLauncher, GDLauncher, Official Minecraft Launcher, CurseForge App, TLauncher.
  - Constructed 73-row Parity Matrix with exact file:line citations and dated URLs as of 2026-09-28.
  - Identified core competitive advantages: CAS hardlinking, token redaction in logs, JVM flag blacklist, procedural fallbacks.
  - Identified strategic gaps: Instance grouping/tagging, world/save manager, in-place version changing, mclo.gs paste integration.
- **Unexplored areas**: None within the scope of Stage 2 & Stage 3.

## Key Decisions Made
- Expanded parity matrix from required 40 rows to 73 rows to exhaustively cover all 9 categories with concrete code proof.
- Classified internal engine methods without UI/IPC exposure (like DownloadEngine::pause) strictly as unsupported (`-`) to maintain user-facing parity integrity.

## Artifact Index
- .agents/teamwork/explorer_benchmarks/DISPATCH.md — Initial dispatch instructions
- .agents/teamwork/explorer_benchmarks/BRIEFING.md — Persistent context & memory
- .agents/teamwork/explorer_benchmarks/progress.md — Liveness heartbeat and milestone tracker
- .agents/teamwork/explorer_benchmarks/findings.md — Full benchmark investigation and parity matrix report
- .agents/teamwork/explorer_benchmarks/handoff.md — 5-component handoff report
