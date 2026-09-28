# Handoff Report — Feature Recommendations & Architecture Strategy

**Agent:** `explorer_recommendations`  
**Working Directory:** `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations\`  
**Date:** 2026-09-28  
**Artifact:** `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations\findings.md`  

---

## 1. Observation
1. **Repository & Architecture State**:
   - Current commit: `dac1cab19771b1bb1a8cd7c998acba774e5cb400` (`git log -1`).
   - Rust core (`src-tauri/src/`): 47 IPC commands registered in `src-tauri/src/lib.rs` and implemented in `src-tauri/src/commands/mod.rs`. Key domains: `instances/mod.rs` (39.5 KB), `instances/run.rs` (42 KB), `instances/content.rs` (22.2 KB), `instances/store.rs` (12.6 KB), `settings.rs` (9.2 KB), `auth/` (ely, msa, offline, skins).
   - Frontend (`src/`): React 19 + TypeScript strict + Zustand (`useInstances`, `useAccounts`, `useSettings`, `useSkins`) + Tailwind CSS with «Сланец» design system (5 palettes via `data-theme`). Key pages: `HomePage.tsx`, `InstancesPage.tsx`, `InstanceDetailPage.tsx`, `ModsPage.tsx`, `SettingsPage.tsx`, `Onboarding.tsx`.
   - IPC Contract: Formatted in camelCase on both ends (`docs/UI-CONTRACT.md`, `DECISIONS.md` D25), error payloads `{code, message, hint}` mapped via `src/i18n/errors.json`.
2. **Iron Rules & Backlog Constraints (§1.3, §1.4)**:
   - Downloads exclusively from official endpoints with hash verification (SHA-1 / SHA-256).
   - Zero in-game bytecode injections (except pre-existing `authlib-injector` for Ely.by).
   - No fake UI elements; localized error codes; keyring secrets only; honest «нелицензионный» labels on offline accounts.
   - Slate theme visual redesign rejected by owner 4 times (palette changes only).
   - Known ceilings: MSA 403 registration policy, unsigned updater awaiting user keys (`scripts/gen-keys.ps1`), A45 icon data-URL schema change deferred, A17/A39/A44/A46/A48/A60 backlog acknowledged.
3. **Benchmarks Investigated**:
   - Prism Launcher (GPLv3 reference for instances, JVM args, logs, integrity check, offline toggle, quick play).
   - Modrinth App (Tauri reference for catalog UX, datapacks, modpack installation).
   - ATLauncher (presets, export/import, shortcuts, safe updates).
   - GDLauncher (loader switching, drag-and-drop).
   - Official Minecraft Launcher & CurseForge App (baseline and card UX reference).

---

## 2. Logic Chain
1. *From Observation 1 (Architecture & Offline Priority)*: The primary user scenario is offline/autonomous play with local modpacks and private servers. Therefore, features that eliminate network hangs, speed up launch, and protect offline savegames provide the highest value.
2. *From Observation 1 & 2 (Process Launch & Arguments)*: `src-tauri/src/instances/run.rs:297-332` constructs game arguments directly via `game_args_extra` and passes them to the Minecraft process. Inserting native Mojang `--quickPlayMultiplayer` and `--quickPlaySingleplayer` arguments (F1) is a clean, low-effort addition that directly benefits offline players joining local worlds or LAN servers without touching UI game menus.
3. *From Observation 1 & 2 (Content & Safety)*: `src-tauri/src/instances/content.rs` already manages mods with `.disabled` suffixes and manifests. Implementing pre-update snapshot/rollback (F8) and integrity verification (F2) uses existing `store.rs` hardlinking and hash check primitives, safeguarding user modpacks without violating official download rules.
4. *From Observation 2 (Iron Rules vs Market Anti-features)*: Common industry practices like scraping CurseForge without an API key, distributing repack torrents, snake-oil RAM cleaners, and injecting vanilla offline skins directly contradict Iron Rules #1, #2, and #3. Explicitly classifying them as «Анти-фичи» protects project integrity and legal standing.
5. *From Observations 1–3*: Formulating 30 structured cards (F1–F30) with explicit offline impact, code extension points, and effort estimates provides a complete strategic roadmap for future milestones.

---

## 3. Caveats
- GPLv3 benchmarks were examined solely for behavioral specifications and public documentation; zero benchmark code was copied or incorporated into recommendations.
- No source code modifications were performed in `src/` or `src-tauri/`, preserving strict read-only compliance.
- No build commands (`cargo build`, `npm run dev`) or intrusive scripts were executed.
- Quick Play CLI arguments (`--quickPlayMultiplayer`, `--quickPlaySingleplayer`) require Minecraft 1.20+. For legacy versions (<1.20), UI must gracefully disable the quick play action.

---

## 4. Conclusion
1. **Deliverables Completed**:
   - Detailed findings report written to `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations\findings.md`.
   - 30 fully structured recommendation cards (F1–F30) across all 9 audit categories.
   - Comprehensive «Анти-фичи» section detailing 8 rejected market patterns.
   - Value-ranked Top-10 recommendations tailored to offline profiles and project stability.
   - 3-phase architectural roadmap (Phase 1: Quick Wins & Offline Comfort; Phase 2: Safety & Integrity; Phase 3: Power Tools & Content Management).
2. **Top-3 Immediate Priorities**:
   - **F16 (Work Offline Toggle)**: Eliminates socket timeouts on flaky connections.
   - **F2 (Verify Integrity & Repair)**: One-click recovery of corrupted game files without save loss.
   - **F1 (Quick Play)**: Native direct-to-world / direct-to-server launching.

---

## 5. Verification Method
1. **File Inspection**:
   - Inspect `findings.md` at `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations\findings.md`: verify exactly 30 cards formatted with all mandatory fields (`Что делает у эталона`, `Зачем нам` + offline point, `Как ляжет в код`, `Усилие`, `Приоритет`, `Риски/зависимости`, `Совместимость с железными правилами`), the 8-item Anti-features section, and the Top-10 ranking.
2. **Repository Cleanliness Check**:
   - Run `git status --porcelain` to verify that no product code (`src/`, `src-tauri/`, `docs/`, `package.json`, `Cargo.toml`) was modified. Only files inside `.agents/teamwork/explorer_recommendations/` have been written.
3. **Iron Rule Invalidation Condition**:
   - If any recommendation proposes in-game bytecode injection, unofficial unhashed downloads, visual Slate redesign, or secret storage outside keyring, that recommendation must be immediately invalidated. All 30 cards have been verified compliant.
