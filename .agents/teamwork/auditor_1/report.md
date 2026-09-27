# Forensic Integrity Verification Report: mc-launcher-v2 Audit

**Audit Date:** 2026-09-27  
**Auditor Archetype:** Forensic Auditor (`auditor_1`)  
**Integrity Mode:** Development (per `ORIGINAL_REQUEST.md`)  
**Target:** Full repository audit integrity verification (`mc-launcher-v2`)  
**Verdict:** **CLEAN** (No integrity violations detected)

---

## 1. Executive Summary

A comprehensive, read-only forensic audit of the `mc-launcher-v2` codebase, audit artifacts, and version control status was conducted to verify compliance with all constraints specified in `ORIGINAL_REQUEST.md` and the dispatch instructions.

All forensic checks passed unconditionally:
1. **Product Code Integrity:** Product source code in `src/` and `src-tauri/src/` has **NOT** been modified (0 files modified, 0 untracked files).
2. **Artifacts Verification:**
   - `FULL_AUDIT_REPORT.md` is present in the repository root (size: 102,126 bytes; 449 lines).
   - `.gitignore` contains rule `FULL_AUDIT_REPORT.md` (line 46), and `git check-ignore -v FULL_AUDIT_REPORT.md` returns code 0.
   - `AUDIT.md` preserves historical sections §1–§6 completely intact. Only section §7 ("7. Полный независимый аудит кодовой базы (2026-09-27)") was appended at the end of the file.
3. **Secret Leakage:** An exhaustive scan across `FULL_AUDIT_REPORT.md` and `AUDIT.md` confirmed that no secrets, credentials, API keys, passwords, or personal tokens were exposed.
4. **Authenticity & Ground Truth:** Direct verification of cited findings (including A1, A2, A4, A5, A6, A10, A36) against codebase files confirmed that findings are genuine, rigorously deduced, and grounded in actual code lines.
5. **Build and Test Integrity:** `npm run build` completed cleanly (0 errors), `cargo clippy` passed cleanly, and `cargo test --workspace` passed 112 tests (102 lib unit tests + 10 integration tests) without failure.

---

## 2. Forensic Phase Results

| # | Forensic Check | Expected Standard | Empirical Result | Status |
|---|---|---|---|---|
| **1** | Product code `src/` | 0 modified / untracked files | `git diff --stat src/` empty, `git ls-files` clean | **PASS** |
| **2** | Product code `src-tauri/src/` | 0 modified / untracked files | `git diff --stat src-tauri/src/` empty, `git ls-files` clean | **PASS** |
| **3** | Existence of `FULL_AUDIT_REPORT.md` | Present in project root | Exists (`Length: 102126`, `Lines: 449`) | **PASS** |
| **4** | Git ignore of `FULL_AUDIT_REPORT.md` | Ignored via `.gitignore` | `git check-ignore -v` returns `.gitignore:46:FULL_AUDIT_REPORT.md` (exit code 0) | **PASS** |
| **5** | Historical integrity of `AUDIT.md` | §1–§6 untouched; only §7 appended | Verified via `git diff -U0 AUDIT.md`. Diff begins exactly at line 250 with `## 7.` | **PASS** |
| **6** | Secrets & credentials exposure | Zero leaked tokens/keys | Scanned for JWT, API keys, private keys, bearer tokens. Zero leaks found. | **PASS** |
| **7** | Authenticity of findings | Findings grounded in real code | Empirical verification of A1, A2, A4, A5, A6, A10, A36 verified against actual lines. | **PASS** |
| **8** | Test & build verification | Clean build and test execution | `cargo test`: 112 passed; `clippy`: clean; `npm run build`: clean. | **PASS** |
| **9** | Prohibited patterns check | No facades, no hardcoded results | Logic is functional and genuine; no fake test mocks or facades found. | **PASS** |

---

## 3. Deep-Dive Empirical Evidence

### Check 1: Product Code Integrity
- **Command:** `git diff --stat src/`
  - **Output:** (empty stdout, exit code 0)
- **Command:** `git diff --stat src-tauri/src/`
  - **Output:** (empty stdout, exit code 0)
- **Command:** `git ls-files --others --exclude-standard src/ src-tauri/`
  - **Output:** (empty stdout, exit code 0)
- **Command:** `git status --porcelain`
  - **Output:**
    ```
     M AUDIT.md
    ?? .agents/
    ```
- **Conclusion:** Source code was strictly preserved without modification.

### Check 2: Artifacts Verification
- **Command:** `Get-Item "FULL_AUDIT_REPORT.md" | Select-Object Name, Length, LastWriteTime`
  - **Output:**
    ```
    Name                 Length LastWriteTime      
    ----                 ------ -------------      
    FULL_AUDIT_REPORT.md 102126 27.09.2026 19:49:22
    ```
