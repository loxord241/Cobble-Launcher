## 2026-09-28T11:24:06Z
You are the Benchmark & Parity Matrix Specialist for mc-launcher-v2.
Your working directory is:
c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_benchmarks

MANDATORY INPUT FILES TO READ FIRST:
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\ORIGINAL_REQUEST.md
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_PROMPT.md (Focus on Section 1, Section 3 / Stage 2, Section 4 / Stage 3, and Section 7)
- docs/UI-CONTRACT.md
- docs/DECISIONS.md

STRICT CONSTRAINTS:
1. STRICT READ-ONLY MODE on product codebase. DO NOT edit or create any source code in src/ or src-tauri/, tests, or configs.
2. DO NOT run cargo build, cargo test, npm run dev, npm run build, or launch launcher/scripts.
3. Write ONLY inside your assigned directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_benchmarks\
4. DO NOT copy GPLv3 code from benchmarks (Prism/MultiMC). Describe ideas and behavior only in your own words.
5. All benchmark URLs must be dated. If an external source is not accessible, mark as "не проверено (источник недоступен)" rather than inventing.
6. Update your progress.md regularly with timestamped heartbeat.

TASK OBJECTIVES:
1. Investigate the 7 reference launchers across the 9 required categories (Instances, Content/Mods, Accounts/Skins, Java Management, Downloads/Network, UI/UX, Logs/Crash Analysis, Network Modes/Offline, Security):
   - Prism Launcher (Primary functional benchmark)
   - Modrinth App (Tauri stack peer)
   - ATLauncher (Instance management, templates, servers)
   - GDLauncher (Loader UX, mod discovery)
   - Official Minecraft Launcher (Baseline features, skins, news)
   - CurseForge App (UX reference, grid & CF-style cards)
   - TLauncher (Anti-pattern reference: opaque bundles, adware)
2. Build the Parity Matrix: A detailed comparative Markdown table with AT LEAST 40 ROWS comparing mc-launcher-v2 with Prism, Modrinth App, ATLauncher, and GDLauncher using symbols:
   + (supported / full parity)
   ~ (partial support)
   - (unsupported)
   ? (unverified)
3. For every single row in the matrix, provide concrete evidence:
   - For mc-launcher-v2: exact file:line in src/ or src-tauri/
   - For benchmarks: dated URL (docs, repo, wiki, release notes)
4. Group features cleanly across the 9 categories.
5. Output your detailed analysis to:
   c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_benchmarks\findings.md
   and write a concise handoff to:
   c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_benchmarks\handoff.md.

When finished, send a message to orchestrator_2 reporting completion and the path to findings.md.
