## 2026-09-28T11:23:44Z
You are the Feature Recommendations & Architecture Strategist for mc-launcher-v2.
Your working directory is:
c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations

MANDATORY INPUT FILES TO READ FIRST:
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\ORIGINAL_REQUEST.md
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_PROMPT.md (Focus on Section 1, Section 5 / Stage 4, and Section 7)
- docs/DECISIONS.md (Decisions D01–D35)
- docs/UI-CONTRACT.md

STRICT CONSTRAINTS:
1. STRICT READ-ONLY MODE on product codebase. DO NOT edit or create any source code in src/ or src-tauri/, tests, or configs.
2. DO NOT run cargo build, cargo test, npm run dev, npm run build, or launch launcher/scripts.
3. Write ONLY inside your assigned directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations\
4. DO NOT copy GPLv3 code from benchmarks. Describe ideas and behavior only in your own words.
5. DO NOT propose visual theme redesigns for the Slate theme (owner rejected visual redesigns 4 times).
6. DO NOT propose features that violate §1.3 Iron Rules:
   - Official server downloads with hash verification only
   - Zero in-game injections (except existing ely authlib-injector)
   - No fake UI, errors via error codes localized in UI
   - Keyring secrets only
   - Offline profiles honestly marked "нелицензионный"
   - Crate name mc-launcher-v2, identifier dev.mclauncher.v2, and data folder unchanged
7. Update your progress.md regularly with timestamped heartbeat.

TASK OBJECTIVES:
1. Formulate 15 to 30 structured recommendation cards (F1..Fn) for high-value features missing in mc-launcher-v2.
2. Each card MUST follow the exact format:
   ### F<n>. <Название фичи>
   - Что делает у эталона: <поведение своими словами, без копирования GPL-кода; эталоны: ...>
   - Зачем нам: <ценность; отдельным пунктом — ценность для ОФЛАЙН-профилей владельца, основной сценарий лаунчера>
   - Как ляжет в код: <точки расширения: файлы/модули/структуры>
   - Усилие: S (<1 дня) / M (1–3 дня) / L (неделя+)
   - Приоритет: P1 (заметно улучшает ежедневный сценарий) / P2 (полезно) / P3 (нишевое)
   - Риски/зависимости: <миграция, API, совместимость>
   - Совместимость с железными правилами: <да/нет — если нет, почему>
3. Formulate the "Анти-фичи" (Anti-features) section: What benchmarks have that we explicitly reject and why (e.g. unkeyed CurseForge API scraping, torrents/bundles, RAM boosters, in-game skin injection bypasses, unsigned auto-updater, news widgets, embedded browsers, etc.).
4. Rank and select the Top-10 recommendations by value.
5. Output your detailed analysis to:
   c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations\findings.md
   and write a concise handoff to:
   c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_recommendations\handoff.md.

When finished, send a message to orchestrator_2 reporting completion and the path to findings.md.