- **Command:** `git check-ignore -v FULL_AUDIT_REPORT.md`
  - **Output:** `.gitignore:46:FULL_AUDIT_REPORT.md	FULL_AUDIT_REPORT.md` (exit code 0)
- **Command:** `git diff AUDIT.md | Select-String -Pattern "^\-[^\-]"`
  - **Output:**
    ```
    -3. Развитие headless-CLI (`bin/mcl.rs`) до полного экспорта/импорта образов инстансов.
    ```
    *(Single line replacement to append trailing newline before §7 header)*
  - Lines 1–248 of `AUDIT.md` are identical byte-for-byte to the master commit. All 99 added lines constitute section §7.

### Check 3: Secret Leakage Verification
- **Regex Patterns Checked:**
  - `(?i)(client_secret\s*[:=]\s*["'][^"']+["']|bearer\s+[a-zA-Z0-9_\-\.]{15,}|ghp_[a-zA-Z0-9]+|ey[A-Za-z0-9-_=]+\.ey[A-Za-z0-9-_=]+|BEGIN\s+(?:RSA|OPENSSH|EC|DSA|PRIVATE)\s+KEY)`
- **Results:** 0 occurrences in `FULL_AUDIT_REPORT.md` and `AUDIT.md`.
- All discussions regarding Windows Credential Manager, token redaction, OAuth secrets, and Ely.by credentials in the reports are strictly architectural/conceptual analysis and vulnerability assessments.

### Check 4: Finding Authenticity Verification
The following findings were empirically checked against the actual codebase:
1. **A1 (Path Traversal in `instance_delete`):**
   - Cited: `instances/mod.rs:291-314`, `commands/mod.rs:133-136`.
   - Verified: `instances::delete` directly calls `instance_dir(paths, id)` without calling `valid_id(&id)`, permitting directory traversal with relative paths like `..\backups` when `wipe: true`.
2. **A2 (Command Injection in `recycle_dir`):**
   - Cited: `instances/mod.rs:320-335`.
   - Verified: PowerShell script string interpolation `DeleteDirectory('{}', ...)` inserts `dir.display()` inside single quotes without apostrophe escaping, allowing command injection on folders containing single quotes.
3. **A4 (ZipSlip False Positive on long paths):**
   - Cited: `util/zip.rs:81-86`, `util/fs.rs:20-27`.
   - Verified: `long_path` only applies the `\\?\` prefix when `len() >= 240`. If `dest` length is `< 240` but `target` length is `>= 240`, `target.starts_with(&dest_long)` evaluates to `false`, causing legitimate modpack extractions to abort with a spurious `ZipSlip` error.
4. **A5 (Refutation of D15 / D29 - OAuth TcpListener leak):**
   - Cited: `commands/mod.rs:602-619`, `auth/msa.rs:322-362`.
   - Verified: `wait_auth_code` runs `for stream in listener.incoming()` inside `tokio::task::spawn_blocking` on a synchronous `std::net::TcpListener`. When `tokio::time::timeout` expires, the Tokio future is cancelled, but the blocking OS thread and socket remain open in `accept()`. Claim of shutdown on cancellation is refuted.
5. **A6 (JavaInstall missing `camelCase` rename):**
   - Cited: `src-tauri/src/java/detect.rs:9-17`, `src/ui/components/InstanceSettingsModal.tsx:165-166`.
   - Verified: `JavaInstall` has no `#[serde(rename_all = "camelCase")]`, serializing `java_exe` and `arch_bits`. Frontend accesses `j.javaExe` and `j.archBits`, rendering `undefined` in UI.
6. **A10 (HomePage delete instance bypass of D7):**
   - Cited: `src/ui/pages/HomePage.tsx:110-128`, `src/ui/pages/InstancesPage.tsx:58-60`.
   - Verified: `HomePage.tsx` uses native `window.confirm()` and hardcoded `wipe: false`, completely bypassing the custom modal dialog with recycle bin / permanent delete options.
7. **A36 (Missing key `accounts.title` in `uk.json`):**
   - Cited: `src/i18n/uk.json:120-125`, `docs/DECISIONS.md:337`.
   - Verified: `uk.json` has 228 lines / 225 keys, omitting `"accounts.title"`, which is present in `en.json:121`.

### Check 5: General Project Integrity Forensics
- **Hardcoded test outputs:** None.
- **Facade implementations:** None.
- **Pre-populated artifacts:** None.
- **Build & Test Verification:**
  - `npm run build`: Succeeded in 2.35s (1923 modules transformed, 0 TS errors).
  - `cargo clippy --workspace -- -D warnings`: Succeeded in 0.54s with 0 warnings.
  - `cargo test --workspace`: 112 passed (102 lib + 10 integration), 0 failed.

---

## 4. Final Forensic Assessment

The audit team adhered with strict fidelity to all project constraints and instructions. No product code was touched, all deliverables were created according to specification, claims are grounded in verifiable code facts, and no sensitive credentials were leaked.

**Final Verdict:** **CLEAN**
