## 2026-09-28T11:22:21Z

Conduct a comprehensive read-only feature parity audit and static bug hunt for the Tauri 2 / Rust / React Minecraft launcher (mc-launcher-v2), benchmarking against leading open-source launchers (Prism Launcher, Modrinth App, ATLauncher, GDLauncher) and producing a detailed Markdown report at PARITY_AUDIT_REPORT.md.

Working directory: C:\Users\Admin\.zcode\workspace\default\mc-launcher-v2
Integrity mode: development

Target Deliverable:
Produce a comprehensive, rigorous Markdown report at c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_REPORT.md conforming to all requirements (R1–R6, Stages 0–8, sections 0–6, acceptance criteria).

Strict Constraints:
1. STRICT READ-ONLY MODE on product codebase: DO NOT edit or modify any source code in src/ or src-tauri/, tests, or config files.
2. DO NOT run cargo build, cargo test, npm run dev, npm run build, or launch the launcher/scripts. Use static code analysis, ripgrep / regex searches, and file inspection.
3. DO NOT run modifying git commands (git commit, git checkout, etc.).
4. The ONLY permitted output file to write to the repository root is PARITY_AUDIT_REPORT.md. All agent coordination files (plans, findings, handoffs) MUST be placed in dedicated subdirectories under .agents/teamwork/.
5. DO NOT copy any GPLv3 code from benchmarks/reference launchers into the report.
6. DO NOT propose visual theme redesigns for the Slate theme.
7. DO NOT reopen known ceilings/backlog items (§1.4 / known issues: MSA 403, unsigned auto-updater, A17, A45, A46, etc.) as new bugs.
8. Maintain your progress.md and BRIEFING.md in orchestrator_2/.
