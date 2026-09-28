## 2026-09-28T11:23:44Z
<USER_REQUEST>
You are the Static Bug Hunter & Reliability Auditor for mc-launcher-v2.
Your working directory is:
c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_bughunt

MANDATORY INPUT FILES TO READ FIRST:
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\ORIGINAL_REQUEST.md
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_PROMPT.md (Focus on Section 1, Section 6 / Stage 5, and Section 7)
- docs/UI-CONTRACT.md
- docs/DECISIONS.md
- scripts/*.cjs (Read to understand the CDP acceptance contract for aria-labels and selectors)
- AUDIT.md (§6 discovery backlog D1–D29, §7 full audit A1–A60)

STRICT CONSTRAINTS:
1. STRICT READ-ONLY MODE on product codebase. DO NOT edit or create any source code in src/ or src-tauri/, tests, or configs.
2. DO NOT run cargo build, cargo test, npm run dev, npm run build, or launch launcher/scripts.
3. Write ONLY inside your assigned directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_bughunt\
4. DO NOT reopen known ceilings and backlog items from §1.4!
   - MSA 403 (Mojang app registration block) is a known ceiling, NOT a bug.
   - Unsigned auto-updater / installer (SmartScreen) is a known owner step, NOT a bug.
   - A45 (data-URL in instance.json) is an owner architectural decision, NOT a new bug.
   - Open backlog items: A17, A39, A44, A46, A48, A60, A16-tail, Installed tab file names, Versions/Changelog tabs, Modrinth filter/sort, saves/worlds, MC version change, mod copy, instance grouping, MSA in-game skins. These are KNOWN — DO NOT report them as bugs.
5. Identify ONLY confirmed, real bugs verified directly by reading code with eyes (lesson D15).
6. Update your progress.md regularly with timestamped heartbeat.

TASK OBJECTIVES:
1. Perform deep static code analysis of src-tauri/ and src/ for 5 to 25 real, confirmed bugs (B1..Bn):
   - Race conditions: event subscriptions vs unmount, list reloads, concurrent operations, kill vs launch.
   - Error handling: swallowed try/catch, missing error UI states where operations can fail (fake UI violation).
   - i18n: missing keys across en.json, ru.json, uk.json, errors.json; hardcoded Russian text in TSX/JSX (outside comments); unreplaced {name} interpolations.
   - a11y & CDP acceptance test contracts: verify aria-labels, test IDs, and selectors in scripts/*.cjs vs current JSX/TSX. Any mismatch is a contract break!
   - Edge cases & data consistency: 0 instances, empty/long names, offline without network (which sections show misleading status), non-existent active profile, path injection, data corruption.
2. Format each bug strictly as:
   B<n>. [P?] файл:строка — сценарий → ожидание vs факт
   Where P1 = functional failure/data loss, P2 = defect/risk, P3 = quality/cosmetics.
3. Output your detailed analysis to:
   c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_bughunt\findings.md
   and write a concise handoff to:
   c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_bughunt\handoff.md.

When finished, send a message to orchestrator_2 reporting completion and the path to findings.md.
</USER_REQUEST>
