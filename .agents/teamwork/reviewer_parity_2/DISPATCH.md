## 2026-09-28T11:44:07Z

You are Reviewer 2 (Constraint & Policy Reviewer) for mc-launcher-v2.
Your working directory is:
c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\reviewer_parity_2

MANDATORY INPUT FILES TO READ:
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_REPORT.md
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_PROMPT.md
- c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\ORIGINAL_REQUEST.md

STRICT CONSTRAINTS:
1. Strict read-only mode on product codebase. DO NOT edit any source code.
2. DO NOT run cargo build, cargo test, npm run dev, npm run build, or launch launcher/scripts.
3. Write ONLY in your working directory: c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\reviewer_parity_2\

TASK OBJECTIVES:
1. Conduct an adversarial review of c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\PARITY_AUDIT_REPORT.md specifically targeting non-negotiable project constraints:
   - Check that NO GPLv3 code was copied into the report from benchmarks (Prism/MultiMC).
   - Check that NO visual theme redesigns for the Slate theme were proposed.
   - Check that NO known ceilings or backlog items from §1.4 (MSA 403, unsigned updater, A45 icon data-URLs, A17, A39, A44, A46, A48, A60) were reopened as new bugs in Section 5.
   - Check that all benchmark URLs are dated (2026-09-28).
   - Check that all §1.3 Iron Rules are strictly respected in all recommendations F1–F30.
2. Verify git status cleanliness to ensure no product code was touched.
3. Write your detailed review and handoff to:
   c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\reviewer_parity_2\handoff.md
   clearly stating your verdict: APPROVE or REQUEST_CHANGES.

When finished, send a message to orchestrator_2.
