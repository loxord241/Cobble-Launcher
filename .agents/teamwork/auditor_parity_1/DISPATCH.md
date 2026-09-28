## 2026-09-28T11:44:07Z

You are the Forensic Integrity Auditor for mc-launcher-v2.
Your working directory is:
c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\auditor_parity_1

MANDATORY INPUT FILES TO READ:
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_REPORT.md
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_PROMPT.md
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\ORIGINAL_REQUEST.md

STRICT CONSTRAINTS:
1. Strict read-only mode on product codebase. DO NOT edit any source code.
2. DO NOT run cargo build, cargo test, npm run dev, npm run build, or launch launcher/scripts.
3. Write ONLY in your working directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\auditor_parity_1\

TASK OBJECTIVES:
1. Execute a comprehensive forensic integrity audit:
   - Verify `git status --porcelain`: ensure that NO product source files in src/, src-tauri/, docs/, scripts/, package.json, Cargo.toml are modified.
   - Verify that PARITY_AUDIT_REPORT.md is present at the repository root and is ignored by git (`git check-ignore -v PARITY_AUDIT_REPORT.md`).
   - Spot-check 10 random file:line citations across Section 2, Section 3, and Section 5 in PARITY_AUDIT_REPORT.md by viewing the actual source files with view_file to confirm they are authentic and physically exist.
   - Check for any presence of copied GPLv3 code, fake UI proposals, or fabricated test results.
   - Verify that Section 5 bugs B1–B12 are genuine bugs and do not contain any known ceilings from §1.4.
2. Write your forensic audit report to:
   c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\auditor_parity_1\handoff.md
   clearly stating your binary verdict: CLEAN or INTEGRITY VIOLATION.

When finished, send a message to orchestrator_2.
