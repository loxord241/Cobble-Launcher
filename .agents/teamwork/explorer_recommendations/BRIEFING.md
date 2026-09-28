# BRIEFING — 2026-09-28T11:29:30Z

## Mission
Formulate 15 to 30 structured recommendation cards (F1..Fn) for high-value features missing in mc-launcher-v2, define Anti-features, select Top-10 by value, and document findings and handoff reports.

## 🔒 My Identity
- Archetype: explorer
- Roles: Feature Recommendations & Architecture Strategist
- Working directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations
- Original parent: 77618110-4f34-4be7-94e0-5e525096032d
- Milestone: Stage 4 / Recommendations

## 🔒 Key Constraints
- Read-only investigation — do NOT implement or modify source code in src/ or src-tauri/, tests, or configs
- DO NOT run cargo build, cargo test, npm run dev, npm run build, or launch launcher/scripts
- Write ONLY inside assigned directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations\
- DO NOT copy GPLv3 code from benchmarks. Describe ideas and behavior only in your own words.
- DO NOT propose visual theme redesigns for Slate theme (owner rejected visual redesigns 4 times)
- DO NOT propose features that violate §1.3 Iron Rules:
  * Official server downloads with hash verification only
  * Zero in-game injections (except existing ely authlib-injector)
  * No fake UI, errors via error codes localized in UI
  * Keyring secrets only
  * Offline profiles honestly marked "нелицензионный"
  * Crate name mc-launcher-v2, identifier dev.mclauncher.v2, and data folder unchanged

## Current Parent
- Conversation ID: 77618110-4f34-4be7-94e0-5e525096032d
- Updated: 2026-09-28T11:29:30Z

## Investigation State
- **Explored paths**:
  * `src-tauri/src/instances/` (`mod.rs`, `run.rs`, `content.rs`, `store.rs`)
  * `src-tauri/src/commands/mod.rs` (47 IPC handlers)
  * `src-tauri/src/auth/` (`mod.rs`, `ely.rs`, `msa.rs`, `offline.rs`, `skins.rs`)
  * `src-tauri/src/settings.rs`
  * `src/ui/pages/` (`InstanceDetailPage.tsx`, `HomePage.tsx`, `ModsPage.tsx`, `SettingsPage.tsx`)
  * `docs/DECISIONS.md` (D01–D35), `docs/UI-CONTRACT.md`, `AUDIT.md` (§1, §3, §6, §7)
  * Benchmarks: Prism Launcher, Modrinth App, ATLauncher, GDLauncher, Official Launcher, CurseForge App
- **Key findings**:
  * Formulated 30 structured recommendation cards (F1–F30) covering instances, content, accounts, Java, downloads/offline, UI/UX, logs/crash, saves, and system integration.
  * Formulated 8 Anti-features rejected for technical, legal, and Iron Rule reasons.
  * Ranked Top-10 recommendations prioritized for the owner's primary offline scenario.
  * Proposed 3-phase architectural roadmap.
- **Unexplored areas**: None for Stage 4; ready for handoff.

## Key Decisions Made
- Prioritize offline resilience (F16 Work Offline toggle, F2 Integrity Repair, F1 Quick Play, F8 Safe Updates with Undo) as top architectural value drivers.
- Completely reject in-game injection bypasses, unkeyed CurseForge scrapers, and snake-oil RAM boosters.

## Artifact Index
- findings.md — Detailed report containing 30 recommendation cards, anti-features, Top-10 ranking, and implementation phases.
- handoff.md — Standard 5-Component Handoff Report.
- progress.md — Heartbeat and status log.
- DISPATCH.md — Stored prompt instructions.
